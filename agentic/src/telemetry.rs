//! Opt-in task-local observations. Payload bodies and provider transcripts stay
//! out of the ledger so benchmark exports can contain just the measurements.
use crate::task::Task;
use anyhow::Result;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::time::Instant;

const FLAG: &str = "benchmark-telemetry";
const LEDGER: &str = "benchmark-telemetry.jsonl";

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Summary {
    pub mcp: BTreeMap<String, BTreeMap<String, Payloads>>,
    pub worker_turns_started: u64,
    pub worker_turns_finished: u64,
    pub worker_turns_with_stream_usage: u64,
    pub worker_prompt_bytes: u64,
    pub unreadable_records: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Payloads {
    pub calls: usize,
    pub failures: usize,
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub response_p50_bytes: u64,
    pub response_p95_bytes: u64,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Observation {
    Mcp {
        agent: String,
        operation: String,
        request_bytes: u64,
        response_bytes: u64,
        failed: bool,
    },
    WorkerTurnStarted {
        prompt_bytes: u64,
    },
    WorkerTurnFinished {
        stream_usage: Option<Value>,
    },
}

pub fn summary(task: &Task) -> Result<Option<Summary>> {
    let file = match std::fs::File::open(task.dir().join(LEDGER)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    FileExt::lock_shared(&file)?;
    let mut summary = Summary::default();
    let mut samples = BTreeMap::<_, Vec<u64>>::new();
    for line in BufReader::new(file).lines() {
        match serde_json::from_str::<Observation>(&line?) {
            Ok(Observation::Mcp {
                agent,
                operation,
                request_bytes,
                response_bytes,
                failed,
            }) => {
                let role = if agent == crate::state::LEADER {
                    "leader"
                } else {
                    "worker"
                };
                let payloads = summary
                    .mcp
                    .entry(role.into())
                    .or_default()
                    .entry(operation.clone())
                    .or_default();
                payloads.calls += 1;
                payloads.failures += usize::from(failed);
                payloads.request_bytes += request_bytes;
                payloads.response_bytes += response_bytes;
                samples
                    .entry((role, operation))
                    .or_default()
                    .push(response_bytes);
            }
            Ok(Observation::WorkerTurnStarted { prompt_bytes }) => {
                summary.worker_turns_started += 1;
                summary.worker_prompt_bytes += prompt_bytes;
            }
            Ok(Observation::WorkerTurnFinished { stream_usage }) => {
                summary.worker_turns_finished += 1;
                summary.worker_turns_with_stream_usage += u64::from(stream_usage.is_some());
            }
            Err(_) => summary.unreadable_records += 1,
        }
    }
    for ((role, operation), mut sizes) in samples {
        sizes.sort_unstable();
        let payloads = summary
            .mcp
            .get_mut(role)
            .unwrap()
            .get_mut(&operation)
            .unwrap();
        payloads.response_p50_bytes = sizes[(sizes.len() * 50).div_ceil(100) - 1];
        payloads.response_p95_bytes = sizes[(sizes.len() * 95).div_ceil(100) - 1];
    }
    Ok(Some(summary))
}

pub fn start(task: &Task) -> Option<Instant> {
    task.dir().join(FLAG).is_file().then(Instant::now)
}

pub fn record(task: &Task, event: &impl Serialize) {
    let write = || -> Result<()> {
        let mut bytes = serde_json::to_vec(event)?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(task.dir().join(LEDGER))?;
        FileExt::lock_exclusive(&file)?;
        file.write_all(&bytes)?;
        Ok(())
    };
    if let Err(error) = write() {
        eprintln!("devagentics benchmark telemetry: {error}");
    }
}

pub fn mcp(
    task: &Task,
    agent: &str,
    operation: &str,
    request_bytes: usize,
    wire: &str,
    response: &Value,
    started: Instant,
) {
    let result = &response["result"];
    let content_bytes = result["content"][0]["text"].as_str().map(str::len);
    let catalog = result["tools"].as_array();
    record(
        task,
        &json!({
            "version": 1,
            "kind": "mcp",
            "at": crate::log::now(),
            "agent": agent,
            "operation": operation,
            "request_bytes": request_bytes,
            "response_bytes": wire.len(),
            "content_bytes": content_bytes,
            "tool_count": catalog.map(Vec::len),
            "catalog_bytes": catalog.map(|tools| Value::Array(tools.clone()).to_string().len()),
            "failed": response.get("error").is_some() || result["isError"] == true,
            "duration_ms": started.elapsed().as_millis(),
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_ledger_is_opt_in_and_contains_sizes_without_payloads() {
        let directory = tempfile::tempdir().unwrap();
        let home = crate::home::Home::at(directory.path().to_path_buf());
        let task_dir = home.task_dir("fixture");
        std::fs::create_dir_all(&task_dir).unwrap();
        std::fs::write(task_dir.join("task.toml"), "").unwrap();
        let task = Task::open(&home, "fixture").unwrap();
        assert!(start(&task).is_none());
        std::fs::write(task_dir.join(FLAG), "").unwrap();
        let response = json!({"result":{"content":[{"text":"private é"}],"isError":false}});
        let wire = response.to_string();
        mcp(
            &task,
            "leader",
            "task_status",
            80,
            &wire,
            &response,
            start(&task).unwrap(),
        );
        let ledger = std::fs::read_to_string(task_dir.join(LEDGER)).unwrap();
        assert!(!ledger.contains("private"));
        let event: Value = serde_json::from_str(&ledger).unwrap();
        assert_eq!(event["content_bytes"], 10);
        assert_eq!(event["response_bytes"], wire.len());
        assert_eq!(event["request_bytes"], 80);
        assert_eq!(event["failed"], false);
        let summary = summary(&task).unwrap().unwrap();
        assert_eq!(
            summary.mcp["leader"]["task_status"].response_p95_bytes,
            wire.len() as u64
        );
    }

    #[test]
    fn provider_counters_preserve_cache_breakdown_and_unknowns() {
        use crate::providers::ReviewedAdapter;

        let directory = tempfile::tempdir().unwrap();
        let raw = directory.path().join("turns.jsonl");
        let thread = r#"{"type":"thread.started","thread_id":"thread-1"}"#;
        let previous = r#"{"type":"turn.completed","usage":{"input_tokens":10,"cached_input_tokens":4,"output_tokens":1}}"#;
        let codex = r#"{"type":"turn.completed","usage":{"input_tokens":30,"cached_input_tokens":16,"output_tokens":5}}"#;
        let prefix = format!("{thread}\n{previous}\n");
        std::fs::write(&raw, format!("{prefix}{thread}\n{codex}\n")).unwrap();
        let usage = worker_usage(
            &raw,
            prefix.len() as u64,
            ReviewedAdapter::Codex,
            Some("thread-1"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(usage.uncached_input, Some(8));
        assert_eq!(usage.cache_read, Some(12));
        assert_eq!(usage.cache_write, None);
        assert_eq!(usage.output, Some(4));
        assert!(
            worker_usage(
                &raw,
                prefix.len() as u64,
                ReviewedAdapter::Codex,
                Some("foreign")
            )
            .unwrap()
            .is_none()
        );
        std::fs::write(raw.with_file_name("turns.old.jsonl"), &prefix).unwrap();
        std::fs::write(&raw, format!("{thread}\n{codex}\n")).unwrap();
        assert_eq!(
            worker_usage(&raw, 0, ReviewedAdapter::Codex, Some("thread-1")).unwrap(),
            Some(usage)
        );
        assert!(
            worker_usage(&raw, 0, ReviewedAdapter::Codex, Some("missing"))
                .unwrap()
                .is_none()
        );

        let claude = r#"{"type":"result","usage":{"input_tokens":2,"cache_creation_input_tokens":25,"cache_read_input_tokens":50}}"#;
        std::fs::write(&raw, claude).unwrap();
        let usage = worker_usage(&raw, 0, ReviewedAdapter::Claude, None)
            .unwrap()
            .unwrap();
        assert_eq!(usage.uncached_input, Some(2));
        assert_eq!(usage.cache_write, Some(25));
        assert_eq!(usage.output, None);

        let first = r#"{"type":"step_finish","part":{"id":"part-1","tokens":{"input":1,"output":2,"reasoning":3,"cache":{"read":4,"write":5}}}}"#;
        let second = r#"{"type":"step_finish","part":{"id":"part-2","tokens":{"input":10,"output":20,"cache":{"read":40,"write":50}}}}"#;
        std::fs::write(&raw, format!("{first}\n{first}\n{second}\n")).unwrap();
        let usage = worker_usage(&raw, 0, ReviewedAdapter::Opencode, None)
            .unwrap()
            .unwrap();
        assert_eq!(usage.uncached_input, Some(11));
        assert_eq!(usage.output, Some(22));
        assert_eq!(usage.reasoning, None);
        assert_eq!(usage.cache_read, Some(44));
        assert_eq!(usage.cache_write, Some(55));
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct TokenUsage {
    pub uncached_input: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    pub output: Option<u64>,
    pub reasoning: Option<u64>,
}

impl TokenUsage {
    fn from_event(provider: crate::providers::ReviewedAdapter, usage: &Value) -> Self {
        use crate::providers::ReviewedAdapter;
        let read = |field: &str| usage[field].as_u64();
        match provider {
            ReviewedAdapter::Claude => Self {
                uncached_input: read("input_tokens"),
                cache_read: read("cache_read_input_tokens"),
                cache_write: read("cache_creation_input_tokens"),
                output: read("output_tokens"),
                reasoning: None,
            },
            ReviewedAdapter::Codex => Self {
                uncached_input: read("input_tokens")
                    .zip(read("cached_input_tokens"))
                    .and_then(|(input, cached)| input.checked_sub(cached))
                    .and_then(|input| {
                        input.checked_sub(read("cache_write_input_tokens").unwrap_or(0))
                    }),
                cache_read: read("cached_input_tokens"),
                cache_write: read("cache_write_input_tokens"),
                output: read("output_tokens"),
                reasoning: None,
            },
            ReviewedAdapter::Opencode => Self {
                uncached_input: read("input"),
                cache_read: usage["cache"]["read"].as_u64(),
                cache_write: usage["cache"]["write"].as_u64(),
                output: read("output"),
                reasoning: read("reasoning"),
            },
        }
    }

    fn add(self, other: Self) -> Self {
        let sum = |left: Option<u64>, right: Option<u64>| {
            left.zip(right)
                .and_then(|(left, right)| left.checked_add(right))
        };
        Self {
            uncached_input: sum(self.uncached_input, other.uncached_input),
            cache_read: sum(self.cache_read, other.cache_read),
            cache_write: sum(self.cache_write, other.cache_write),
            output: sum(self.output, other.output),
            reasoning: sum(self.reasoning, other.reasoning),
        }
    }

    fn since(self, previous: Self) -> Self {
        let diff = |current: Option<u64>, before: Option<u64>| {
            current
                .zip(before)
                .and_then(|(current, before)| current.checked_sub(before))
        };
        Self {
            uncached_input: diff(self.uncached_input, previous.uncached_input),
            cache_read: diff(self.cache_read, previous.cache_read),
            cache_write: diff(self.cache_write, previous.cache_write),
            output: diff(self.output, previous.output),
            reasoning: diff(self.reasoning, previous.reasoning),
        }
    }
}

// A successful resumed Codex process reports lifetime thread usage. Keep the
// last attempt even when it has no total, so recovery leaves an unknown delta.
fn codex_total(
    reader: impl BufRead,
    expected: Option<&str>,
) -> Result<Option<(String, Option<TokenUsage>)>> {
    let mut last = None;
    let mut matches_thread = false;
    for line in reader.lines() {
        let Ok(event) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        match event["type"].as_str() {
            Some("thread.started") => {
                matches_thread = event["thread_id"].as_str().is_some_and(|thread| {
                    if expected.is_none_or(|expected| expected == thread) {
                        last = Some((thread.to_owned(), None));
                        true
                    } else {
                        false
                    }
                });
            }
            Some("turn.completed") if matches_thread && event["usage"].is_object() => {
                if let Some((_, usage)) = &mut last {
                    *usage = Some(TokenUsage::from_event(
                        crate::providers::ReviewedAdapter::Codex,
                        &event["usage"],
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(last)
}

fn codex_turn_usage(
    raw: &std::path::Path,
    offset: u64,
    resumed_thread: Option<&str>,
    current: impl BufRead,
) -> Result<Option<TokenUsage>> {
    use std::io::Read;
    let Some((thread, Some(current))) = codex_total(current, resumed_thread)? else {
        return Ok(None);
    };
    if resumed_thread.is_none() {
        return Ok(Some(current));
    }
    let previous = codex_total(
        BufReader::new(std::fs::File::open(raw)?.take(offset)),
        Some(&thread),
    )?;
    let previous = match previous {
        Some(found) => Some(found),
        None => match std::fs::File::open(raw.with_file_name("turns.old.jsonl")) {
            Ok(file) => codex_total(BufReader::new(file), Some(&thread))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        },
    };
    Ok(previous
        .and_then(|(_, usage)| usage)
        .map(|usage| current.since(usage)))
}

pub fn worker_usage(
    raw: &std::path::Path,
    offset: u64,
    provider: crate::providers::ReviewedAdapter,
    resumed_codex_thread: Option<&str>,
) -> Result<Option<TokenUsage>> {
    use crate::providers::ReviewedAdapter;
    use std::io::{Seek, SeekFrom};

    let mut file = std::fs::File::open(raw)?;
    file.seek(SeekFrom::Start(offset))?;
    if provider == ReviewedAdapter::Codex {
        return codex_turn_usage(raw, offset, resumed_codex_thread, BufReader::new(file));
    }
    let mut total: Option<TokenUsage> = None;
    let mut seen = std::collections::HashSet::new();
    for line in BufReader::new(file).lines() {
        let Ok(event) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        match (provider, event["type"].as_str()) {
            (ReviewedAdapter::Claude, Some("result")) if event["usage"].is_object() => {
                total = Some(TokenUsage::from_event(provider, &event["usage"]));
            }
            (ReviewedAdapter::Opencode, Some("step_finish")) => {
                let part = &event["part"];
                if let Some(id) = part["id"].as_str()
                    && part["tokens"].is_object()
                    && seen.insert(id.to_owned())
                {
                    let usage = TokenUsage::from_event(provider, &part["tokens"]);
                    total = Some(match total {
                        Some(previous) => previous.add(usage),
                        None => usage,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(total)
}
