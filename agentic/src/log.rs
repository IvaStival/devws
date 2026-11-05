use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Start,
    Order,
    Report,
    Check,
    Review,
    Plan,
    Approve,
    Spawn,
    Stop,
    Pause,
    Repo,
    Delivered,
    Lost,
    Escalate,
    Pr,
    Question,
    Answer,
    /// The leader's program: chosen at start, or handed over to its fallback.
    Leader,
}

/// One line of `log.jsonl`: what happened in the team, written by devAgentics
/// only.
#[derive(Debug, Serialize)]
pub struct Entry {
    pub at: u64,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub text: String,
}

impl Entry {
    pub fn new(kind: Kind, agent: Option<&str>, step: Option<&str>, text: &str) -> Self {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        Self {
            at,
            kind,
            agent: agent.map(str::to_string),
            step: step.map(str::to_string),
            text: text.to_string(),
        }
    }
}
