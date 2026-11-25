//! opencode: a cheap model for the floor and seniors, switched on by the user
//! (crate::opencode, opencode.toml). It leads reviewed tasks only, in the
//! isolated configuration of crate::opencode_worker.

use super::{Provider, ReviewedAdapter};
use crate::agent_args::{Launch, push_option};
use crate::roles::{Fallback, Role};

pub struct Opencode;

impl Provider for Opencode {
    fn name(&self) -> &'static str {
        "opencode"
    }

    /// The agents devws defines for it (agent_runner.sh): one that edits, for
    /// the floor, and one without edit tools, for seniors. Its TUI takes no
    /// reasoning-effort flag, so a role's effort does not apply. It gets no
    /// MCP server.
    fn arguments(&self, context: &Launch) -> Vec<String> {
        let role = context.role;
        let mut arguments = vec!["--prompt".to_string(), context.first_message.to_string()];
        push_option(&mut arguments, "--model", role.model.as_deref());
        let agent = if role.writes {
            "devws-floor"
        } else {
            "devws-senior"
        };
        push_option(&mut arguments, "--agent", Some(agent));
        arguments
    }

    /// Only the ones checked against Zen's catalog; `devagentics opencode
    /// models` lists what the account has.
    fn models(&self) -> &'static [&'static str] {
        &["opencode/deepseek-v4.1-flash-free"]
    }

    /// Its TUI takes no effort.
    fn efforts(&self) -> &'static [&'static str] {
        &[]
    }

    /// devws's opencode agents decide what it may do (agent_runner.sh).
    fn approval_modes(&self) -> &'static [&'static str] {
        &[]
    }

    /// It is there to try cheap models: the one the user picked stays.
    fn floor(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    fn default_fallback(&self) -> Fallback {
        super::codex::sol_fallback()
    }

    fn default_approvals(&self, _role: &Role) -> &'static str {
        "provider-default"
    }

    fn can_lead(&self) -> bool {
        false
    }

    fn lists_models(&self) -> bool {
        true
    }

    fn provider_models(&self) -> bool {
        true
    }

    fn own_switches(&self) -> bool {
        true
    }

    fn prepaid(&self) -> bool {
        true
    }

    fn reviewed_worker(&self) -> Option<ReviewedAdapter> {
        Some(ReviewedAdapter::Opencode)
    }

    /// Never on its devws TUI agents, which may run any command: the leader
    /// gets only its MCP tools and, when reads are on, its read command.
    fn reviewed_leader(&self) -> Option<ReviewedAdapter> {
        Some(ReviewedAdapter::Opencode)
    }
}
