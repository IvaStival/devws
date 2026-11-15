//! Bounded worker startup selection. Retry state survives dispatcher restarts.
use crate::agent_args::Kind;
use crate::devws::Devws;
use crate::roles::{Fallback, Role};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const LIMIT_PERCENT: u32 = 85;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureScope {
    Account,
    Model,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartupFailure {
    pub scope: FailureScope,
    pub reason: String,
}

impl std::fmt::Display for StartupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.reason)
    }
}
impl std::error::Error for StartupFailure {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaunchState {
    pub role: Role,
    pub candidates: Vec<Fallback>,
    pub next: usize,
    pub selected: Option<Fallback>,
    pub failures: Vec<String>,
    pub blocked_accounts: Vec<String>,
}

impl LaunchState {
    pub fn new(role: &Role, kind: Kind) -> Self {
        Self {
            role: role.clone(),
            candidates: role.worker_candidates(kind),
            next: 0,
            selected: None,
            failures: Vec::new(),
            blocked_accounts: Vec::new(),
        }
    }

    pub fn rejected(&mut self, failure: &StartupFailure) {
        if let Some(selected) = self.selected.take() {
            self.failures
                .push(format!("{}: {}", program_name(&selected), failure.reason));
            if failure.scope == FailureScope::Account {
                self.blocked_accounts.push(selected.agent);
            }
        }
    }

    pub fn next_available(&mut self, devws: &dyn Devws) -> Result<Role> {
        self.selected = None;
        while let Some(candidate) = self.candidates.get(self.next).cloned() {
            self.next += 1;
            if self.blocked_accounts.contains(&candidate.agent) {
                self.failures
                    .push(format!("{}: account unavailable", program_name(&candidate)));
                continue;
            }
            if let Err(error) = devws.agent_kind(&candidate.agent) {
                self.failures
                    .push(format!("{}: {error}", program_name(&candidate)));
                continue;
            }
            let limits = devws.limits(&candidate.agent)?;
            let exhausted = [("week", limits.week), ("5-hour limit", limits.five_hour)]
                .into_iter()
                .find_map(|(window, used)| {
                    used.filter(|used| *used >= LIMIT_PERCENT)
                        .map(|used| (window, used))
                });
            if let Some((window, used)) = exhausted {
                self.failures.push(format!(
                    "{} has used {used}% of its {window}: no new agents on it until it resets",
                    candidate.agent
                ));
                self.blocked_accounts.push(candidate.agent);
                continue;
            }
            let role = self.role.with_program(&candidate);
            self.selected = Some(candidate);
            return Ok(role);
        }
        bail!(
            "no available worker candidate: {}",
            self.failures.join("; ")
        )
    }
}

pub fn program_name(program: &Fallback) -> String {
    format!(
        "{} / {}",
        program.agent,
        program.model.as_deref().unwrap_or("default model")
    )
}

/// Only provider error envelopes are evidence. Chat/tool text is never inspected.
pub fn failure_from_record(record: &Value) -> Option<StartupFailure> {
    let error = if record["type"] == "assistant" && record["isApiErrorMessage"] == true {
        record
    } else if record["type"] == "event_msg" && record["payload"]["type"] == "error" {
        &record["payload"]
    } else if record["type"] == "error" && record["error"].is_object() {
        // An explicit API error envelope (also accepted from a failed launch).
        record
    } else {
        return None;
    };
    let code = error["error"]["code"]
        .as_str()
        .or_else(|| error["error"]["type"].as_str())
        .or_else(|| error["error"].as_str())
        .or_else(|| error["codex_error_info"].as_str())
        .or_else(|| error["code"].as_str())
        .unwrap_or("unknown_provider_error");
    let scope = match code {
        "rate_limit"
        | "rate_limit_error"
        | "rate_limit_exceeded"
        | "usage_limit_reached"
        | "insufficient_quota"
        | "quota_exceeded"
        | "overloaded_error"
        | "server_overloaded"
        | "provider_unavailable" => FailureScope::Account,
        "model_not_found" | "model_not_available" | "model_unavailable" | "unsupported_model" => {
            FailureScope::Model
        }
        _ => FailureScope::Other,
    };
    // Provider codes are enough to explain a rejection without copying prompts or credentials.
    Some(StartupFailure {
        scope,
        reason: format!("provider startup error: {code}"),
    })
}

/// Startup transcripts are small; cap reads and ignore an incomplete last record.
pub fn transcript_failure(path: &Path) -> Result<Option<StartupFailure>> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let length = file.metadata()?.len();
    let offset = length.saturating_sub(256 * 1024);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(256 * 1024).read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut latest = None;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if (offset > 0 && index == 0) || !line.ends_with('\n') {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(failure) = failure_from_record(&record) {
            latest = Some(failure);
        } else if record["type"] == "assistant"
            || (record["type"] == "event_msg" && record["payload"]["type"] == "task_complete")
        {
            latest = None;
        }
    }
    Ok(latest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_structured_provider_errors_can_trigger_fallback() {
        let quota = json!({"type":"assistant","isApiErrorMessage":true,"error":"rate_limit"});
        assert_eq!(
            failure_from_record(&quota).unwrap().scope,
            FailureScope::Account
        );
        let model = json!({"type":"event_msg","payload":{"type":"error","codex_error_info":"model_not_found"}});
        assert_eq!(
            failure_from_record(&model).unwrap().scope,
            FailureScope::Model
        );
        for record in [
            json!({"type":"user","error":"rate_limit"}),
            json!({"type":"assistant","message":{"content":"quota_exceeded"}}),
            json!({"type":"tool_result","error":"model_not_found"}),
        ] {
            assert!(failure_from_record(&record).is_none());
        }
        let generic = json!({"type":"error","error":{"type":"authentication_error"}});
        assert_eq!(
            failure_from_record(&generic).unwrap().scope,
            FailureScope::Other
        );
    }

    #[test]
    fn transcript_reads_complete_records_and_latest_response_only() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let error =
            "{\"type\":\"assistant\",\"isApiErrorMessage\":true,\"error\":\"rate_limit\"}\n";
        std::fs::write(&path, format!("{error}{{\"type\":")).unwrap();
        assert_eq!(
            transcript_failure(&path).unwrap().unwrap().scope,
            FailureScope::Account
        );
        std::fs::write(
            &path,
            format!("{error}{{\"type\":\"assistant\",\"message\":{{\"content\":\"ready\"}}}}\n"),
        )
        .unwrap();
        assert!(transcript_failure(&path).unwrap().is_none());
    }
}
