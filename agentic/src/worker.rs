//! Bounded worker startup selection. Retry state survives dispatcher restarts.
use crate::devws::Devws;
use crate::opencode::OpencodeSettings;
use crate::providers::Kind;
use crate::roles::{Fallback, Role};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const LIMIT_PERCENT: u32 = 95;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PercentageBlock {
    pub account: String,
    pub week_percent: Option<u32>,
    pub five_hour_percent: Option<u32>,
}

#[derive(Debug)]
pub struct UsageLimitRequired;
impl std::fmt::Display for UsageLimitRequired {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "usage-limit approval required")
    }
}
impl std::error::Error for UsageLimitRequired {}

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
    #[serde(default)]
    pub percentage_blocks: Vec<PercentageBlock>,
}

impl LaunchState {
    /// The role's candidates, after opencode when the user switched it on
    /// for the role's group.
    pub fn new(role: &Role, kind: Kind, opencode: &OpencodeSettings) -> Self {
        let mut candidates: Vec<Fallback> = opencode.candidate(role.group).into_iter().collect();
        for candidate in role.worker_candidates(kind) {
            if !candidates.iter().any(|existing| {
                existing.agent == candidate.agent && existing.model == candidate.model
            }) {
                candidates.push(candidate);
            }
        }
        Self {
            role: role.clone(),
            candidates,
            next: 0,
            selected: None,
            failures: Vec::new(),
            blocked_accounts: Vec::new(),
            percentage_blocks: Vec::new(),
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

    /// `opencode` is read again at each attempt: switching it off holds for
    /// the next one, a retry included.
    pub fn next_available(
        &mut self,
        devws: &dyn Devws,
        approved: bool,
        disabled: &std::collections::BTreeSet<String>,
        opencode: &OpencodeSettings,
    ) -> Result<Role> {
        self.selected = None;
        while let Some(candidate) = self.candidates.get(self.next).cloned() {
            self.next += 1;
            if disabled.contains(&candidate.agent) {
                self.failures
                    .push(format!("{} is turned off for this task", candidate.agent));
                continue;
            }
            if self.blocked_accounts.contains(&candidate.agent) {
                self.failures
                    .push(format!("{}: account unavailable", program_name(&candidate)));
                continue;
            }
            let kind = match devws.agent_kind(&candidate.agent) {
                Ok(kind) => kind,
                Err(error) => {
                    self.failures
                        .push(format!("{}: {error}", program_name(&candidate)));
                    continue;
                }
            };
            let provider = kind.provider();
            // A program on the user's own switches runs only where they allow.
            if provider.own_switches() && !opencode.allows(self.role.group) {
                self.failures.push(format!(
                    "{}: opencode is switched off for the {} group",
                    program_name(&candidate),
                    self.role.group.name()
                ));
                continue;
            }
            if provider.prepaid() {
                // A prepaid account takes no new agent once it is spent.
                if let Some(credit) = devws.credit(&candidate.agent)?
                    && credit.left() <= 0.0
                {
                    self.failures.push(format!(
                        "{} has no credit left (US$ {:.2} of US$ {:.2} used): no new agents on it until it is topped up",
                        candidate.agent, credit.used, credit.balance
                    ));
                    self.blocked_accounts.push(candidate.agent);
                    continue;
                }
            }
            let limits = devws.limits(&candidate.agent)?;
            // Used up, it cannot run at all: no consent starts an agent there.
            let used_up = [("week", limits.week), ("5-hour limit", limits.five_hour)]
                .into_iter()
                .find(|(_, used)| used.is_some_and(|used| used >= 100));
            if let Some((window, _)) = used_up {
                self.failures.push(format!(
                    "{} has used 100% of its {window}: it cannot run until it resets",
                    candidate.agent
                ));
                continue;
            }
            let exhausted = [("week", limits.week), ("5-hour limit", limits.five_hour)]
                .into_iter()
                .find_map(|(window, used)| {
                    used.filter(|used| *used >= LIMIT_PERCENT)
                        .map(|used| (window, used))
                });
            if let Some((window, used)) = exhausted.filter(|_| !approved) {
                self.failures.push(format!(
                    "{} has used {used}% of its {window}: no new agents on it until it resets",
                    candidate.agent
                ));
                if !self
                    .percentage_blocks
                    .iter()
                    .any(|block| block.account == candidate.agent)
                {
                    self.percentage_blocks.push(PercentageBlock {
                        account: candidate.agent.clone(),
                        week_percent: limits.week,
                        five_hour_percent: limits.five_hour,
                    });
                }
                continue;
            }
            let role = self.role.with_program(&candidate);
            self.selected = Some(candidate);
            return Ok(role);
        }
        // Nothing could start. When an account was held back only by its
        // percentage, the user's consent starts it, whatever kept the others
        // out: their usage too, or the user switching them off.
        if !self.percentage_blocks.is_empty() {
            return Err(anyhow::Error::new(UsageLimitRequired).context(format!(
                "usage-limit approval required: {}. Ask the user whether workers may exceed the percentage threshold for this task; record explicit consent with set_usage_limit_approval, then resume_task",
                self.failures.join("; ")
            )));
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
        "model_not_found"
        | "model_not_available"
        | "model_unavailable"
        | "unsupported_model"
        | "adapter_unavailable" => FailureScope::Model,
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
    use crate::devws::Credit;
    use crate::devws::fake::FakeDevws;
    use crate::opencode::{GroupSettings, OpencodeSettings};
    use crate::roles::{Group, Source};
    use serde_json::json;

    const FLASH: &str = "opencode/deepseek-v4.1-flash";

    fn codex_role(group: Group) -> Role {
        Role {
            name: "executor".into(),
            agent: "codex".into(),
            model: Some("gpt-6-luna".into()),
            effort: None,
            writes: group == Group::Floor,
            group,
            sandbox: None,
            approvals: None,
            description: None,
            instructions: "Do it.".into(),
            source: Source::Builtin,
            fallback: None,
            alternatives: Vec::new(),
            shows_references: false,
        }
    }

    fn opencode_on_for_the_floor() -> OpencodeSettings {
        OpencodeSettings {
            enabled: true,
            floor: GroupSettings {
                enabled: true,
                model: Some(FLASH.into()),
            },
            ..OpencodeSettings::default()
        }
    }

    fn devws() -> FakeDevws {
        FakeDevws::new(std::path::Path::new("/nowhere"))
            .with_kind("codex", Kind::Codex)
            .with_kind("claude", Kind::Claude)
            .with_kind("opencode", Kind::Opencode)
    }

    #[test]
    fn a_floor_agent_tries_opencode_first_when_it_is_switched_on() {
        let settings = opencode_on_for_the_floor();
        let mut launch = LaunchState::new(&codex_role(Group::Floor), Kind::Codex, &settings);
        let role = launch
            .next_available(&devws(), false, &Default::default(), &settings)
            .unwrap();
        assert_eq!(role.agent, "opencode");
        assert_eq!(role.model.as_deref(), Some(FLASH));
        assert_eq!(launch.candidates[1].agent, "codex", "codex stays behind it");
    }

    #[test]
    fn a_senior_stays_on_its_program_while_the_seniors_switch_is_off() {
        let settings = opencode_on_for_the_floor();
        let mut launch = LaunchState::new(&codex_role(Group::Senior), Kind::Codex, &settings);
        assert_eq!(
            launch
                .next_available(&devws(), false, &Default::default(), &settings)
                .unwrap()
                .agent,
            "codex"
        );
    }

    #[test]
    fn the_global_switch_off_keeps_even_a_role_that_names_opencode_off_it() {
        let mut settings = opencode_on_for_the_floor();
        let mut launch = LaunchState::new(&codex_role(Group::Floor), Kind::Codex, &settings);
        settings.enabled = false;
        let role = launch
            .next_available(&devws(), false, &Default::default(), &settings)
            .unwrap();
        assert_eq!(role.agent, "codex");
        assert!(
            launch.failures[0].contains("switched off"),
            "{:?}",
            launch.failures
        );

        let mut pinned = codex_role(Group::Floor);
        pinned.agent = "opencode".into();
        pinned.model = Some(FLASH.into());
        let mut launch = LaunchState::new(&pinned, Kind::Opencode, &settings);
        assert_eq!(
            launch
                .next_available(&devws(), false, &Default::default(), &settings)
                .unwrap()
                .agent,
            "codex"
        );
    }

    #[test]
    fn an_opencode_account_without_credit_left_is_skipped() {
        let settings = opencode_on_for_the_floor();
        let devws = devws().with_credit(
            "opencode",
            Credit {
                balance: 10.0,
                used: 10.0,
            },
        );
        let mut launch = LaunchState::new(&codex_role(Group::Floor), Kind::Codex, &settings);
        assert_eq!(
            launch
                .next_available(&devws, false, &Default::default(), &settings)
                .unwrap()
                .agent,
            "codex"
        );
        assert!(
            launch.failures[0].contains("no credit left"),
            "{:?}",
            launch.failures
        );
        assert!(launch.blocked_accounts.contains(&"opencode".to_string()));
    }

    #[test]
    fn an_opencode_account_with_credit_left_runs() {
        let settings = opencode_on_for_the_floor();
        let devws = devws().with_credit(
            "opencode",
            Credit {
                balance: 10.0,
                used: 2.5,
            },
        );
        let mut launch = LaunchState::new(&codex_role(Group::Floor), Kind::Codex, &settings);
        assert_eq!(
            launch
                .next_available(&devws, false, &Default::default(), &settings)
                .unwrap()
                .agent,
            "opencode"
        );
    }

    #[test]
    fn an_account_the_user_turned_off_is_never_chosen() {
        let role = crate::roles::parse(
            "planner",
            "+++\nagent = \"claude\"\n+++\nPlan.\n",
            crate::roles::Source::Home,
            "test",
        )
        .unwrap();
        let folder = tempfile::TempDir::new().unwrap();
        let devws = crate::devws::fake::FakeDevws::new(folder.path())
            .with_kind("claude", Kind::Claude)
            .with_kind("codex", Kind::Codex);

        let mut launch = LaunchState::new(&role, Kind::Claude, &OpencodeSettings::default());
        let off = std::collections::BTreeSet::from(["claude".to_string()]);
        let chosen = launch
            .next_available(&devws, false, &off, &OpencodeSettings::default())
            .unwrap();
        assert_eq!(chosen.agent, "codex");
        assert!(
            launch
                .failures
                .iter()
                .any(|failure| failure.contains("claude is turned off for this task")),
            "{:?}",
            launch.failures
        );

        let mut launch = LaunchState::new(&role, Kind::Claude, &OpencodeSettings::default());
        let all_off = std::collections::BTreeSet::from(["claude".to_string(), "codex".to_string()]);
        let error = launch
            .next_available(&devws, false, &all_off, &OpencodeSettings::default())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("codex is turned off for this task"),
            "{error}"
        );
    }

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
