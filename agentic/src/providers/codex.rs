//! Codex.

use super::{Provider, ReviewedAdapter};
use crate::agent_args::{AUTO, Launch, push_option, toml_string};
use crate::roles::{Fallback, Role};

/// The floor's model on Codex: any Luna.
pub const FLOOR_MODEL: &str = "gpt-6-luna";

pub struct Codex;

/// Codex Sol at medium effort, reviewing itself: the other programs' next one.
pub(super) fn sol_fallback() -> Fallback {
    Fallback {
        agent: "codex".into(),
        model: Some("gpt-6.1-sol".into()),
        effort: Some("medium".into()),
        approvals: Some(AUTO.into()),
        sandbox: None,
    }
}

impl Provider for Codex {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn arguments(&self, context: &Launch) -> Vec<String> {
        let role = context.role;
        // A team's agents work in view all day: Codex's shimmering status line
        // would have the terminal repaint many times a second.
        let mut arguments = vec!["-c".to_string(), "tui.animations=false".to_string()];
        push_option(&mut arguments, "-m", role.model.as_deref());
        if let Some(effort) = &role.effort {
            arguments.push("-c".to_string());
            arguments.push(format!("model_reasoning_effort={}", toml_string(effort)));
        }
        // An agent's `devagentics` commands write the task's state, outside the
        // repo, and only the workspace-write sandbox takes another writable
        // folder. So a senior gets it too: that it leaves the repo alone is
        // enforced by its orders (every file forbidden), not by the sandbox.
        // `approvals = "auto"`: Codex's own reviewer answers its approval
        // requests, in the workspace-write sandbox it picks itself (it takes
        // neither -s nor -a).
        let auto = role.approvals.as_deref() == Some(AUTO);
        if auto {
            arguments.push("--approve-for-me".to_string());
        } else {
            let sandbox =
                role.sandbox
                    .as_deref()
                    .unwrap_or(if role.writes || !context.writable.is_empty() {
                        "workspace-write"
                    } else {
                        "read-only"
                    });
            push_option(&mut arguments, "-s", Some(sandbox));
        }
        for dir in &context.writable {
            push_option(&mut arguments, "--add-dir", Some(&dir.to_string_lossy()));
        }
        if !auto {
            push_option(
                &mut arguments,
                "-a",
                Some(role.approvals.as_deref().unwrap_or("on-request")),
            );
        }
        if let Some(mcp) = context.mcp {
            arguments.push("-c".to_string());
            arguments.push(format!(
                "mcp_servers.devagentics.command={}",
                toml_string(&mcp.command)
            ));
            arguments.push("-c".to_string());
            arguments.push(format!(
                "mcp_servers.devagentics.args=[{}, {}]",
                toml_string("mcp"),
                toml_string(&mcp.task_id)
            ));
        }
        arguments.push(context.first_message.to_string());
        arguments
    }

    fn models(&self) -> &'static [&'static str] {
        &["gpt-6.1-sol", "gpt-6-luna", "gpt-5.6-luna"]
    }

    fn efforts(&self) -> &'static [&'static str] {
        &["minimal", "low", "medium", "high", "xhigh"]
    }

    /// Its -a, and devws's auto review.
    fn approval_modes(&self) -> &'static [&'static str] {
        &["untrusted", "on-request", "never", AUTO]
    }

    fn floor(&self) -> Option<(&'static str, &'static str)> {
        Some(("luna", FLOOR_MODEL))
    }

    /// Claude at high effort, in auto mode.
    fn default_fallback(&self) -> Fallback {
        Fallback {
            agent: "claude".into(),
            model: None,
            effort: Some("high".into()),
            approvals: Some(AUTO.into()),
            sandbox: None,
        }
    }

    fn default_approvals(&self, _role: &Role) -> &'static str {
        "on-request"
    }

    fn sandbox(&self, role: &Role) -> Option<String> {
        Some(role.sandbox.clone().unwrap_or_else(|| {
            if role.writes {
                "workspace-write"
            } else {
                "read-only"
            }
            .to_string()
        }))
    }

    fn reviewed_worker(&self) -> Option<ReviewedAdapter> {
        Some(ReviewedAdapter::Codex)
    }

    fn reviewed_leader(&self) -> Option<ReviewedAdapter> {
        Some(ReviewedAdapter::Codex)
    }
}
