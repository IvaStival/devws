//! Claude Code.

use super::{Provider, ReviewedAdapter};
use crate::agent_args::{AUTO, Launch, OWN_COMMANDS, push_option};
use crate::roles::{Fallback, Role};

/// The floor's model on Claude: any Sonnet.
pub const FLOOR_MODEL: &str = "sonnet";

pub struct Claude;

/// Its permission mode for a role: its `approvals`, else auto for the leader
/// (it runs the team and its risky calls already wait for your approval
/// through its own tools), acceptEdits for a role that writes, and default
/// for the rest.
pub fn permission_mode(role: &Role) -> &str {
    role.approvals.as_deref().unwrap_or(default_mode(role))
}

fn default_mode(role: &Role) -> &'static str {
    if role.name == "leader" {
        AUTO
    } else if role.writes {
        "acceptEdits"
    } else {
        "default"
    }
}

impl Provider for Claude {
    fn name(&self) -> &'static str {
        "claude"
    }

    /// The first message comes first: `--allowedTools` and `--mcp-config`
    /// take any number of values, and would swallow it after them. The
    /// user's own MCP servers (ai-memory) stay: no `--strict-mcp-config`.
    fn arguments(&self, context: &Launch) -> Vec<String> {
        let role = context.role;
        let mut arguments = vec![context.first_message.to_string()];
        push_option(&mut arguments, "--model", role.model.as_deref());
        push_option(&mut arguments, "--effort", role.effort.as_deref());
        push_option(
            &mut arguments,
            "--permission-mode",
            Some(permission_mode(role)),
        );

        // What every agent does by itself; the leader's tools too.
        arguments.push("--allowedTools".to_string());
        for command in OWN_COMMANDS {
            arguments.push(format!("Bash(devagentics {command}:*)"));
        }
        if context.mcp.is_some() {
            arguments.push("mcp__devagentics".to_string());
        }
        if let Some(mcp) = context.mcp {
            push_option(
                &mut arguments,
                "--mcp-config",
                Some(&mcp.config_file.to_string_lossy()),
            );
        }
        arguments
    }

    fn models(&self) -> &'static [&'static str] {
        &[
            "claude-opus-5-5",
            "claude-sonnet-5-5",
            "claude-haiku-5-5",
            "claude-fable-5-1",
        ]
    }

    fn efforts(&self) -> &'static [&'static str] {
        &["low", "medium", "high", "xhigh", "max"]
    }

    fn approval_modes(&self) -> &'static [&'static str] {
        &["default", "acceptEdits", "plan", AUTO, "bypassPermissions"]
    }

    fn floor(&self) -> Option<(&'static str, &'static str)> {
        Some(("sonnet", FLOOR_MODEL))
    }

    fn default_fallback(&self) -> Fallback {
        super::codex::sol_fallback()
    }

    fn default_approvals(&self, role: &Role) -> &'static str {
        default_mode(role)
    }

    fn reviewed_worker(&self) -> Option<ReviewedAdapter> {
        Some(ReviewedAdapter::Claude)
    }

    fn reviewed_leader(&self) -> Option<ReviewedAdapter> {
        Some(ReviewedAdapter::Claude)
    }
}
