//! Antigravity (`agy`).

use super::Provider;
use crate::agent_args::{AUTO, Launch, OWN_COMMANDS, push_option};
use crate::roles::{Fallback, Role};

/// The floor's model on Antigravity: any Gemini Flash.
pub const FLOOR_MODEL: &str = "gemini-3.8-flash-medium";

pub struct Antigravity;

impl Provider for Antigravity {
    fn name(&self) -> &'static str {
        "antigravity"
    }

    /// Its permission mode is a role's `approvals`, else accept-edits for a
    /// role that writes and agy's own (it asks) for the rest. Its session's
    /// `--settings` allow what Claude's `--allowedTools` does: the agent's own
    /// commands, and the leader's tools. agy takes no MCP server on its
    /// command line: devws's plugin serves the leader's from the file named
    /// by `--mcp-config`, which devws takes off the command line, and devws
    /// adds its status line to these settings (its module,
    /// agents/40-antigravity). The plugin's server is named after it:
    /// devws_devagentics.
    fn arguments(&self, context: &Launch) -> Vec<String> {
        let role = context.role;
        let mut arguments = vec!["-i".to_string(), context.first_message.to_string()];
        push_option(&mut arguments, "--model", role.model.as_deref());
        push_option(&mut arguments, "--effort", role.effort.as_deref());
        push_option(
            &mut arguments,
            "--mode",
            role.approvals
                .as_deref()
                .or(role.writes.then_some("accept-edits")),
        );
        for dir in &context.writable {
            push_option(&mut arguments, "--add-dir", Some(&dir.to_string_lossy()));
        }
        let mut allowed: Vec<String> = OWN_COMMANDS
            .iter()
            .map(|command| format!("command(devagentics {command})"))
            .collect();
        if context.mcp.is_some() {
            allowed.push("mcp(devws_devagentics/*)".to_string());
        }
        let settings = serde_json::json!({ "permissions": { "allow": allowed } });
        push_option(&mut arguments, "--settings", Some(&settings.to_string()));
        if let Some(mcp) = context.mcp {
            push_option(
                &mut arguments,
                "--mcp-config",
                Some(&mcp.config_file.to_string_lossy()),
            );
        }
        arguments
    }

    /// `agy models` lists what the account has (devws agent models).
    fn models(&self) -> &'static [&'static str] {
        &[
            "gemini-4-carbon-high",
            "gemini-4-carbon-medium",
            "gemini-4-argon-high",
            "gemini-4-argon-medium",
            "gemini-4-hydrogen-high",
            "gemini-3.8-flash-high",
            FLOOR_MODEL,
        ]
    }

    fn efforts(&self) -> &'static [&'static str] {
        &["low", "medium", "high", "xhigh", "max"]
    }

    /// agy's --mode; without one it asks (request-review).
    fn approval_modes(&self) -> &'static [&'static str] {
        &["accept-edits", AUTO, "plan"]
    }

    fn floor(&self) -> Option<(&'static str, &'static str)> {
        Some(("flash", FLOOR_MODEL))
    }

    fn default_fallback(&self) -> Fallback {
        super::codex::sol_fallback()
    }

    fn default_approvals(&self, role: &Role) -> &'static str {
        if role.writes {
            "accept-edits"
        } else {
            "request-review"
        }
    }

    fn lists_models(&self) -> bool {
        true
    }
}
