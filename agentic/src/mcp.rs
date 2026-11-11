//! The leader's tools, as an MCP server on standard input and output
//! (`devagentics mcp <task>`): newline-delimited JSON-RPC, with the three
//! things a client asks of a tools server — `initialize`, `tools/list`,
//! `tools/call`. Each tool is a thin call into the modules that do the work.

use crate::devws::Devws;
use crate::home::Home;
use crate::leader;
use crate::log::Kind;
use crate::orders::{self, OrderRequest};
use crate::prs;
use crate::state::LEADER;
use crate::status;
use crate::task::Task;
use crate::team::Team;
use crate::watch::format_line;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

/// What a client without a preference is told.
const PROTOCOL_VERSION: &str = "2024-11-05";

pub struct Server<'a> {
    pub task: &'a Task,
    pub home: &'a Home,
    pub devws: &'a dyn Devws,
    /// The program that opens pull requests (`gh`).
    pub gh: String,
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

fn tools() -> Vec<Value> {
    let text = |what: &str| json!({ "type": "string", "description": what });
    let globs =
        |what: &str| json!({ "type": "array", "items": { "type": "string" }, "description": what });
    vec![
        tool(
            "task_status",
            "Where the team stands: the plan, repos, agents and their states, steps, what waits for you, limits, the recent log. Call it first, after any pause, and whenever you are unsure.",
            json!({}),
            &[],
        ),
        tool(
            "approve_plan",
            "Record that the user approved the plan, so agents that write may start. Call it only after the user said yes to the plan in this conversation; never on your own.",
            json!({}),
            &[],
        ),
        tool(
            "spawn_agent",
            "Start an agent with one of the roles task_status lists in a repo's worktree. It answers 'ready' and then takes orders. Refused for a writing role before the plan is approved (approve_plan), when a writer already works in that worktree, at the agent limit, or on an account near its limit.",
            json!({
                "role": text("The role's name (devagentics role list)"),
                "repo": text("The repo's name; the task's first by default"),
                "lane": text("For a factory-floor agent: the lane it writes in (add_lane); required when its repo has lanes"),
            }),
            &["role"],
        ),
        tool(
            "add_lane",
            "Add a lane: an area of a repo (back, front, infra, docs…) a factory-floor agent writes in, at the same time as the agents of other lanes. It gets a worktree of its own on a branch off the task's; an order to its agent first takes in the other lanes' accepted work, and accepting a step merges the lane into the task's branch. Lanes must not share files.",
            json!({
                "name": text("The lane's name: lowercase letters, digits and - (back, front, docs)"),
                "files": globs("Its files, globs from the repo's root (backend/**)"),
                "repo": text("The repo's name; the task's first by default"),
            }),
            &["name", "files"],
        ),
        tool(
            "add_repo",
            "Add another repo to the task: a worktree on the task's branch, from owner/repo, a URL or a folder. Then spawn agents in it.",
            json!({ "repo": text("owner/repo, a URL or a folder"), "check": text("The command that decides a step is done in this repo") }),
            &["repo"],
        ),
        tool(
            "send_order",
            "Give an agent one order. It is typed into the agent when it can take it. devAgentics checks the result itself when the agent reports: a clean tree, only the allowed files, the check.",
            json!({
                "agent": text("The agent's id (executor-1)"),
                "step": text("The plan's step this is (s1, 2…); the same step again after a failure"),
                "title": text("The step's name"),
                "text": text("What to do and what done means"),
                "allowed": globs("Globs of the files it may change, from the repo's root; none means any"),
                "forbidden": globs("Globs of the files it must not change"),
                "check": text("The command that decides it is done; the repo's by default. `true` for an order that changes no files"),
                "plan": text("For a factory-floor agent (required): the senior's step whose accepted execution plan this order follows"),
            }),
            &["agent", "step", "title", "text"],
        ),
        tool(
            "read_plan",
            "A senior's execution plan for a step it was ordered. Read it before you accept that step: accepting it approves the plan for the factory floor.",
            json!({ "step": text("The senior's step") }),
            &["step"],
        ),
        tool(
            "answer_question",
            "Answer a factory-floor agent's question (QUESTION q…) that came to you; the answer is typed into that agent.",
            json!({ "question": text("The question's id (q7)"), "text": text("The answer") }),
            &["question", "text"],
        ),
        tool(
            "accept_step",
            "Accept a step that was reported and checked. For a senior's step this approves its execution plan.",
            json!({ "step": text("The step's id") }),
            &["step"],
        ),
        tool(
            "reject_step",
            "Send a reported step back, with the reason; then order it again.",
            json!({ "step": text("The step's id"), "reason": text("What is wrong") }),
            &["step", "reason"],
        ),
        tool(
            "stop_agent",
            "End an agent: its panel closes and its worktree is free for another writer.",
            json!({ "agent": text("The agent's id") }),
            &["agent"],
        ),
        tool(
            "set_check",
            "Set the command that decides a step is done in a repo.",
            json!({ "repo": text("The repo's name"), "command": text("The command, run in the worktree") }),
            &["repo", "command"],
        ),
        tool(
            "read_log",
            "The task's log, newest last.",
            json!({
                "limit": { "type": "integer", "description": "How many entries (20 by default)" },
                "kinds": { "type": "array", "items": { "type": "string" }, "description": "Only these kinds: order, report, check, review, spawn, stop, escalate, pr…" },
            }),
            &[],
        ),
        tool(
            "escalate",
            "Put a decision to the user that you cannot make yourself. Logs it and marks you as needing them; then ask the question in your reply and stop.",
            json!({ "question": text("What you need decided, with the options") }),
            &["question"],
        ),
        tool(
            "switch_leader",
            "Move yourself, the leader, to your other program (Claude or Codex, your role's and its fallback). Use it when the user asks you to change model or program. devAgentics restarts your panel on the other program once this turn is over; the new leader catches up from task_status and the log. Refused when the other program's account has run out. After calling it, tell the user in one line and end your turn.",
            json!({ "reason": text("Why, for the log and the new leader (the user asked for Claude)") }),
            &[],
        ),
        tool(
            "open_prs",
            "When every step is accepted: push each repo's branch and open a draft pull request for each, linking the others.",
            json!({}),
            &[],
        ),
    ]
}

fn string(arguments: &Value, key: &str) -> Result<String> {
    arguments[key]
        .as_str()
        .map(str::to_string)
        .with_context(|| format!("'{key}' is required (a string)"))
}

fn optional_string(arguments: &Value, key: &str) -> Option<String> {
    arguments[key].as_str().map(str::to_string)
}

fn strings(arguments: &Value, key: &str) -> Result<Vec<String>> {
    match &arguments[key] {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .with_context(|| format!("'{key}' holds strings"))
            })
            .collect(),
        _ => bail!("'{key}' is a list of strings"),
    }
}

impl Server<'_> {
    /// Answers requests from `input` until it ends.
    pub fn serve(&self, input: impl BufRead, mut output: impl Write) -> Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let response = match serde_json::from_str::<Value>(&line) {
                Ok(request) => self.handle(&request),
                Err(error) => Some(failure(Value::Null, -32700, &format!("not JSON: {error}"))),
            };
            if let Some(response) = response {
                writeln!(output, "{response}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// The response to a request; none for a notification.
    pub fn handle(&self, request: &Value) -> Option<Value> {
        let method = request["method"].as_str().unwrap_or("");
        let id = request.get("id").cloned()?;
        Some(match method {
            "initialize" => success(
                id,
                json!({
                    "protocolVersion": request["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSION),
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "devagentics", "version": env!("CARGO_PKG_VERSION") },
                }),
            ),
            "ping" => success(id, json!({})),
            "tools/list" => success(id, json!({ "tools": tools() })),
            "tools/call" => {
                let name = request["params"]["name"].as_str().unwrap_or("");
                let result = self.call(name, &request["params"]["arguments"]);
                success(
                    id,
                    match result {
                        Ok(text) => {
                            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
                        }
                        Err(error) => {
                            json!({ "content": [{ "type": "text", "text": format!("{error:#}") }], "isError": true })
                        }
                    },
                )
            }
            _ => failure(id, -32601, &format!("no such method: {method}")),
        })
    }

    fn team(&self) -> Team<'_> {
        Team {
            task: self.task,
            home: self.home,
            devws: self.devws,
        }
    }

    fn call(&self, name: &str, arguments: &Value) -> Result<String> {
        match name {
            "task_status" => Ok(serde_json::to_string_pretty(&status::summary(
                self.task, self.devws, self.home,
            )?)?),
            "approve_plan" => {
                self.team().approve()?;
                Ok("Plan approved: agents that write may start.".to_string())
            }
            "add_lane" => {
                let lane = crate::lanes::add(
                    self.task,
                    &string(arguments, "name")?,
                    &strings(arguments, "files")?,
                    optional_string(arguments, "repo").as_deref(),
                )?;
                Ok(format!(
                    "Lane {} of {} ({}) at {}, on {}. spawn_agent with lane \"{}\" starts its agent.",
                    lane.name,
                    lane.repo,
                    lane.globs.join(", "),
                    lane.worktree.display(),
                    lane.branch,
                    lane.name
                ))
            }
            "spawn_agent" => {
                let agent = self.team().spawn_agent_in(
                    &string(arguments, "role")?,
                    optional_string(arguments, "repo").as_deref(),
                    optional_string(arguments, "lane").as_deref(),
                )?;
                Ok(format!(
                    "Started {} ({}) in {}, in pane {}. It will answer 'ready'; then send_order gives it work.",
                    agent.id, agent.role, agent.repo, agent.pane
                ))
            }
            "add_repo" => {
                let repo = self.team().add_repo(
                    &string(arguments, "repo")?,
                    optional_string(arguments, "check").as_deref(),
                )?;
                Ok(format!(
                    "Added {} at {}. spawn_agent can start agents in it (repo \"{}\").",
                    repo.name,
                    repo.worktree.display(),
                    repo.name
                ))
            }
            "send_order" => {
                let agent = string(arguments, "agent")?;
                let id = orders::send_order(
                    self.task,
                    OrderRequest {
                        agent: agent.clone(),
                        step: string(arguments, "step")?,
                        title: string(arguments, "title")?,
                        text: string(arguments, "text")?,
                        allowed: strings(arguments, "allowed")?,
                        forbidden: strings(arguments, "forbidden")?,
                        check: optional_string(arguments, "check"),
                        plan: optional_string(arguments, "plan"),
                    },
                )?;
                Ok(format!(
                    "Order {id} is queued for {agent}; it is typed in when {agent} can take it. Its report will come to you."
                ))
            }
            "read_plan" => crate::plans::read(self.task, &string(arguments, "step")?),
            "answer_question" => crate::questions::answer(
                self.task,
                crate::questions::Answerer::Leader,
                &string(arguments, "question")?,
                &string(arguments, "text")?,
            ),
            "accept_step" => {
                orders::accept_step(self.task, &string(arguments, "step")?)?;
                Ok("Accepted.".to_string())
            }
            "reject_step" => {
                orders::reject_step(
                    self.task,
                    &string(arguments, "step")?,
                    &string(arguments, "reason")?,
                )?;
                Ok("Rejected: order it again with send_order.".to_string())
            }
            "stop_agent" => {
                let agent = string(arguments, "agent")?;
                self.team().stop_agent(&agent)?;
                Ok(format!("{agent} is stopped."))
            }
            "set_check" => {
                self.team()
                    .set_check(&string(arguments, "repo")?, &string(arguments, "command")?)?;
                Ok("Set.".to_string())
            }
            "read_log" => self.read_log(arguments),
            "escalate" => self.escalate(&string(arguments, "question")?),
            "switch_leader" => {
                let said = leader::request_switch(
                    self.home,
                    self.task,
                    self.devws,
                    optional_string(arguments, "reason").as_deref(),
                )?;
                Ok(format!(
                    "{said} Tell the user in one line and end your turn."
                ))
            }
            "open_prs" => {
                let opened = prs::open_prs(self.task, &self.gh)?;
                Ok(opened
                    .iter()
                    .map(|(repo, url)| format!("{repo}: {url}"))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            other => bail!("no such tool: {other}"),
        }
    }

    fn read_log(&self, arguments: &Value) -> Result<String> {
        let limit = arguments["limit"].as_u64().unwrap_or(20) as usize;
        let kinds = strings(arguments, "kinds")?;
        let lines: Vec<String> = self
            .task
            .log_lines()?
            .into_iter()
            .filter(|line| {
                kinds.is_empty()
                    || serde_json::from_str::<Value>(line)
                        .ok()
                        .and_then(|entry| {
                            entry["kind"]
                                .as_str()
                                .map(|kind| kinds.iter().any(|wanted| wanted == kind))
                        })
                        .unwrap_or(false)
            })
            .collect();
        let shown: Vec<_> = lines[lines.len().saturating_sub(limit)..]
            .iter()
            .map(|line| format_line(line))
            .collect();
        Ok(if shown.is_empty() {
            "The log has nothing like that yet.".to_string()
        } else {
            shown.join("\n")
        })
    }

    /// A decision for the user. Their agent marks it, and the reply is theirs.
    fn escalate(&self, question: &str) -> Result<String> {
        self.task.transaction(|tx| {
            tx.log(Kind::Escalate, Some(LEADER), None, question);
            Ok(())
        })?;
        if let Some(pane) = self
            .devws
            .pane_with_role(&self.task.config()?.workspace, "agent")?
        {
            self.devws.mark(&pane, "waiting")?;
        }
        Ok("The user will see you need them. Put the question to them in your reply, with the options, and stop.".to_string())
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_args::Kind as Program;
    use crate::devws::fake::FakeDevws;
    use crate::start::start_task;
    use std::io::Cursor;
    use tempfile::TempDir;

    struct Fixture {
        _dir: TempDir,
        home: Home,
        task: Task,
        devws: FakeDevws,
        _repo: TempDir,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&dir.path().join("w")).with_kind("claude", Program::Claude);
        start_task(&home, &devws, repo.path(), "d", None).unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        Fixture {
            _dir: dir,
            home,
            task,
            devws,
            _repo: repo,
        }
    }

    impl Fixture {
        fn server(&self) -> Server<'_> {
            Server {
                task: &self.task,
                home: &self.home,
                devws: &self.devws,
                gh: "gh".to_string(),
            }
        }

        fn call(&self, name: &str, arguments: Value) -> (bool, String) {
            let response = self.server().handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": arguments } })).unwrap();
            (
                response["result"]["isError"].as_bool().unwrap(),
                response["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            )
        }
    }

    #[test]
    fn it_introduces_itself_and_echoes_the_clients_protocol_version() {
        let f = fixture();
        let response = f.server().handle(&json!({ "jsonrpc": "2.0", "id": 7, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } })).unwrap();
        assert_eq!(response["id"], 7);
        assert_eq!(response["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(response["result"]["serverInfo"]["name"], "devagentics");
        assert!(response["result"]["capabilities"]["tools"].is_object());
        let without = f
            .server()
            .handle(&json!({ "jsonrpc": "2.0", "id": 8, "method": "initialize" }))
            .unwrap();
        assert_eq!(without["result"]["protocolVersion"], PROTOCOL_VERSION);
    }

    #[test]
    fn it_lists_the_leaders_tools_with_their_schemas() {
        let f = fixture();
        let response = f
            .server()
            .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
            .unwrap();
        let listed = response["result"]["tools"].as_array().unwrap();
        let names: Vec<_> = listed
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "task_status",
                "approve_plan",
                "spawn_agent",
                "add_lane",
                "add_repo",
                "send_order",
                "read_plan",
                "answer_question",
                "accept_step",
                "reject_step",
                "stop_agent",
                "set_check",
                "read_log",
                "escalate",
                "switch_leader",
                "open_prs"
            ]
        );
        for tool in listed {
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
            assert!(tool["description"].as_str().unwrap().len() > 20);
        }
        let required = listed
            .iter()
            .find(|tool| tool["name"] == "send_order")
            .unwrap()["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .len();
        assert_eq!(required, 4);
    }

    #[test]
    fn the_leader_asks_to_move_to_its_other_program() {
        let f = fixture();
        let (failed, text) = f.call(
            "switch_leader",
            json!({ "reason": "the user asked for Codex" }),
        );
        assert!(!failed, "{text}");
        assert!(
            text.starts_with("The leader moves to codex (gpt-6.1-sol) once its turn is over."),
            "{text}"
        );
        assert_eq!(
            f.task.state().unwrap().leader_switch.as_deref(),
            Some("the user asked for Codex")
        );
    }

    #[test]
    fn notifications_get_no_answer_pings_do_and_unknown_methods_are_errors() {
        let f = fixture();
        assert!(
            f.server()
                .handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
                .is_none()
        );
        assert_eq!(
            f.server()
                .handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }))
                .unwrap()["result"],
            json!({})
        );
        let unknown = f
            .server()
            .handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" }))
            .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    #[test]
    fn the_leader_records_the_users_approval_and_writers_may_start() {
        let f = fixture();
        let (failed, text) = f.call("approve_plan", json!({}));
        assert!(failed && text.contains("no plan to approve"), "{text}");
        crate::team::Team {
            task: &f.task,
            home: &f.home,
            devws: &f.devws,
        }
        .write_plan("1. s1: add a file\n")
        .unwrap();
        let (failed, text) = f.call("approve_plan", json!({}));
        assert!(!failed && text.contains("approved"), "{text}");
        let (failed, text) = f.call("spawn_agent", json!({ "role": "executor" }));
        assert!(!failed, "{text}");
    }

    #[test]
    fn a_tool_that_fails_answers_with_its_reason_not_a_protocol_error() {
        let f = fixture();
        let (failed, text) = f.call("spawn_agent", json!({ "role": "executor" }));
        assert!(failed && text.contains("not approved"), "{text}");
        let (failed, text) = f.call("spawn_agent", json!({}));
        assert!(failed && text.contains("'role' is required"), "{text}");
        let (failed, text) = f.call("no_such_tool", json!({}));
        assert!(failed && text.contains("no such tool"), "{text}");
        let (failed, text) = f.call(
            "send_order",
            json!({ "agent": "x", "step": "s", "title": "t", "text": "t", "allowed": "src/**" }),
        );
        assert!(failed && text.contains("list of strings"), "{text}");
    }

    #[test]
    fn the_leader_can_start_a_planner_give_it_an_order_and_read_the_status() {
        let f = fixture();
        let (failed, text) = f.call("spawn_agent", json!({ "role": "planner" }));
        assert!(!failed && text.contains("Started planner-1"), "{text}");

        let (failed, text) = f.call("send_order", json!({ "agent": "planner-1", "step": "plan", "title": "Plan it", "text": "Write the plan", "check": "true" }));
        assert!(
            !failed && text.contains("Order o") && text.contains("queued for planner-1"),
            "{text}"
        );

        let (failed, status) = f.call("task_status", json!({}));
        assert!(!failed);
        let status: Value = serde_json::from_str(&status).unwrap();
        assert_eq!(status["agents"][0]["order"]["step"], "plan");
        assert_eq!(status["steps"][0]["status"], "ordered");

        let (_, stopped) = f.call("stop_agent", json!({ "agent": "planner-1" }));
        assert!(stopped.contains("planner-1 is stopped"));
        let (failed, text) = f.call("accept_step", json!({ "step": "plan" }));
        assert!(
            failed && text.contains("not waiting for a review"),
            "{text}"
        );
    }

    #[test]
    fn the_log_can_be_read_by_kind() {
        let f = fixture();
        f.call("spawn_agent", json!({ "role": "planner" }));
        let (_, all) = f.call("read_log", json!({}));
        assert!(
            all.contains("task started") && all.contains("spawn"),
            "{all}"
        );
        let (_, spawns) = f.call("read_log", json!({ "kinds": ["spawn"] }));
        assert!(
            spawns.contains("spawn") && !spawns.contains("task started"),
            "{spawns}"
        );
        let (_, none) = f.call("read_log", json!({ "kinds": ["pr"] }));
        assert!(none.contains("nothing like that"));
        let (_, last) = f.call("read_log", json!({ "limit": 1 }));
        assert_eq!(last.lines().count(), 1);
    }

    #[test]
    fn escalating_logs_the_question_and_marks_the_leader_as_needing_the_user() {
        let f = fixture();
        f.devws
            .set_role(&f.task.config().unwrap().workspace, "agent", "%1");
        f.devws.set_state("%1", crate::devws::PaneState::Working);
        let (failed, text) = f.call("escalate", json!({ "question": "Keep the old API?" }));
        assert!(
            !failed && text.contains("Put the question to them"),
            "{text}"
        );
        assert!(f.devws.calls().contains(&"mark %1 waiting".to_string()));
        assert!(f.task.log_lines().unwrap().iter().any(
            |line| line.contains("\"kind\":\"escalate\"") && line.contains("Keep the old API?")
        ));
    }

    #[test]
    fn it_serves_a_session_over_lines() {
        let f = fixture();
        let input = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "",
            "not json",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ]
        .join("\n");
        let mut output = Vec::new();
        f.server().serve(Cursor::new(input), &mut output).unwrap();
        let responses: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            responses.len(),
            3,
            "one per request, none for the notification"
        );
        assert_eq!(responses[0]["id"], 1);
        assert_eq!(responses[1]["error"]["code"], -32700);
        assert_eq!(responses[2]["id"], 2);
    }
}
