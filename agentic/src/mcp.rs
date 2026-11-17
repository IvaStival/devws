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
use crate::state::{Effect, LEADER, PermissionRequest, PermissionStatus};
use crate::status;
use crate::task::Task;
use crate::team::Team;
use crate::watch::format_line;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

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
    let mut tools = vec![
        tool(
            "application_workflow",
            "Project application planning. Actions: status (an outline; input.section for one part in full, e.g. runner; section application or proposal with input.items [ids] for those full contracts), propose (input.application: the whole next revision), amend (input.changes: only what changes in the next revision, by id, null removes; input.reason), answer, confirm, approve, milestones, preview_adoption, adopt, review_request, provision_roles, runner_configure (input.configuration; its lanes: {lane: {image, image_mounts}} give a lane its own image), image_build (input: name, dockerfile, context_files, repo; builds a runner image on the user's rootless Docker and returns its ID), recover_turn, validate_milestone, leader_inbox, inbox_ack, runner_images, project_overview, preflight_retry. provision_roles changes managed storage outside the project and requires exact-call external authorization; so does a runner_configure that changes docker_host. Other runner changes and image builds are yours to decide. Only record confirm/approve after the user's explicit answer for that exact revision. Material proposals never inherit approval. Preview is read-only; runner enforcement may block adoption.",
            json!({"action":text("Workflow action"),"input":{"type":"object"}}),
            &["action"],
        ),
        tool(
            "pause_task",
            "Pause deliveries; optionally interrupt active turns without closing panes.",
            json!({"interrupt": {"type":"boolean"}}),
            &[],
        ),
        tool(
            "resume_task",
            "Queue authoritative recovery work and resume deliveries.",
            json!({}),
            &[],
        ),
        tool(
            "configure_checks",
            "Configure repository named checks and isolated environments; configuration is a RepositoryChecks JSON object.",
            json!({"repo": text("Repo name"), "configuration": {"type":"object"}}),
            &["repo", "configuration"],
        ),
        tool(
            "run_check",
            "Run checks for an exact delivered order and return structured evidence.",
            json!({"order": text("Order ID")}),
            &["order"],
        ),
        tool(
            "amend_order",
            "Amend an open order allowlist, preserving its identity and base.",
            json!({"order": text("Order ID"), "allowed": globs("Additional allowed paths or globs"), "reason": text("Authorization reason")}),
            &["order", "allowed", "reason"],
        ),
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
            "set_usage_limit_approval",
            "Record the user's explicit approval, denial or revocation of a pending worker percentage-limit override. Never infer consent from plan approval. Applies to this task until revoked; provider quota errors still apply. After approval call resume_task to restart existing blocked workers, not spawn_agent. Reconsider a denied request only when the user explicitly asks.",
            json!({"approved": {"type":"boolean"}}),
            &["approved"],
        ),
        tool(
            "authorize_action",
            "After the user explicitly approves or denies a pending call outside the current project, record that decision. Approval is for one exact invocation only; pass the returned request ID as approval_id when retrying that same tool with unchanged arguments.",
            json!({"request": text("Pending permission request ID"), "approved": {"type":"boolean", "description":"True only after explicit user approval"}}),
            &["request", "approved"],
        ),
        tool(
            "spawn_agent",
            "Start an agent with one of the roles task_status lists in a repo's worktree. It answers 'ready' and then takes orders. Refused for a writing role before the plan is approved (approve_plan), when a writer already works in that worktree, at the agent limit, or when no compatible startup candidate is available. Account exhaustion and explicit model/provider unavailability automatically try alternatives without separate approval.",
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
            json!({ "repo": text("owner/repo, a URL or a folder"), "check": text("The command that decides a step is done in this repo"), "approval_id": text("One-use approval request ID, only after explicit user approval") }),
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
                "profiles": globs("Selected named check profiles; cannot mix with a legacy check"),
                "check": text("The command that decides it is done; the repo's by default. `true` for an order that changes no files"),
                "plan": text("For a factory-floor agent (required): the senior's step whose accepted execution plan this order follows"),
                "spec": text("The plan part this step belongs to, as a release version: a new part takes task_status's next_version (1.3.0) and its steps are named 1.3.1, 1.3.2…; the board shows finished work by it. Give it on the first order of a step"),
            }),
            &["agent", "step", "title", "text"],
        ),
        tool(
            "graph",
            "Ask the code graph (when the user turned it on for the task) about a repo at its task branch: action=explain with symbol (where it is defined, what calls it and what it calls; an ambiguous name lists its candidates: retry with path::symbol), path with from and to (how one reaches the other), or query with question (a bounded search). Use it for reuse, interfaces, callers and the impact of a change before planning or accepting work.",
            json!({
                "action": text("explain, path or query"),
                "symbol": text("For explain: a name, or path::name"),
                "from": text("For path: the first symbol"),
                "to": text("For path: the second symbol"),
                "question": text("For query: a question about the code"),
                "repo": text("The repo (default: the task's first)")
            }),
            &["action"],
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
            "End a task-owned agent without separate approval: its panel closes and its worktree is free for another writer.",
            json!({ "agent": text("The task-owned agent's id") }),
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
            json!({ "approval_id": text("One-use approval request ID, only after explicit user approval") }),
            &[],
        ),
    ];
    for entry in &mut tools {
        if let Some(name) = entry["name"].as_str()
            && let Some(effect) = effect(name)
        {
            let description = entry["description"].as_str().unwrap_or_default();
            entry["description"] = json!(format!("Effect: {}. {description}", effect.name()));
        }
    }
    tools
}

/// Only actions crossing the project boundary require separate authorization.
fn effect(name: &str) -> Option<Effect> {
    match name {
        "application_workflow" => Some(Effect::System),
        "add_repo" | "open_prs" => Some(Effect::External),
        "stop_agent" => Some(Effect::System),
        "set_usage_limit_approval"
        | "spawn_agent"
        | "pause_task"
        | "resume_task"
        | "configure_checks"
        | "run_check"
        | "amend_order"
        | "task_status"
        | "approve_plan"
        | "add_lane"
        | "send_order"
        | "graph"
        | "read_plan"
        | "answer_question"
        | "accept_step"
        | "reject_step"
        | "set_check"
        | "read_log"
        | "escalate"
        | "switch_leader"
        | "authorize_action" => Some(Effect::System),
        _ => None,
    }
}

fn validate_required(name: &str, arguments: &Value) -> Result<()> {
    let Some(definition) = tools().into_iter().find(|tool| tool["name"] == name) else {
        return Ok(());
    };
    if let Some(required) = definition["inputSchema"]["required"].as_array() {
        for key in required.iter().filter_map(Value::as_str) {
            if arguments.get(key).is_none_or(Value::is_null) {
                bail!("'{key}' is required");
            }
        }
    }
    Ok(())
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
    /// Answers requests from `input` until it ends. Once a reinstall has
    /// replaced this binary, the installed one answers instead, so the
    /// leader never runs old tools until its next start.
    pub fn serve(&self, input: impl BufRead, output: impl Write) -> Result<()> {
        let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
        self.serve_with(input, output, || {
            let running = std::env::current_exe().ok()?;
            let binary = crate::executable::replacement(&running)?;
            Some((binary, arguments.clone()))
        })
    }

    fn serve_with(
        &self,
        input: impl BufRead,
        mut output: impl Write,
        replacement: impl Fn() -> Option<(PathBuf, Vec<OsString>)>,
    ) -> Result<()> {
        let mut announced = false;
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some((binary, arguments)) = replacement() {
                if !announced {
                    // The client reloads the tools, so it sees the new ones.
                    writeln!(
                        output,
                        "{}",
                        json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
                    )?;
                    announced = true;
                }
                output.write_all(&relayed(&binary, &arguments, &line))?;
                output.flush()?;
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
                    "capabilities": { "tools": { "listChanged": true } },
                    "serverInfo": { "name": "devagentics", "version": env!("CARGO_PKG_VERSION") },
                }),
            ),
            "ping" => success(id, json!({})),
            "tools/list" => success(id, json!({ "tools": tools() })),
            "tools/call" => {
                let name = request["params"]["name"].as_str().unwrap_or("");
                let result = self
                    .call(name, &request["params"]["arguments"])
                    .and_then(|text| fitting(name, text));
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
        if name == "authorize_action" {
            return self.authorize_action(arguments);
        }
        validate_required(name, arguments)?;
        if name == "application_workflow" && self.leaves_project(arguments)? {
            return self.authorized_call(name, arguments, Effect::External);
        }
        if let Some(effect) = effect(name) {
            if effect != Effect::System {
                return self.authorized_call(name, arguments, effect);
            }
        }
        self.call_inner(name, arguments)
    }

    /// Whether an `application_workflow` call reaches outside the project:
    /// provisioning roles writes devAgentics' home, and a runner change that
    /// moves to another Docker daemon leaves the user's own. Every other
    /// runner change stays inside the bounds its validation keeps (local
    /// image IDs, capped memory and CPUs, image folders, no network): the
    /// leader decides it.
    fn leaves_project(&self, arguments: &Value) -> Result<bool> {
        Ok(match arguments["action"].as_str() {
            Some("provision_roles") => true,
            Some("runner_configure") => {
                let requested = arguments["input"]["configuration"]["docker_host"].as_str();
                let current = self
                    .task
                    .state()?
                    .runner
                    .and_then(|runner| runner.docker_host);
                requested != current.as_deref()
            }
            _ => false,
        })
    }

    fn authorized_call(&self, name: &str, arguments: &Value, effect: Effect) -> Result<String> {
        let approval_id = optional_string(arguments, "approval_id");
        let mut exact_arguments = arguments.clone();
        if let Some(object) = exact_arguments.as_object_mut() {
            object.remove("approval_id");
        }
        let Some(approval_id) = approval_id else {
            let request = self.task.transaction(|tx| {
                if let Some(existing) = tx.state.permissions.iter().find(|request| {
                    request.tool == name
                        && request.arguments == exact_arguments
                        && request.effect == effect
                        && request.status == PermissionStatus::Pending
                }) {
                    return Ok(existing.id.clone());
                }
                let id = format!("p{}", tx.state.next_number());
                tx.state.permissions.push(PermissionRequest {
                    id: id.clone(),
                    tool: name.to_string(),
                    arguments: exact_arguments.clone(),
                    effect,
                    status: PermissionStatus::Pending,
                });
                tx.log(
                    Kind::Escalate,
                    Some(LEADER),
                    None,
                    &format!("Permission {id} requested for {name} ({})", effect.name()),
                );
                Ok(id)
            })?;
            return Ok(format!(
                "Permission required ({}) for {name}. Request {request}. No action was taken. Show the user the exact invocation outside the project: {}. Ask for approval; only after an explicit yes call authorize_action with this request ID and approved=true. Then retry the same tool with the same arguments and approval_id={request}.",
                effect.name(),
                serde_json::to_string(&exact_arguments)?
            ));
        };

        self.task.transaction(|tx| {
            let request = tx
                .state
                .permissions
                .iter_mut()
                .find(|request| request.id == approval_id)
                .with_context(|| format!("permission request {approval_id} does not exist"))?;
            if request.tool != name
                || request.arguments != exact_arguments
                || request.effect != effect
            {
                bail!("permission {approval_id} is bound to a different tool invocation");
            }
            if request.status != PermissionStatus::Authorized {
                bail!("permission {approval_id} is not authorized or has already been used");
            }
            request.status = PermissionStatus::Consumed;
            Ok(())
        })?;
        self.call_inner(name, &exact_arguments)
    }

    fn authorize_action(&self, arguments: &Value) -> Result<String> {
        let id = string(arguments, "request")?;
        let approved = arguments["approved"]
            .as_bool()
            .context("'approved' is required (a boolean)")?;
        self.task.transaction(|tx| {
            let request = tx
                .state
                .permissions
                .iter_mut()
                .find(|request| request.id == id)
                .with_context(|| format!("permission request {id} does not exist"))?;
            if request.status != PermissionStatus::Pending {
                bail!("permission request {id} is no longer pending");
            }
            request.status = if approved {
                PermissionStatus::Authorized
            } else {
                PermissionStatus::Denied
            };
            tx.log(
                Kind::Review,
                Some(LEADER),
                None,
                &format!(
                    "Permission {id} {}",
                    if approved { "authorized" } else { "denied" }
                ),
            );
            Ok(())
        })?;
        Ok(if approved {
            format!(
                "Permission {id} authorized for one exact invocation. Retry that tool once with approval_id={id}."
            )
        } else {
            format!("Permission {id} denied. The action will not run.")
        })
    }

    fn call_inner(&self, name: &str, arguments: &Value) -> Result<String> {
        match name {
            "application_workflow" => Ok(serde_json::to_string(&crate::workflow::call(
                self.task,
                &string(arguments, "action")?,
                arguments.get("input").unwrap_or(&json!({})),
            )?)?),
            "task_status" => Ok(serde_json::to_string(&status::summary(
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
            "set_usage_limit_approval" => {
                let approved = arguments["approved"]
                    .as_bool()
                    .context("approved must be a boolean")?;
                self.team().set_usage_limit_approval(approved)?;
                Ok(if approved {
                    "Worker percentage override approved for this task until revoked. Call resume_task for existing blocked workers; provider errors remain enforced."
                } else {
                    "Worker percentage override denied or revoked. Do not ask again unless the user requests reconsideration."
                }.into())
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
            "pause_task" => {
                self.team().pause_mode(
                    arguments
                        .get("interrupt")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    std::env::var("TMUX_PANE").ok().as_deref(),
                )?;
                Ok("Paused.".into())
            }
            "resume_task" => {
                let restarted = self
                    .team()
                    .resume_from(std::env::var("TMUX_PANE").ok().as_deref())?;
                Ok(format!("Resumed; restarted: {}", restarted.join(", ")))
            }
            "configure_checks" => {
                let config = serde_json::from_value(
                    arguments
                        .get("configuration")
                        .context("missing configuration")?
                        .clone(),
                )?;
                crate::check_profiles::configure(self.task, &string(arguments, "repo")?, config)?;
                Ok("Check profiles configured.".into())
            }
            "run_check" => Ok(serde_json::to_string(
                &crate::check_profiles::run(self.task, &string(arguments, "order")?, None)?
                    .response(),
            )?),
            "amend_order" => orders::amend(
                self.task,
                &string(arguments, "order")?,
                strings(arguments, "allowed")?,
                &string(arguments, "reason")?,
                "leader MCP",
            ),
            "send_order" => {
                let agent = string(arguments, "agent")?;
                let id = orders::send_order(
                    self.task,
                    OrderRequest {
                        profiles: strings(arguments, "profiles")?,
                        agent: agent.clone(),
                        step: string(arguments, "step")?,
                        title: string(arguments, "title")?,
                        text: string(arguments, "text")?,
                        allowed: strings(arguments, "allowed")?,
                        forbidden: strings(arguments, "forbidden")?,
                        check: optional_string(arguments, "check"),
                        plan: optional_string(arguments, "plan"),
                        spec: optional_string(arguments, "spec"),
                    },
                )?;
                Ok(format!(
                    "Order {id} is queued for {agent}; it is typed in when {agent} can take it. Its report will come to you."
                ))
            }
            "read_plan" => crate::plans::read(self.task, &string(arguments, "step")?),
            "graph" => {
                let config = self.task.config()?;
                let repo = match arguments["repo"].as_str() {
                    Some(name) => config.repos.iter().find(|repo| repo.name == name),
                    None => config.repos.first(),
                }
                .context("unknown repo")?;
                let head = crate::checks::head(&repo.worktree)?;
                crate::graph::answer(
                    &crate::graph::Graphify::from_env()?,
                    self.task,
                    repo,
                    &head,
                    arguments,
                )
            }
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
            tx.state.leader_wait = crate::state::LeaderWait::Asked;
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

/// One request line answered by a fresh `binary` server with `arguments`:
/// a request carries all it needs, so nothing is lost between servers. When
/// that server cannot answer, the request gets the reason.
fn relayed(binary: &Path, arguments: &[OsString], line: &str) -> Vec<u8> {
    let answer = (|| -> Result<Vec<u8>> {
        let mut child = std::process::Command::new(binary)
            .args(arguments)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("starting the installed devagentics at {}", binary.display()))?;
        child
            .stdin
            .take()
            .context("no input to the installed devagentics")?
            .write_all(format!("{line}\n").as_bytes())?;
        let output = child.wait_with_output()?;
        ensure!(
            output.status.success(),
            "the installed devagentics at {} failed ({})",
            binary.display(),
            output.status
        );
        Ok(output.stdout)
    })();
    answer.unwrap_or_else(|error| {
        let id = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|request| request.get("id").cloned());
        match id {
            Some(id) => format!("{}\n", failure(id, -32603, &format!("{error:#}"))).into_bytes(),
            None => Vec::new(),
        }
    })
}

/// The most a tool result may hold: past it Claude Code saves the result to
/// a file, which an MCP-only leader cannot open.
const MAX_RESULT_CHARS: usize = 80_000;

/// A result too big for the leader becomes what to ask for instead.
fn fitting(name: &str, text: String) -> Result<String> {
    let size = text.chars().count();
    ensure!(
        size <= MAX_RESULT_CHARS,
        "{name}'s result is {} KB, more than your tools take in ({} KB). Ask for less: application_workflow status takes section (runner, ready_work, reviews, …) and, for application or proposal, items [ids].",
        size / 1000,
        MAX_RESULT_CHARS / 1000
    );
    Ok(text)
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
    fn a_replaced_server_hands_each_request_to_the_installed_binary() {
        let f = fixture();
        let folder = TempDir::new().unwrap();
        let installed = folder.path().join("devagentics");
        // The installed binary stands in as an echo: its answer is the request.
        std::fs::write(&installed, "#!/bin/sh\ncat\n").unwrap();
        std::fs::set_permissions(&installed, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let requests = "{\"id\":1,\"method\":\"tools/list\"}\n{\"id\":2,\"method\":\"ping\"}\n";
        let mut output = Vec::new();
        f.server()
            .serve_with(Cursor::new(requests), &mut output, || {
                Some((installed.clone(), vec!["mcp".into(), "feat-x".into()]))
            })
            .unwrap();
        let lines: Vec<&str> = std::str::from_utf8(&output).unwrap().lines().collect();
        assert_eq!(
            lines,
            [
                r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#,
                r#"{"id":1,"method":"tools/list"}"#,
                r#"{"id":2,"method":"ping"}"#,
            ],
            "the tools reload once, then every answer is the installed one's"
        );

        let missing = relayed(&folder.path().join("gone"), &[], r#"{"id":5,"method":"ping"}"#);
        let answer: Value = serde_json::from_slice(&missing).unwrap();
        assert_eq!(answer["id"], 5);
        assert!(answer["error"]["message"].as_str().unwrap().contains("starting the installed devagentics"));
        assert!(relayed(&folder.path().join("gone"), &[], r#"{"method":"note"}"#).is_empty(), "a notification gets no answer");
    }

    #[test]
    fn a_current_server_answers_itself() {
        let f = fixture();
        let mut output = Vec::new();
        f.server()
            .serve_with(Cursor::new("{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}\n"), &mut output, || None)
            .unwrap();
        let answer: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(answer["id"], 3);
        assert_eq!(answer["result"], json!({}));
    }

    #[test]
    fn a_result_too_big_for_the_leader_says_what_to_ask_for() {
        let error = fitting("task_status", "x".repeat(MAX_RESULT_CHARS + 1)).unwrap_err().to_string();
        assert!(error.contains("80 KB"), "{error}");
        assert!(error.contains("section"), "{error}");
        assert_eq!(fitting("task_status", "small".into()).unwrap(), "small");
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
                "application_workflow",
                "pause_task",
                "resume_task",
                "configure_checks",
                "run_check",
                "amend_order",
                "task_status",
                "approve_plan",
                "set_usage_limit_approval",
                "authorize_action",
                "spawn_agent",
                "add_lane",
                "add_repo",
                "send_order",
                "graph",
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
        assert!(!failed && text.contains("Started"), "{text}");
    }

    #[test]
    fn the_leader_asks_the_code_graph_only_when_the_user_turned_it_on() {
        let f = fixture();
        let (failed, text) = f.call("graph", json!({"action": "explain", "symbol": "app"}));
        assert!(failed && text.contains("devagentics task graph"), "{text}");
        let (failed, text) = f.call("graph", json!({"action": "explain", "repo": "nope"}));
        assert!(failed && text.contains("unknown repo"), "{text}");
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
    fn usage_approval_is_separate_from_plan_approval_and_validated() {
        let mut f = fixture();
        f.devws = f.devws.with_kind("codex", Program::Codex);
        assert!(f.call("set_usage_limit_approval", json!({})).0);
        assert!(
            f.call("set_usage_limit_approval", json!({"approved":"yes"}))
                .0
        );
        assert!(
            f.call("set_usage_limit_approval", json!({"approved":true}))
                .0
        );
        for account in ["claude", "codex"] {
            f.devws.set_limits(
                account,
                crate::devws::Limits {
                    week: None,
                    five_hour: Some(95),
                },
            );
        }
        assert!(f.call("spawn_agent", json!({"role":"planner"})).0);
        let (_, status) = f.call("task_status", json!({}));
        let status: Value = serde_json::from_str(&status).unwrap();
        assert_eq!(status["usage_limit_approval"]["status"], "pending");
        assert_eq!(
            status["usage_limit_approval"]["blocked_accounts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(
            !f.call("set_usage_limit_approval", json!({"approved":true}))
                .0
        );
        assert!(!f.task.state().unwrap().approved);
        assert!(!f.call("resume_task", json!({})).0);
        assert!(f.task.state().unwrap().agents[0].live);
        assert!(
            !f.call("set_usage_limit_approval", json!({"approved":false}))
                .0
        );
        assert_eq!(
            f.task.state().unwrap().usage_limit_approval,
            crate::state::UsageApproval::Denied
        );
    }

    #[test]
    fn leader_starts_agents_without_separate_permission() {
        let f = fixture();
        let (failed, text) = f.call("spawn_agent", json!({"role":"planner"}));
        assert!(!failed && text.contains("Started planner-1"), "{text}");
        assert!(f.task.state().unwrap().permissions.is_empty());
        assert_eq!(f.task.state().unwrap().agents.len(), 1);
    }

    #[test]
    fn leader_stops_and_replaces_only_task_owned_agents_without_permission() {
        let f = fixture();
        let (_, text) = f.call("spawn_agent", json!({"role":"planner"}));
        assert!(text.contains("Started planner-1"));
        let pane = f.task.state().unwrap().agents[0].pane.clone();
        for foreign in ["%1", "other-task-agent", "leader"] {
            let (failed, _) = f.call("stop_agent", json!({"agent":foreign}));
            assert!(failed);
            assert!(f.task.state().unwrap().agents[0].live);
        }
        let (failed, text) = f.call("stop_agent", json!({"agent":"planner-1"}));
        assert!(!failed, "{text}");
        assert_eq!(
            f.devws.pane_state(&pane).unwrap(),
            crate::devws::PaneState::Gone
        );
        let (failed, text) = f.call("spawn_agent", json!({"role":"planner"}));
        assert!(!failed && text.contains("Started planner-2"), "{text}");
        assert!(f.task.state().unwrap().permissions.is_empty());
    }

    #[test]
    fn external_calls_require_exact_one_use_authorization() {
        let f = fixture();
        let arguments = json!({"repo":"acme/library"});
        let (failed, text) = f.call("add_repo", arguments.clone());
        assert!(!failed && text.contains("No action was taken"), "{text}");
        assert_eq!(f.task.config().unwrap().repos.len(), 1);
        let request = f.task.state().unwrap().permissions[0].id.clone();
        f.call(
            "authorize_action",
            json!({"request":request,"approved":true}),
        );
        let (failed, text) = f.call("add_repo", json!({"repo":"other","approval_id":request}));
        assert!(
            failed && text.contains("different tool invocation"),
            "{text}"
        );
        let arguments = json!({"repo":"acme/library","approval_id":request});
        let (failed, text) = f.call("add_repo", arguments.clone());
        assert!(!failed, "{text}");
        assert_eq!(
            f.task.state().unwrap().permissions[0].status,
            PermissionStatus::Consumed
        );
        let (failed, text) = f.call("add_repo", arguments);
        assert!(
            failed && text.contains("not authorized or has already been used"),
            "{text}"
        );
    }

    #[test]
    fn runner_changes_are_the_leaders_unless_they_change_the_docker_daemon() {
        let f = fixture();
        let configure = |docker_host: Value| {
            json!({"action": "runner_configure", "input": {"configuration": {
                "image": format!("sha256:{}", "a".repeat(64)),
                "source_paths": ["src/**"], "protected_paths": [], "timeout_seconds": 60,
                "docker_host": docker_host,
            }}})
        };
        let (_, text) = f.call("application_workflow", configure(Value::Null));
        assert!(!text.contains("No action was taken"), "{text}");
        assert!(
            f.task.state().unwrap().permissions.is_empty(),
            "the leader decides"
        );

        let (failed, text) = f.call(
            "application_workflow",
            configure(json!("unix:///run/other/docker.sock")),
        );
        assert!(!failed && text.contains("No action was taken"), "{text}");
        let (failed, text) = f.call(
            "application_workflow",
            json!({"action": "provision_roles", "input": {"approved_storage": true}}),
        );
        assert!(!failed && text.contains("No action was taken"), "{text}");
        assert_eq!(f.task.state().unwrap().permissions.len(), 2);
    }

    #[test]
    fn denied_external_calls_never_add_a_repository() {
        let f = fixture();
        f.call("add_repo", json!({"repo":"acme/library"}));
        let request = f.task.state().unwrap().permissions[0].id.clone();
        f.call(
            "authorize_action",
            json!({"request":request,"approved":false}),
        );
        let (failed, _) = f.call(
            "add_repo",
            json!({"repo":"acme/library","approval_id":request}),
        );
        assert!(failed);
        assert_eq!(f.task.config().unwrap().repos.len(), 1);
        assert_eq!(
            f.task.state().unwrap().permissions[0].status,
            PermissionStatus::Denied
        );
    }

    #[test]
    fn tool_catalog_labels_effects_and_includes_authorization() {
        let listed = tools();
        let described = |name: &str| {
            listed.iter().find(|entry| entry["name"] == name).unwrap()["description"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert!(described("add_repo").starts_with("Effect: external."));
        assert!(described("open_prs").starts_with("Effect: external."));
        assert!(described("task_status").starts_with("Effect: system."));
        assert!(described("spawn_agent").starts_with("Effect: system."));
        assert!(described("stop_agent").starts_with("Effect: system."));
        assert!(
            listed
                .iter()
                .any(|entry| entry["name"] == "authorize_action")
        );
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
        assert_eq!(
            status["steps"][0]["version"], "1.1.1",
            "its spec is the first"
        );
        assert_eq!(status["next_version"], "1.2.0", "the next plan part's name");

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
        assert_eq!(
            f.task.state().unwrap().leader_wait,
            crate::state::LeaderWait::Asked,
            "the mark would not outlive the leader's turn"
        );
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
