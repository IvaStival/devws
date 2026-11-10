use crate::roles::Role;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// The program an agent runs. devws names agents (`claude_work`); this is what
/// they run, which decides the flags they take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Claude,
    Codex,
}

impl Kind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(Kind::Claude),
            "codex" => Some(Kind::Codex),
            _ => None,
        }
    }
}

/// The MCP server the leader's tools come from: `<command> mcp <task id>`.
pub struct McpServer {
    pub command: String,
    pub task_id: String,
    /// For Claude, the JSON file that names the server.
    pub config_file: PathBuf,
}

/// A role's `approvals` that lets the agent work without asking you: Claude's
/// auto permission mode, Codex's automatic review.
pub const AUTO: &str = "auto";

pub struct Launch<'a> {
    pub role: &'a Role,
    pub kind: Kind,
    /// What the agent is told when it starts.
    pub first_message: &'a str,
    /// Only the leader has one.
    pub mcp: Option<&'a McpServer>,
    /// Folders outside its worktree a Codex agent writes in: the devAgentics
    /// home, for its own `devagentics` commands (report, ask, plan write), and
    /// the repo's git data, for its commits. The leader, which uses its MCP
    /// server instead, has none.
    pub writable: Vec<PathBuf>,
}

/// The arguments for the agent's program, after devws's own (`devws agent
/// spawn --agent-args`). The first message comes first: Claude's `--allowedTools`
/// and `--mcp-config` take any number of values, and would swallow it after
/// them. The user's own MCP servers (ai-memory) stay: no `--strict-mcp-config`.
pub fn arguments(context: &Launch) -> Vec<String> {
    match context.kind {
        Kind::Claude => claude(context),
        Kind::Codex => codex(context),
    }
}

fn push_option(arguments: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        arguments.push(flag.to_string());
        arguments.push(value.to_string());
    }
}

fn claude(context: &Launch) -> Vec<String> {
    let role = context.role;
    let mut arguments = vec![context.first_message.to_string()];
    push_option(&mut arguments, "--model", role.model.as_deref());
    push_option(&mut arguments, "--effort", role.effort.as_deref());
    let permission_mode = role.approvals.as_deref().unwrap_or(if role.writes {
        "acceptEdits"
    } else {
        "default"
    });
    push_option(&mut arguments, "--permission-mode", Some(permission_mode));

    // What every agent does by itself; the leader's tools too.
    arguments.push("--allowedTools".to_string());
    for command in ["report", "plan write", "plan show", "ask", "answer"] {
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

fn codex(context: &Launch) -> Vec<String> {
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

/// Codex's `-c` values are TOML: a string is quoted.
fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The file `devws --agent-args` reads: the arguments, each followed by a NUL,
/// so one can hold line breaks.
pub fn write_args_file(path: &Path, arguments: &[String]) -> Result<()> {
    let mut bytes = Vec::new();
    for argument in arguments {
        bytes.extend_from_slice(argument.as_bytes());
        bytes.push(0);
    }
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// The JSON file Claude reads to find the leader's MCP server.
pub fn write_mcp_config(mcp: &McpServer) -> Result<()> {
    let config = serde_json::json!({
        "mcpServers": {
            "devagentics": { "command": mcp.command, "args": ["mcp", mcp.task_id] }
        }
    });
    fs::write(&mcp.config_file, serde_json::to_string_pretty(&config)?)
        .with_context(|| format!("writing {}", mcp.config_file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::Source;
    use tempfile::TempDir;

    fn role(
        name: &str,
        agent: &str,
        model: Option<&str>,
        effort: Option<&str>,
        writes: bool,
    ) -> Role {
        Role {
            name: name.to_string(),
            agent: agent.to_string(),
            model: model.map(str::to_string),
            effort: effort.map(str::to_string),
            writes,
            group: if writes {
                crate::roles::Group::Floor
            } else {
                crate::roles::Group::Senior
            },
            sandbox: None,
            approvals: None,
            description: None,
            instructions: "Do it.".to_string(),
            source: Source::Builtin,
            fallback: None,
        }
    }

    fn mcp() -> McpServer {
        McpServer {
            command: "/bin/devagentics".to_string(),
            task_id: "feat-login".to_string(),
            config_file: "/tasks/feat-login/mcp.json".into(),
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn a_claude_executor_edits_and_reports() {
        let role = role("executor", "claude", Some("sonnet"), Some("medium"), true);
        let arguments = arguments(&Launch {
            role: &role,
            kind: Kind::Claude,
            first_message: "Hello\nthere",
            mcp: None,
            writable: Vec::new(),
        });
        assert_eq!(
            arguments,
            strings(&[
                "Hello\nthere",
                "--model",
                "sonnet",
                "--effort",
                "medium",
                "--permission-mode",
                "acceptEdits",
                "--allowedTools",
                "Bash(devagentics report:*)",
                "Bash(devagentics plan write:*)",
                "Bash(devagentics plan show:*)",
                "Bash(devagentics ask:*)",
                "Bash(devagentics answer:*)",
            ])
        );
    }

    #[test]
    fn a_claude_leader_gets_its_tools_and_keeps_the_users_servers() {
        let role = role("leader", "claude", None, Some("high"), false);
        let server = mcp();
        let arguments = arguments(&Launch {
            role: &role,
            kind: Kind::Claude,
            first_message: "Lead",
            mcp: Some(&server),
            writable: Vec::new(),
        });
        assert_eq!(
            arguments,
            strings(&[
                "Lead",
                "--effort",
                "high",
                "--permission-mode",
                "default",
                "--allowedTools",
                "Bash(devagentics report:*)",
                "Bash(devagentics plan write:*)",
                "Bash(devagentics plan show:*)",
                "Bash(devagentics ask:*)",
                "Bash(devagentics answer:*)",
                "mcp__devagentics",
                "--mcp-config",
                "/tasks/feat-login/mcp.json",
            ])
        );
        assert!(!arguments.contains(&"--strict-mcp-config".to_string()));
    }

    #[test]
    fn the_role_can_set_the_permission_mode() {
        let mut role = role("tester", "claude", None, None, true);
        role.approvals = Some("plan".to_string());
        let arguments = arguments(&Launch {
            role: &role,
            kind: Kind::Claude,
            first_message: "x",
            mcp: None,
            writable: Vec::new(),
        });
        let at = arguments
            .iter()
            .position(|argument| argument == "--permission-mode")
            .unwrap();
        assert_eq!(arguments[at + 1], "plan");
    }

    #[test]
    fn a_codex_executor_writes_in_its_sandbox() {
        let role = role("executor", "codex", Some("gpt-5"), Some("high"), true);
        let arguments = arguments(&Launch {
            role: &role,
            kind: Kind::Codex,
            first_message: "Go",
            mcp: None,
            writable: Vec::new(),
        });
        assert_eq!(
            arguments,
            strings(&[
                "-c",
                "tui.animations=false",
                "-m",
                "gpt-5",
                "-c",
                "model_reasoning_effort=\"high\"",
                "-s",
                "workspace-write",
                "-a",
                "on-request",
                "Go"
            ])
        );
    }

    #[test]
    fn a_codex_agent_may_write_the_tasks_state_and_a_senior_gets_the_sandbox_for_it() {
        for writes in [true, false] {
            let role = role("x", "codex", None, None, writes);
            let arguments = arguments(&Launch {
                role: &role,
                kind: Kind::Codex,
                first_message: "Go",
                mcp: None,
                writable: vec![PathBuf::from("/home/.devagentics")],
            });
            assert_eq!(
                arguments,
                strings(&[
                    "-c",
                    "tui.animations=false",
                    "-s",
                    "workspace-write",
                    "--add-dir",
                    "/home/.devagentics",
                    "-a",
                    "on-request",
                    "Go"
                ]),
                "writes = {writes}"
            );
        }
    }

    #[test]
    fn auto_lets_codex_review_itself_and_claude_run_in_auto_mode() {
        let mut role = role("hammer", "codex", Some("gpt-5.6-luna"), None, true);
        role.approvals = Some("auto".to_string());
        let arguments = arguments(&Launch {
            role: &role,
            kind: Kind::Codex,
            first_message: "Go",
            mcp: None,
            writable: vec![
                PathBuf::from("/home/.devagentics"),
                PathBuf::from("/repo/.git"),
            ],
        });
        assert_eq!(
            arguments,
            strings(&[
                "-c",
                "tui.animations=false",
                "-m",
                "gpt-5.6-luna",
                "--approve-for-me",
                "--add-dir",
                "/home/.devagentics",
                "--add-dir",
                "/repo/.git",
                "Go"
            ])
        );
        let mut role = role_for_claude();
        role.approvals = Some("auto".to_string());
        let claude_arguments = super::arguments(&Launch {
            role: &role,
            kind: Kind::Claude,
            first_message: "Go",
            mcp: None,
            writable: Vec::new(),
        });
        let at = claude_arguments
            .iter()
            .position(|argument| argument == "--permission-mode")
            .unwrap();
        assert_eq!(claude_arguments[at + 1], "auto");
    }

    fn role_for_claude() -> Role {
        role("hammer", "claude", None, None, true)
    }

    #[test]
    fn a_codex_leader_reads_only_and_names_its_server() {
        let role = role("leader", "codex", None, None, false);
        let server = mcp();
        let arguments = arguments(&Launch {
            role: &role,
            kind: Kind::Codex,
            first_message: "Lead",
            mcp: Some(&server),
            writable: Vec::new(),
        });
        assert_eq!(
            arguments,
            strings(&[
                "-c",
                "tui.animations=false",
                "-s",
                "read-only",
                "-a",
                "on-request",
                "-c",
                "mcp_servers.devagentics.command=\"/bin/devagentics\"",
                "-c",
                "mcp_servers.devagentics.args=[\"mcp\", \"feat-login\"]",
                "Lead",
            ])
        );
    }

    #[test]
    fn codex_values_are_quoted_for_toml() {
        assert_eq!(toml_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn the_args_file_keeps_each_argument_whole() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("agent.args");
        write_args_file(&file, &strings(&["two words\nand a line", "--flag"])).unwrap();
        let bytes = fs::read(&file).unwrap();
        let read: Vec<_> = bytes.split(|byte| *byte == 0).collect();
        assert_eq!(
            read,
            [&b"two words\nand a line"[..], &b"--flag"[..], &b""[..]]
        );
    }

    #[test]
    fn the_mcp_config_names_the_server_for_claude() {
        let dir = TempDir::new().unwrap();
        let server = McpServer {
            config_file: dir.path().join("mcp.json"),
            ..mcp()
        };
        write_mcp_config(&server).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&server.config_file).unwrap()).unwrap();
        assert_eq!(
            config["mcpServers"]["devagentics"]["command"],
            "/bin/devagentics"
        );
        assert_eq!(
            config["mcpServers"]["devagentics"]["args"],
            serde_json::json!(["mcp", "feat-login"])
        );
    }

    #[test]
    fn only_claude_and_codex_are_known() {
        assert_eq!(Kind::parse("claude"), Some(Kind::Claude));
        assert_eq!(Kind::parse("codex"), Some(Kind::Codex));
        assert_eq!(Kind::parse("opencode"), None);
    }
}
