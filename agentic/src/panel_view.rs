//! What a managed worker's panel shows: the provider's machine output (Codex
//! `exec --json`, Claude `stream-json`) turned into one event model, drawn
//! for a person: formatted messages, one line per tool call with its outcome,
//! and a header and footer around each turn.
use serde_json::Value;
use std::collections::HashMap;

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

/// How long a tool's input or a failure reason may be on its one line.
const DETAIL_CHARS: usize = 100;

#[derive(Debug, PartialEq)]
pub enum Event {
    /// What the agent says, in markdown.
    Message(String),
    Tool {
        action: String,
        detail: String,
        outcome: Outcome,
    },
    Command {
        line: String,
        exit: Option<i64>,
    },
    Error(String),
    TurnDone(Usage),
    /// A line that is not an event: shown as it is.
    Raw(String),
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Done,
    Failed(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

/// One line of `codex exec --json`.
pub fn codex_events(line: &str) -> Vec<Event> {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return vec![Event::Raw(line.to_string())];
    };
    let item = &event["item"];
    match (event["type"].as_str(), item["type"].as_str()) {
        (Some("item.completed"), Some("agent_message")) => {
            vec![Event::Message(text_of(&item["text"]))]
        }
        (Some("item.completed"), Some("mcp_tool_call")) => {
            let failed = item["status"] == "failed" || item["result"]["isError"] == true;
            vec![Event::Tool {
                action: action_of(&item["tool"], &item["arguments"]),
                detail: detail_of(&item["arguments"]["input"]),
                outcome: if failed {
                    Outcome::Failed(reason(
                        item["error"]["message"].as_str(),
                        &item["result"]["content"],
                    ))
                } else {
                    Outcome::Done
                },
            }]
        }
        (Some("item.completed"), Some("command_execution")) => {
            let command = item["command"].as_str().unwrap_or_default();
            // Codex wraps each command in a login shell; show the command.
            let command = command
                .strip_prefix("/bin/bash -lc '")
                .and_then(|inner| inner.strip_suffix('\''))
                .unwrap_or(command);
            vec![Event::Command {
                line: command.to_string(),
                exit: item["exit_code"].as_i64(),
            }]
        }
        (Some("turn.completed"), _) => {
            let usage = &event["usage"];
            vec![Event::TurnDone(Usage {
                input: usage["input_tokens"].as_u64().unwrap_or(0),
                cached: usage["cached_input_tokens"].as_u64().unwrap_or(0),
                output: usage["output_tokens"].as_u64().unwrap_or(0),
            })]
        }
        (Some("error" | "turn.failed"), _) => vec![Event::Error(
            event["message"]
                .as_str()
                .or(event["error"]["message"].as_str())
                .unwrap_or("Codex reported an error")
                .to_string(),
        )],
        _ => Vec::new(),
    }
}

/// Claude's `stream-json` lines. A tool call arrives as a `tool_use` in the
/// assistant's message and its outcome later as a `tool_result`, so the
/// call is kept until its result comes.
#[derive(Default)]
pub struct ClaudeStream {
    pending: HashMap<String, (String, String)>,
}

impl ClaudeStream {
    pub fn events(&mut self, line: &str) -> Vec<Event> {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            return vec![Event::Raw(line.to_string())];
        };
        match event["type"].as_str() {
            Some("assistant") => self.assistant(&event["message"]["content"]),
            Some("user") => self.results(&event["message"]["content"]),
            Some("result") => {
                let mut events = Vec::new();
                if event["is_error"] == true {
                    events.push(Event::Error(
                        event["result"]
                            .as_str()
                            .unwrap_or("Claude reported an error")
                            .to_string(),
                    ));
                }
                let usage = &event["usage"];
                events.push(Event::TurnDone(Usage {
                    input: usage["input_tokens"].as_u64().unwrap_or(0)
                        + usage["cache_read_input_tokens"].as_u64().unwrap_or(0),
                    cached: usage["cache_read_input_tokens"].as_u64().unwrap_or(0),
                    output: usage["output_tokens"].as_u64().unwrap_or(0),
                }));
                events
            }
            _ => Vec::new(),
        }
    }

    fn assistant(&mut self, content: &Value) -> Vec<Event> {
        let mut events = Vec::new();
        for block in content.as_array().into_iter().flatten() {
            match block["type"].as_str() {
                Some("text") => events.push(Event::Message(text_of(&block["text"]))),
                Some("tool_use") => {
                    let name = block["name"].as_str().unwrap_or("tool");
                    let tool = name.rsplit("__").next().unwrap_or(name);
                    self.pending.insert(
                        block["id"].as_str().unwrap_or_default().to_string(),
                        (
                            action_of(&Value::from(tool), &block["input"]),
                            detail_of(&block["input"]["input"]),
                        ),
                    );
                }
                _ => {}
            }
        }
        events
    }

    fn results(&mut self, content: &Value) -> Vec<Event> {
        let mut events = Vec::new();
        for block in content.as_array().into_iter().flatten() {
            if block["type"] != "tool_result" {
                continue;
            }
            let id = block["tool_use_id"].as_str().unwrap_or_default();
            let (action, detail) = self
                .pending
                .remove(id)
                .unwrap_or_else(|| ("tool".into(), String::new()));
            let outcome = if block["is_error"] == true {
                let content = &block["content"];
                Outcome::Failed(reason(content.as_str(), content))
            } else {
                Outcome::Done
            };
            events.push(Event::Tool {
                action,
                detail,
                outcome,
            });
        }
        events
    }
}

fn text_of(value: &Value) -> String {
    value.as_str().unwrap_or_default().trim_end().to_string()
}

/// `worker checkpoint`: the tool and, for the worker tool, its action.
fn action_of(tool: &Value, arguments: &Value) -> String {
    let tool = tool.as_str().unwrap_or("tool");
    match arguments["action"].as_str() {
        Some(action) if tool == "worker" => action.to_string(),
        Some(action) => format!("{tool} {action}"),
        None => tool.to_string(),
    }
}

/// The one input that says what a call is about.
fn detail_of(input: &Value) -> String {
    for key in [
        "command", "summary", "question", "reason", "symbol", "query", "outcome",
    ] {
        if let Some(text) = input[key].as_str() {
            return shorten(text.lines().next().unwrap_or_default());
        }
    }
    String::new()
}

/// Why a call failed: its error, or the text of what it returned.
fn reason(message: Option<&str>, content: &Value) -> String {
    let returned: Vec<&str> = content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect();
    let text = message
        .filter(|message| !message.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| returned.join(" "));
    if text.trim().is_empty() {
        "no reason given".into()
    } else {
        shorten(
            text.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .as_str(),
        )
    }
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= DETAIL_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(DETAIL_CHARS - 1).collect();
    format!("{kept}…")
}

/// The event as the panel shows it, `width` columns wide.
pub fn render(event: &Event, width: usize) -> Option<String> {
    match event {
        Event::Message(text) if text.is_empty() => None,
        Event::Message(text) => {
            let skin = termimad::MadSkin::default();
            Some(
                skin.text(text, Some(width.max(20)))
                    .to_string()
                    .trim_end()
                    .to_string(),
            )
        }
        Event::Tool {
            action,
            detail,
            outcome,
        } => {
            let detail = if detail.is_empty() {
                String::new()
            } else {
                format!(" {detail:?}")
            };
            Some(match outcome {
                Outcome::Done => format!("{DIM}• {action}{detail}{RESET} {GREEN}✓{RESET}"),
                Outcome::Failed(why) => {
                    format!("{DIM}• {action}{detail}{RESET} {RED}✕ {why}{RESET}")
                }
            })
        }
        Event::Command { line, exit } => Some(match exit {
            Some(0) | None => format!("{DIM}$ {line}{RESET}"),
            Some(code) => format!("{DIM}$ {line}{RESET}  {RED}(exit {code}){RESET}"),
        }),
        Event::Error(text) => Some(format!("{RED}! {text}{RESET}")),
        Event::TurnDone(_) => None,
        Event::Raw(line) => Some(line.clone()),
    }
}

/// What a turn is: who, on what, for which order.
pub struct Turn<'a> {
    pub agent: &'a str,
    pub program: &'a str,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    /// `o52 (B1-1-AT)`, or nothing for a message that is not an order.
    pub order: Option<String>,
}

fn rule(text: &str, width: usize) -> String {
    let used = text.chars().count() + 4;
    let fill = "─".repeat(width.saturating_sub(used).max(2));
    format!("{DIM}── {RESET}{text}{DIM} {fill}{RESET}")
}

/// The line that opens a turn.
pub fn header(turn: &Turn, at: &str, width: usize) -> String {
    let mut parts = vec![
        format!("{BOLD}{}{RESET}", turn.agent),
        turn.program.to_string(),
        turn.model.unwrap_or("default model").to_string(),
    ];
    parts.extend(turn.effort.map(str::to_string));
    parts.extend(turn.order.as_ref().map(|order| format!("order {order}")));
    parts.push(at.to_string());
    rule(&parts.join(" · "), width)
}

/// The line that closes a turn: how long, what it used, or why it failed.
pub fn footer(seconds: u64, usage: Option<Usage>, failure: Option<&str>, width: usize) -> String {
    let took = if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    };
    let text = match failure {
        Some(why) => format!("{RED}failed after {took}: {why}{RESET}"),
        None => match usage {
            Some(usage) => format!(
                "{GREEN}done{RESET} in {took} · {} in ({} cached) · {} out",
                tokens(usage.input),
                tokens(usage.cached),
                tokens(usage.output)
            ),
            None => format!("{GREEN}done{RESET} in {took}"),
        },
    };
    rule(&text, width)
}

/// 1136501 → 1.1M, 8329 → 8.3k.
fn tokens(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..1_000_000 => format!("{:.1}k", count as f64 / 1_000.0),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
}

/// The panel's width: its terminal's, or a sensible one without a terminal.
pub fn width() -> usize {
    termimad::crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns))
        .unwrap_or(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(character) = chars.next() {
            if character == '\x1b' {
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(character);
            }
        }
        out
    }

    #[test]
    fn a_failed_codex_tool_call_shows_the_text_its_server_returned() {
        // As Codex 0.169 sends it: no error message, the reason in the result.
        let line = r#"{"type":"item.completed","item":{"id":"item_9","type":"mcp_tool_call","server":"devagentics_worker","tool":"worker","arguments":{"action":"checkpoint","input":{"summary":"Add B1-1-AT tests"}},"result":{"content":[{"type":"text","text":"checkpoint outside approved source scope: web/.next/types/cache-life.d.ts"}],"structured_content":null},"error":null,"status":"failed"}}"#;
        assert_eq!(
            codex_events(line),
            [Event::Tool {
                action: "checkpoint".into(),
                detail: "Add B1-1-AT tests".into(),
                outcome: Outcome::Failed(
                    "checkpoint outside approved source scope: web/.next/types/cache-life.d.ts"
                        .into()
                ),
            }]
        );
        let shown = plain(&render(&codex_events(line)[0], 120).unwrap());
        assert!(
            shown.contains("• checkpoint \"Add B1-1-AT tests\" ✕ checkpoint outside"),
            "{shown}"
        );
    }

    #[test]
    fn codex_messages_commands_and_usage_become_events() {
        assert_eq!(
            codex_events(
                r#"{"type":"item.completed","item":{"type":"agent_message","text":"**Done**\n"}}"#
            ),
            [Event::Message("**Done**".into())]
        );
        assert_eq!(
            codex_events(
                r#"{"type":"item.completed","item":{"type":"command_execution","command":"/bin/bash -lc 'ls web'","exit_code":2}}"#
            ),
            [Event::Command {
                line: "ls web".into(),
                exit: Some(2)
            }]
        );
        assert_eq!(
            codex_events(
                r#"{"type":"turn.completed","usage":{"input_tokens":1136501,"cached_input_tokens":1069568,"output_tokens":8329}}"#
            ),
            [Event::TurnDone(Usage {
                input: 1136501,
                cached: 1069568,
                output: 8329
            })]
        );
        assert!(codex_events(r#"{"type":"thread.started","thread_id":"t1"}"#).is_empty());
        assert_eq!(
            codex_events("Reading additional input from stdin..."),
            [Event::Raw("Reading additional input from stdin...".into())]
        );
    }

    #[test]
    fn a_claude_tool_call_is_shown_when_its_result_arrives() {
        let mut stream = ClaudeStream::default();
        let call = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Checking status."},{"type":"tool_use","id":"tu_1","name":"mcp__devagentics_worker__worker","input":{"action":"execute","input":{"command":"npm run typecheck"}}}]}}"#;
        assert_eq!(
            stream.events(call),
            [Event::Message("Checking status.".into())]
        );
        let result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu_1","is_error":true,"content":[{"type":"text","text":"task is paused"}]}]}}"#;
        assert_eq!(
            stream.events(result),
            [Event::Tool {
                action: "execute".into(),
                detail: "npm run typecheck".into(),
                outcome: Outcome::Failed("task is paused".into()),
            }]
        );
        let done = r#"{"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":12,"cache_read_input_tokens":3000,"output_tokens":400}}"#;
        assert_eq!(
            stream.events(done),
            [Event::TurnDone(Usage {
                input: 3012,
                cached: 3000,
                output: 400
            })]
        );
    }

    #[test]
    fn messages_are_formatted_markdown_that_wraps_at_the_width() {
        let text = "## Plan\n\n- **54** items across `five` milestones, each with its own acceptance tests and dependencies";
        let shown = render(&Event::Message(text.into()), 40).unwrap();
        assert!(shown.contains('\x1b'), "no formatting: {shown}");
        let plain = plain(&shown);
        assert!(!plain.contains("**") && !plain.contains("## "), "{plain}");
        assert!(plain.contains("54") && plain.contains("five"), "{plain}");
        assert!(
            plain.lines().all(|line| line.chars().count() <= 40),
            "overflows 40 columns:\n{plain}"
        );
    }

    #[test]
    fn a_turn_is_framed_by_who_it_is_and_what_it_used() {
        let turn = Turn {
            agent: "planner-5",
            program: "codex",
            model: Some("gpt-6.1-sol"),
            effort: Some("medium"),
            order: Some("o52 (B1-1-AT)".into()),
        };
        assert_eq!(
            plain(&header(&turn, "10:12", 80))
                .trim_end_matches('─')
                .trim_end(),
            "── planner-5 · codex · gpt-6.1-sol · medium · order o52 (B1-1-AT) · 10:12"
        );
        let done = plain(&footer(
            134,
            Some(Usage {
                input: 1136501,
                cached: 1069568,
                output: 8329,
            }),
            None,
            80,
        ));
        assert!(
            done.starts_with("── done in 2m 14s · 1.1M in (1.1M cached) · 8.3k out"),
            "{done}"
        );
        let failed = plain(&footer(5, None, Some("exit status 1"), 80));
        assert!(
            failed.starts_with("── failed after 5s: exit status 1"),
            "{failed}"
        );
    }
}
