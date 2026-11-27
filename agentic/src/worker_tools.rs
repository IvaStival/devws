//! Worker-only MCP catalog and managed noninteractive provider turns.
use crate::{
    devws::{CommandDevws, Devws},
    log::Kind,
    runner,
    task::Task,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

pub fn claude_arguments(
    config: &Path,
    prompt: &str,
    model: Option<&str>,
    session: &str,
    resume: bool,
) -> Vec<String> {
    let mut args = vec![
        "--print".into(),
        "--restricted".into(),
        "--tools".into(),
        "".into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        config.to_string_lossy().into_owned(),
        "--setting-sources".into(),
        "".into(),
        "--disable-slash-commands".into(),
        "--no-chrome".into(),
        "--permission-mode".into(),
        "dontAsk".into(),
        "--allowedTools".into(),
        "mcp__devagentics_worker__worker".into(),
        // Every message and tool call as it happens, for the panel; `json`
        // would print one blob when the turn is over.
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    if let Some(model) = model {
        args.extend(["--model".into(), model.into()]);
    }
    args.extend([
        if resume { "--resume" } else { "--session-id" }.into(),
        session.into(),
        prompt.into(),
    ]);
    args
}
/// One managed Codex turn of a read-only reviewer.
pub struct CodexTurn<'a> {
    pub executable: &'a Path,
    pub task: &'a str,
    pub agent: &'a str,
    pub identity: &'a Path,
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    /// The conversation to continue; None starts it.
    pub thread: Option<&'a str>,
    /// A writer edits its snapshot (its working folder); a reviewer reads.
    pub writes: bool,
}

/// A Codex reviewer's turn. Codex cannot turn its own shell off, so this
/// adapter is for read-only reviewers only: the read-only sandbox, no user
/// config, rules or approval prompts, and the worker MCP server as its only
/// configured tool server, its tools pre-approved. Verified against Codex
/// 0.169 (read-only seccomp sandbox, thread resume). Its shell can still read host files; it cannot
/// write them. `resume` takes no sandbox flag, so the sandbox is set as
/// config on every turn.
/// The longest a worker's tool call may take: `check` runs every profile of
/// the order in turn (each with its own limit), and the worker waits for it.
/// The programs' own defaults (OpenCode 5 s, Codex 60 s) give up long before
/// a full test suite ends, and the turn then fails on a check still running.
pub const WORKER_TOOL_TIMEOUT_SECONDS: u64 = 60 * 60;

pub fn codex_arguments(turn: &CodexTurn) -> Vec<String> {
    use crate::agent_args::toml_string;
    let mut args: Vec<String> = vec!["exec".into()];
    if let Some(thread) = turn.thread {
        args.extend(["resume".into(), thread.into()]);
    }
    let server_args = [
        "worker-mcp".to_string(),
        turn.task.into(),
        turn.agent.into(),
        "--identity".into(),
        turn.identity.to_string_lossy().into_owned(),
    ]
    .iter()
    .map(|arg| toml_string(arg))
    .collect::<Vec<_>>()
    .join(", ");
    args.extend([
        "--json".into(),
        "--ignore-user-config".into(),
        "--ignore-rules".into(),
        "--skip-git-repo-check".into(),
        "-c".into(),
        format!(
            "sandbox_mode={}",
            toml_string(if turn.writes {
                "workspace-write"
            } else {
                "read-only"
            })
        ),
        "-c".into(),
        format!("approval_policy={}", toml_string("never")),
        "-c".into(),
        format!(
            "mcp_servers.devagentics_worker.command={}",
            toml_string(&turn.executable.to_string_lossy())
        ),
        "-c".into(),
        format!("mcp_servers.devagentics_worker.args=[{server_args}]"),
        "-c".into(),
        "mcp_servers.devagentics_worker.env_vars=[\"DEVAGENTICS_HOME\",\"XDG_RUNTIME_DIR\",\"TMUX\",\"TMUX_PANE\"]".into(),
        "-c".into(),
        format!("mcp_servers.devagentics_worker.tool_timeout_sec={WORKER_TOOL_TIMEOUT_SECONDS}"),
        // With approval_policy "never" Codex refuses every MCP call that is
        // not pre-approved. The worker server's own tools are: it checks the
        // worker's identity on each call. Nothing else is approved.
        "-c".into(),
        format!(
            "mcp_servers.devagentics_worker.default_tools_approval_mode={}",
            toml_string("approve")
        ),
    ]);
    if turn.writes {
        // Writable is the working folder (its snapshot) alone: Codex adds
        // /tmp and $TMPDIR unless told not to (checked against Codex 0.169:
        // writes in the home and /tmp, and the network, were all refused).
        args.extend([
            "-c".into(),
            "sandbox_workspace_write.network_access=false".into(),
            "-c".into(),
            "sandbox_workspace_write.exclude_tmpdir_env_var=true".into(),
            "-c".into(),
            "sandbox_workspace_write.exclude_slash_tmp=true".into(),
        ]);
    }
    if let Some(model) = turn.model {
        args.extend(["-m".into(), model.into()]);
    }
    if let Some(effort) = turn.effort {
        args.extend([
            "-c".into(),
            format!("model_reasoning_effort={}", toml_string(effort)),
        ]);
    }
    args.push(turn.prompt.into());
    args
}

/// Runs a Codex reviewer's turn in its read-only review snapshot, continuing
/// its conversation; the events still show in its pane.
fn codex_turn(
    task: &Task,
    agent: &str,
    identity: &Path,
    prompt: &str,
    model: Option<&str>,
    effort: Option<&str>,
    raw: &Path,
) -> Result<(std::process::ExitStatus, Option<crate::panel_view::Usage>)> {
    let state = task.state()?;
    let workspace = state
        .runner_workspaces
        .get(agent)
        .context("a Codex worker needs its runner snapshot")?;
    let (snapshot, writes) = (workspace.snapshot.clone(), !workspace.readonly);
    let thread = state.worker_sessions.get(agent).cloned();
    let executable = crate::executable::own()?;
    let task_id = task.config()?.id;
    let mut child = Command::new("codex")
        .args(codex_arguments(&CodexTurn {
            executable: &executable,
            task: &task_id,
            agent,
            identity,
            prompt,
            model,
            effort,
            thread: thread.as_deref(),
            writes,
        }))
        .current_dir(&snapshot)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .context("starting codex")?;
    let mut started = None;
    let output = child.stdout.take().context("codex output")?;
    let usage = show_turn(output, raw, |line| {
        if started.is_none() {
            started = thread_id(line);
        }
        crate::panel_view::codex_events(line)
    })?;
    let status = child.wait()?;
    if let (None, Some(started)) = (thread, started) {
        task.transaction(|tx| {
            tx.state
                .worker_sessions
                .insert(agent.into(), started.clone());
            Ok(())
        })?;
    }
    Ok((status, usage))
}

/// A Claude worker's turn, its events drawn in the pane as they come.
fn claude_turn(
    arguments: Vec<String>,
    directory: &Path,
    raw: &Path,
) -> Result<(std::process::ExitStatus, Option<crate::panel_view::Usage>)> {
    let mut child = Command::new("claude")
        .args(arguments)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .context("starting claude")?;
    let output = child.stdout.take().context("claude output")?;
    let mut stream = crate::panel_view::ClaudeStream::default();
    let usage = show_turn(output, raw, |line| stream.events(line))?;
    Ok((child.wait()?, usage))
}

fn opencode_turn(
    task: &Task,
    agent: &str,
    adapter: &crate::opencode_worker::Adapter,
    prompt: &str,
    model: Option<&str>,
    raw: &Path,
    mut observe: impl FnMut(&crate::panel_view::Event),
) -> Result<(std::process::ExitStatus, Option<crate::panel_view::Usage>)> {
    let devws = CommandDevws::from_env();
    let pane = std::env::var("TMUX_PANE")?;
    // Provider-specific keys prevent a fallback from resuming another provider's ID.
    let session_key = format!("opencode/{agent}");
    let mut session = task.state()?.worker_sessions.get(&session_key).cloned();
    let mut child = adapter
        .command()
        .args(crate::opencode_worker::arguments(
            prompt,
            model,
            session.as_deref(),
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .context("starting OpenCode")?;
    let mut stream = crate::panel_view::OpenCodeStream::default();
    let mut metadata_error = None;
    let mut provider_error = false;
    let output = child.stdout.take().context("OpenCode output")?;
    let usage = show_turn(output, raw, |line| {
        let metadata = (|| -> Result<()> {
            if let Ok(event) = serde_json::from_str::<Value>(line) {
                if let Some(started) = event["sessionID"].as_str() {
                    if let Some(existing) = &session {
                        ensure!(
                            existing == started,
                            "OpenCode resumed a different worker session"
                        );
                    } else {
                        task.transaction(|tx| {
                            tx.state
                                .worker_sessions
                                .insert(session_key.clone(), started.into());
                            Ok(())
                        })?;
                        session = Some(started.into());
                    }
                    devws.set_pane_option(
                        &pane,
                        "@devws_transcript",
                        &format!("{}#{started}", adapter.database().display()),
                    )?;
                }
                if event["type"] == "error" {
                    provider_error = true;
                    devws.set_pane_option(
                        &pane,
                        "@devws_startup_error",
                        &crate::opencode_worker::failure_record(&event).to_string(),
                    )?;
                }
            }
            Ok(())
        })();
        if let Err(error) = metadata {
            metadata_error = Some(error);
        }
        let events = stream.events(line);
        for event in &events {
            observe(event);
        }
        events
    });
    if usage.is_err() {
        child.kill()?;
    }
    let status = child.wait()?;
    let usage = usage?;
    if let Some(error) = metadata_error {
        return Err(error);
    }
    ensure!(
        !provider_error,
        "OpenCode reported a provider error; claimed work requires explicit recovery"
    );
    ensure!(
        session.is_some(),
        "OpenCode returned no worker session; claimed work requires explicit recovery"
    );
    Ok((status, usage))
}

/// Draws a turn's provider output in the pane and keeps each raw line in
/// `raw`, to check what the provider really sent; returns what it used.
fn show_turn(
    output: impl std::io::Read,
    raw: &Path,
    mut events: impl FnMut(&str) -> Vec<crate::panel_view::Event>,
) -> Result<Option<crate::panel_view::Usage>> {
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(raw)?;
    let width = crate::panel_view::width();
    let mut usage = None;
    for line in std::io::BufReader::new(output).lines() {
        let line = line?;
        writeln!(log, "{line}")?;
        for event in events(&line) {
            if let crate::panel_view::Event::TurnDone(used) = event {
                usage = Some(used);
            } else if let Some(text) = crate::panel_view::render(&event, width) {
                println!("{text}");
            }
        }
    }
    Ok(usage)
}

/// The raw provider lines a worker's turns sent: the latest, plus the
/// previous file once it passes `RAW_LIMIT`.
const RAW_LIMIT: u64 = 2 * 1024 * 1024;

/// The account error a failed turn's provider lines show, when the worker
/// did nothing before it (no tool, no command): nothing of the turn ran, so
/// it can start again elsewhere.
fn account_refusal(
    lines: &str,
    adapter: crate::providers::ReviewedAdapter,
) -> Option<crate::worker::StartupFailure> {
    use crate::panel_view::Event;
    use crate::providers::ReviewedAdapter;
    use crate::worker::FailureScope;
    let mut claude = crate::panel_view::ClaudeStream::default();
    let mut opencode = crate::panel_view::OpenCodeStream::default();
    let mut refusal = None;
    for line in lines.lines() {
        let events = match adapter {
            ReviewedAdapter::Codex => crate::panel_view::codex_events(line),
            ReviewedAdapter::Claude => claude.events(line),
            ReviewedAdapter::Opencode => opencode.events(line),
        };
        if events
            .iter()
            .any(|event| matches!(event, Event::Tool { .. } | Event::Command { .. }))
        {
            return None;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // OpenCode names its errors; the code is in its HTTP status.
        let record = if adapter == ReviewedAdapter::Opencode && record["type"] == "error" {
            crate::opencode_worker::failure_record(&record)
        } else {
            record
        };
        if let Some(failure) = crate::worker::failure_from_record(&record)
            && failure.scope == FailureScope::Account
        {
            refusal.get_or_insert(failure);
        }
    }
    refusal
}

/// The worker's account refused it before it did anything: the account is
/// held back from new agents for a while, the claimed turn is dropped (none
/// of it ran), and the dispatcher restarts the worker on its next account
/// once this panel goes. The leader is told once.
fn hold_and_restart(
    task: &Task,
    agent: &str,
    failure: &crate::worker::StartupFailure,
) -> Result<()> {
    task.transaction(|tx| {
        let account = tx.state.agent(agent).context("unknown worker")?.agent.clone();
        let until = crate::log::now() + crate::accounts::HOLD_SECONDS;
        tx.state.account_holds.insert(
            account.clone(),
            crate::state::AccountHold { until, reason: failure.reason.clone() },
        );
        tx.state.worker_turns.remove(agent);
        tx.state.restart_on_next_account.insert(agent.to_string());
        tx.state.say(
            crate::state::LEADER,
            format!(
                "{agent} was refused by {account} before it did anything ({}). devAgentics holds {account} back from new agents for {} minutes and restarts {agent} on its next account with the same work: nothing to do unless it says it could not.",
                failure.reason,
                crate::accounts::HOLD_SECONDS / 60
            ),
        );
        tx.log(
            Kind::Lost,
            Some(agent),
            None,
            &format!("refused by {account} ({}): restarting on the next account", failure.reason),
        );
        Ok(())
    })
}

/// What a turn added to the raw file, from `start`.
fn turn_lines(raw: &Path, start: u64) -> String {
    use std::io::{Read, Seek};
    let mut text = String::new();
    if let Ok(mut file) = std::fs::File::open(raw)
        && file.seek(std::io::SeekFrom::Start(start)).is_ok()
    {
        let _ = file.read_to_string(&mut text);
    }
    text
}

fn raw_turns(directory: &Path) -> Result<std::path::PathBuf> {
    let raw = directory.join("turns.jsonl");
    if std::fs::metadata(&raw).is_ok_and(|metadata| metadata.len() > RAW_LIMIT) {
        std::fs::rename(&raw, directory.join("turns.old.jsonl"))?;
    }
    Ok(raw)
}

/// The conversation id in a line of `codex exec --json` output.
pub fn thread_id(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line)
        .ok()?
        .get("thread_id")?
        .as_str()
        .map(str::to_string)
}

/// A reviewed leader on Codex, for a team the user put on Codex: Codex's
/// interactive pane in the read-only sandbox, without approval prompts, the
/// leader catalog its only pre-approved tools. A leader never writes; its
/// shell can still read the host, which the user accepted for the task.
pub fn codex_leader_arguments(
    command: &str,
    task: &str,
    first_message: &str,
    model: Option<&str>,
    effort: Option<&str>,
    leader_cli: bool,
) -> Vec<String> {
    use crate::agent_args::toml_string;
    let mut args: Vec<String> = vec!["-c".into(), "tui.animations=false".into()];
    if let Some(model) = model {
        args.extend(["-m".into(), model.into()]);
    }
    if let Some(effort) = effort {
        args.extend([
            "-c".into(),
            format!("model_reasoning_effort={}", toml_string(effort)),
        ]);
    }
    args.extend([
        "-s".into(),
        "read-only".into(),
        "-a".into(),
        "never".into(),
        "-c".into(),
        format!("mcp_servers.devagentics.command={}", toml_string(command)),
        "-c".into(),
        format!(
            "mcp_servers.devagentics.args=[{}, {}{}]",
            toml_string("mcp"),
            toml_string(task),
            if leader_cli { ", \"--leader-cli-catalog\"" } else { "" }
        ),
        "-c".into(),
        "mcp_servers.devagentics.env_vars=[\"DEVAGENTICS_HOME\",\"XDG_RUNTIME_DIR\",\"TMUX\",\"TMUX_PANE\"]".into(),
        "-c".into(),
        format!(
            "mcp_servers.devagentics.default_tools_approval_mode={}",
            toml_string("approve")
        ),
        first_message.into(),
    ]);
    args
}

/// Reviewed coordinators use only the leader catalog; no unmediated host shell.
/// `cli`, when the user turned leader reads on, is the one command its shell
/// may run (`<devagentics> leader-read <task>`): Bash comes back for that
/// prefix only, and `dontAsk` refuses every other command without a prompt.
pub fn leader_arguments(
    config: &Path,
    prompt: &str,
    model: Option<&str>,
    effort: Option<&str>,
    cli: Option<&str>,
) -> Vec<String> {
    let allowed = match cli {
        Some(prefix) => format!("mcp__devagentics,Bash({prefix}:*)"),
        None => "mcp__devagentics".into(),
    };
    let mut arguments = vec![
        "--restricted".into(),
        "--tools".into(),
        if cli.is_some() { "Bash" } else { "" }.into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        config.to_string_lossy().into_owned(),
        "--setting-sources".into(),
        "".into(),
        "--disable-slash-commands".into(),
        "--no-chrome".into(),
        "--permission-mode".into(),
        "dontAsk".into(),
        "--allowedTools".into(),
        allowed,
    ];
    if let Some(model) = model {
        arguments.extend(["--model".into(), model.into()]);
    }
    if let Some(effort) = effort {
        arguments.extend(["--effort".into(), effort.into()]);
    }
    arguments.push(prompt.into());
    arguments
}

pub fn mcp(task: &Task, agent: &str, identity: &Path) -> Result<()> {
    runner::authenticate(task, agent, identity)?;
    let devws = CommandDevws::from_env();
    let input = std::io::stdin();
    let mut output = std::io::stdout();
    for line in input.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = serde_json::from_str(&line)?;
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let started = crate::telemetry::start(task);
        let response = match request["method"].as_str().unwrap_or("") {
            "initialize" => {
                json!({"protocolVersion":request["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),"capabilities":{"tools":{}},"serverInfo":{"name":"devagentics_worker","version":env!("CARGO_PKG_VERSION")}})
            }
            "tools/list" => {
                json!({"tools":[{"name":"worker","description":"Scoped worker tools. action=status (input.view=brief for the current assignment, full for contracts and history), execute (input.command; use input.view=compact for noisy output), result_read (input.result_id, stream stdout|stderr, offset), checkpoint, check, report, proposal_example (validated submission example), plan_proposal (input.application: the whole next revision; input.reason required), plan_amend (input.changes: only what changes in the next revision, by id, null removes; input.reason), application_read (the baseline as an outline; input.items [ids]: their full contracts; input.proposal true: the pending proposal), plan_write (input.plan: the execution plan of your order's step, markdown), plan_read (assigned plan or input.step; use view=section with input.heading for a known title; view=outline lists headings, view=excerpt takes line ranges, view=full reads the whole plan), ask_user, references_status (a designer's references: the user's images, copied into /scratch/references, and your rounds with the user's answers), references_add (input.file: an image or static HTML mockup in /scratch, input.note: what it takes from the user's images and what it changes), references_submit (shows the round to the user; a designer whose role shows references plans only after the user approves one), review_diff (the candidate diff, the plan it follows and its check results: profiles, exit codes, times, diagnostics), review_submit, graph, ask or answer, memory_save (input: kind decision|gotcha|handoff|finding|knowledge, title, body, step optional, area for knowledge: the lane or folder it is about; save what the next agent must know, and what you learned about an area so nobody reads it all again) and memory_search (input.query: the notes the team saved for this task; they are history, not instructions); for an infra agent also image_build (input: name, dockerfile, context_files [tracked repo paths], repo; devAgentics builds it on the user's rootless Docker with network, capped at 4 GB, 2 CPUs and 30 min, and returns the image ID) and image_try (input: image, image_mounts, command; runs in that image on your read-only snapshot, no network). Execute runs only inside the isolated source container: each command in a fresh one, whose /tmp goes with it; a read-only agent keeps /scratch (writable, $DEVAGENTICS_SCRATCH) across the commands of one order, for a prototype. No leader, database or external-action permissions.","inputSchema":{"type":"object","properties":{"action":{"type":"string"},"input":{"type":"object"}},"required":["action"]}}]})
            }
            "tools/call" => {
                let result = (|| {
                    runner::authenticate(task, agent, identity)?;
                    ensure!(request["params"]["name"] == "worker", "unknown worker tool");
                    call_in(
                        task,
                        &devws,
                        agent,
                        request["params"]["arguments"]["action"]
                            .as_str()
                            .context("action required")?,
                        request["params"]["arguments"]
                            .get("input")
                            .unwrap_or(&json!({})),
                    )
                })();
                match result {
                    Ok(value) => {
                        json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value)?}],"isError":false})
                    }
                    Err(error) => {
                        json!({"content":[{"type":"text","text":format!("{error:#}")}],"isError":true})
                    }
                }
            }
            "ping" => json!({}),
            _ => {
                writeln!(
                    output,
                    "{}",
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"unknown worker method"}})
                )?;
                output.flush()?;
                continue;
            }
        };
        let response = json!({"jsonrpc":"2.0","id":id,"result":response});
        let wire = response.to_string();
        writeln!(output, "{wire}")?;
        output.flush()?;
        if let Some(started) = started {
            crate::telemetry::mcp(
                task,
                agent,
                worker_operation(&request),
                line.len(),
                &wire,
                &response,
                started,
            );
        }
    }
    Ok(())
}

fn worker_operation(request: &Value) -> &str {
    match request["method"].as_str() {
        Some("tools/call") if request["params"]["name"] == "worker" => {
            match request["params"]["arguments"]["action"].as_str() {
                Some("execute") if request["params"]["arguments"]["input"]["view"] == "compact" => {
                    "execute.compact"
                }
                Some("status") if request["params"]["arguments"]["input"]["view"] == "brief" => {
                    "status.brief"
                }
                Some("plan_read")
                    if request["params"]["arguments"]["input"]["view"] == "outline" =>
                {
                    "plan_read.outline"
                }
                Some("plan_read")
                    if request["params"]["arguments"]["input"]["view"] == "excerpt" =>
                {
                    "plan_read.excerpt"
                }
                Some("plan_read")
                    if request["params"]["arguments"]["input"]["view"] == "section" =>
                {
                    "plan_read.section"
                }
                Some(
                    action @ ("status" | "execute" | "result_read" | "checkpoint" | "check"
                    | "report" | "proposal_example" | "plan_proposal" | "plan_amend"
                    | "application_read" | "plan_write" | "plan_read" | "ask_user"
                    | "references_status" | "references_add" | "references_submit"
                    | "review_diff" | "review_submit" | "graph" | "ask" | "answer"
                    | "memory_save" | "memory_search" | "image_build" | "image_try"),
                ) => action,
                _ => "unknown_worker_action",
            }
        }
        Some("tools/call") => "unknown_tool",
        Some(method @ ("tools/list" | "initialize" | "ping")) => method,
        _ => "unknown_method",
    }
}
/// What a review looks at: everything the step brings to the branch it
/// merges into, not only the last order. A step can take several orders,
/// and the work committed under an earlier one is the base of the next, so
/// `base..source` of the last one can be empty.
fn review_range(review: &crate::reviews::Review) -> String {
    format!("{}...{}", review.target, review.source)
}

/// The accepted contracts of the steps a reviewer was asked to review: its
/// own role has none, and it reviews against them.
fn reviewed_contracts(
    state: &crate::state::State,
    reviewer: &str,
) -> std::collections::BTreeMap<String, crate::execution::Contract> {
    state
        .reviews
        .iter()
        .filter(|review| review.reviewer == reviewer && review.verdict.is_none())
        .filter_map(|review| state.orders.iter().find(|order| order.id == review.order))
        .filter_map(|order| {
            state
                .application
                .items
                .get(&order.step)
                .map(|item| (order.step.clone(), item.clone()))
        })
        .collect()
}

/// The role that builds the team's runner images.
const INFRA_ROLE: &str = "infra";

fn field<'a>(input: &'a Value, name: &str) -> Result<&'a str> {
    input[name]
        .as_str()
        .with_context(|| format!("{name} is required"))
}
/// Queue a response check without opening an execution item or binding a runner.
pub fn probe(task: &Task, devws: &dyn Devws, agent: &str) -> Result<u64> {
    let _control = task.control_lock()?;
    let state = task.state()?;
    ensure!(
        state.workflow == crate::execution::Workflow::Reviewed,
        "worker probes require a reviewed task"
    );
    ensure!(!state.paused, "resume the task before probing a worker");
    let worker = state.agent(agent).context("unknown worker")?;
    ensure!(
        worker.live
            && worker.ready
            && !worker.lost
            && !worker.unavailable
            && !worker.pane.is_empty(),
        "{agent} is not ready"
    );
    ensure!(
        devws
            .agent_kind(&worker.agent)?
            .provider()
            .reviewed_worker()
            == Some(crate::providers::ReviewedAdapter::Opencode),
        "{agent} is not running managed OpenCode"
    );
    ensure!(
        state.open_order_of(agent).is_none()
            && crate::reviews::resumption(&state, agent).is_none()
            && !state.worker_turns.contains_key(agent)
            && !state.queue.iter().any(|message| message.to == agent),
        "{agent} already has work"
    );
    task.transaction(|tx| Ok(tx.state.probe(agent)))
}

/// The worker, when it may use its tools now.
fn usable_worker<'a>(
    state: &'a crate::state::State,
    agent: &str,
) -> Result<&'a crate::state::Agent> {
    let worker = state.agent(agent).context("unknown worker")?;
    ensure!(worker.live, "worker stopped");
    ensure!(
        !state
            .worker_turns
            .get(agent)
            .is_some_and(|message| message.response_probe),
        "worker tools are disabled during a response probe"
    );
    Ok(worker)
}

/// A worker's tool call: the memory actions need devws (the team's memory),
/// the rest do not.
pub fn call_in(
    task: &Task,
    devws: &dyn Devws,
    agent: &str,
    action: &str,
    input: &Value,
) -> Result<Value> {
    match action {
        "memory_save" | "memory_search" => memory_action(task, devws, agent, action, input),
        // Every report and verdict is kept for the agents after it.
        "report" => {
            let order = task.state()?.open_order_of(agent).cloned();
            let result = call(task, agent, action, input)?;
            if let Some(order) = order {
                let outcome = input["outcome"].as_str().unwrap_or("reported");
                let summary = input["summary"].as_str().unwrap_or_default();
                crate::notes::record_report(task, devws, agent, &order, outcome, summary);
            }
            Ok(result)
        }
        "review_submit" => {
            let state = task.state()?;
            let order = state
                .reviews
                .iter()
                .find(|review| review.id == input["review"])
                .and_then(|review| state.orders.iter().find(|order| order.id == review.order))
                .cloned();
            let result = call(task, agent, action, input)?;
            if let Some(order) = order {
                let verdict = input["verdict"].as_str().unwrap_or("reviewed");
                let summary = input["summary"].as_str().unwrap_or_default();
                crate::notes::record_report(
                    task,
                    devws,
                    agent,
                    &order,
                    &format!("review {verdict}"),
                    summary,
                );
            }
            Ok(result)
        }
        _ => call(task, agent, action, input),
    }
}

/// A worker saves a note for the agents after it (about its order's step
/// unless it names one), or searches the task's notes.
fn memory_action(
    task: &Task,
    devws: &dyn Devws,
    agent: &str,
    action: &str,
    input: &Value,
) -> Result<Value> {
    let state = task.state()?;
    usable_worker(&state, agent)?;
    if action == "memory_search" {
        let hits = crate::notes::search(task, devws, field(input, "query")?)?;
        return Ok(json!({ "memory_on": devws.memory().is_on(), "notes": hits
            .iter()
            .map(|page| json!({"path": page.path, "title": page.title, "text": page.body}))
            .collect::<Vec<_>>() }));
    }
    let step = input["step"]
        .as_str()
        .map(str::to_string)
        .or_else(|| state.open_order_of(agent).map(|order| order.step.clone()));
    let note = crate::memory::Note {
        area: input["area"].as_str().map(str::to_string),
        kind: crate::notes::agent_kind(field(input, "kind")?)?,
        title: field(input, "title")?.to_string(),
        body: field(input, "body")?.to_string(),
        author: agent.to_string(),
        step,
    };
    let path = crate::notes::save(task, devws, &note)?;
    Ok(json!({ "saved": path.is_some(), "path": path, "memory_on": devws.memory().is_on() }))
}

fn brief_status(state: &crate::state::State, worker: &crate::state::Agent) -> Value {
    let id = &worker.id;
    let order = state.open_order_of(id).map(|order| {
        json!({
            "id": order.id, "step": order.step, "plan": order.plan,
            "revision": state.revision(&order.id),
            "allowed": order.allowed, "forbidden": order.forbidden,
            "read": order.read, "targets": order.targets,
            "profiles": state.order_profiles.get(&order.id),
        })
    });
    let review = crate::reviews::pending(state, id).map(|review| {
        let reviewed_order = state.orders.iter().find(|order| order.id == review.order);
        json!({
            "id": review.id,
            "order": review.order,
            "step": reviewed_order.map(|order| order.step.as_str()),
            "plan": reviewed_order.and_then(|order| order.plan.as_deref()),
        })
    });
    let questions: Vec<_> = state.questions.iter().filter(|question| {
        question.answer.is_none() && (&question.to == id || &question.from == id)
    }).map(|question| json!({
        "id": question.id, "from": question.from, "to": question.to, "order": question.order,
    })).collect();
    json!({
        "agent": id, "role": worker.role, "repo": worker.repo, "lane": worker.lane,
        "order": order, "review": review, "questions": questions,
        "paused": state.paused,
        "application_revision": state.application.revision,
        "proposal_revision": if worker.writes { None } else { state.application_proposal.as_ref().map(|proposal| proposal.revision) },
    })
}

/// The item a worker's open order is on, when it is a baseline item.
fn assigned_item<'a>(
    state: &'a crate::state::State,
    worker: &crate::state::Agent,
) -> Option<(&'a String, &'a crate::execution::Contract)> {
    let order = state.open_order_of(&worker.id)?;
    state.application.items.get_key_value(&order.step)
}

/// The contracts a worker sees: only its order's item while it has one, so a
/// large baseline does not ride along on every status; otherwise every item
/// of its role and repo, as what may come next.
fn assigned_contracts(
    state: &crate::state::State,
    worker: &crate::state::Agent,
) -> std::collections::BTreeMap<String, crate::execution::Contract> {
    if let Some((id, item)) = assigned_item(state, worker) {
        return std::collections::BTreeMap::from([(id.clone(), item.clone())]);
    }
    state
        .application
        .items
        .iter()
        .filter(|(_, item)| item.role == worker.role && item.repo == worker.repo)
        .map(|(id, item)| (id.clone(), item.clone()))
        .collect()
}

/// The specs of the interfaces the worker's item provides or consumes.
fn assigned_interfaces(
    state: &crate::state::State,
    worker: &crate::state::Agent,
) -> std::collections::BTreeMap<String, crate::execution::Interface> {
    let Some((_, item)) = assigned_item(state, worker) else {
        return Default::default();
    };
    item.provides
        .iter()
        .chain(&item.consumes)
        .filter_map(|id| {
            let interface = state.application.interfaces.get(id)?;
            Some((id.clone(), interface.clone()))
        })
        .collect()
}

pub fn call(task: &Task, agent: &str, action: &str, input: &Value) -> Result<Value> {
    let state = task.state()?;
    let worker = usable_worker(&state, agent)?;
    let pane = &worker.pane;
    match action {
        "status" => match input.get("view").map(Value::as_str) {
            Some(Some("brief")) => Ok(brief_status(&state, worker)),
            None | Some(Some("full")) => Ok(
                json!({"agent":agent,"role":worker.role,"order":state.open_order_of(agent),"application_revision":state.application.revision,"planning_proposal":if !worker.writes{state.application_proposal.as_ref().map(crate::execution::outline)}else{None},"contracts":assigned_contracts(&state,worker),"interfaces":assigned_interfaces(&state,worker),"questions":state.questions.iter().filter(|question|question.to==agent||question.from==agent).collect::<Vec<_>>(),"reviews":state.reviews.iter().filter(|review|review.reviewer==agent).collect::<Vec<_>>(),"reviewed_contracts":reviewed_contracts(&state,agent)}),
            ),
            _ => bail!("status view must be brief or full"),
        },
        "proposal_example" => {
            ensure!(
                !worker.writes && worker.role != "code-reviewer",
                "only a planning specialist requests a proposal example"
            );
            crate::execution::proposal_example()
        }
        "plan_proposal" | "plan_amend" => {
            ensure!(
                !worker.writes && worker.role != "code-reviewer",
                "only a planning specialist proposes a baseline"
            );
            let affected = if action == "plan_amend" {
                let changes = serde_json::from_value(input["changes"].clone())?;
                crate::execution::amend(task, changes, field(input, "reason")?)?
            } else {
                crate::execution::store_input(task, input)?
            };
            task.transaction(|tx|{if let Some(order)=tx.state.open_order_of(agent).cloned(){if let Some(step)=tx.state.step_mut(&order.step){step.set_status(crate::state::StepStatus::Reported,crate::log::now());}
                if let Some(order)=tx.state.orders.iter_mut().find(|assignment|assignment.id==order.id){order.status=crate::state::OrderStatus::Closed;}}tx.state.say(crate::state::LEADER,format!("Planner {agent} proposed an application baseline. Inspect application_workflow status, interview the user and obtain explicit confirmation and approval before implementation."));Ok(())})?;
            Ok(json!({"proposed":true,"affected":affected}))
        }
        "application_read" => {
            ensure!(
                !worker.writes,
                "only a read-only specialist reads the baseline"
            );
            let application = if input["proposal"].as_bool() == Some(true) {
                state
                    .application_proposal
                    .as_ref()
                    .context("no baseline proposal awaiting decision")?
            } else {
                &state.application
            };
            match input.get("items") {
                Some(ids) => crate::execution::items(
                    application,
                    &serde_json::from_value::<Vec<String>>(ids.clone())?,
                ),
                None => Ok(crate::execution::outline(application)),
            }
        }
        "plan_read" => {
            // The plan its order, or the order it reviews, follows, unless it
            // names a step: a step planned elsewhere gives that plan.
            let step = match input["step"].as_str() {
                Some(step) if crate::plans::read(task, step).is_err() => {
                    plan_of_step(&state, step).unwrap_or_else(|| step.to_string())
                }
                Some(step) => step.to_string(),
                None => followed_plan(&state, agent)
                    .context("your order follows no plan: name the step")?,
            };
            crate::plans::worker_view(&step, &crate::plans::read(task, &step)?, input)
        }
        "plan_write" => {
            ensure!(
                !worker.writes && worker.role != "code-reviewer",
                "only a planning specialist writes an execution plan"
            );
            let step = crate::plans::store_execution(task, agent, field(input, "plan")?)?;
            Ok(
                json!({"stored":true,"step":step,"next":"report the order done; the leader reads it with read_plan"}),
            )
        }
        "references_status" => crate::references::worker_status(task, agent),
        "references_add" => {
            let file = crate::references::scratch_file(task, agent, field(input, "file")?)?;
            let id = crate::references::add(task, agent, &file, field(input, "note")?)?;
            Ok(json!({"added":id,"next":"add your other directions, then references_submit"}))
        }
        "references_submit" => {
            let (step, count) = crate::references::submit(task, agent)?;
            Ok(
                json!({"submitted":count,"step":step,"next":"end your turn now: the user's answer is typed into you"}),
            )
        }
        "ask_user" => {
            ensure!(
                !worker.writes && worker.role != "code-reviewer",
                "only planning specialists request product decisions"
            );
            let question = field(input, "question")?;
            let recommendation = field(input, "recommendation")?;
            task.transaction(|tx|{tx.state.say(crate::state::LEADER,format!("Planner {agent} asks: {question}\nRecommended: {recommendation}\nRelay this product decision to the user; this is not action authorization."));Ok(())})?;
            Ok(json!({"queued_for_user":true}))
        }
        "execute" => {
            let command = field(input, "command")?;
            match input.get("view").map(Value::as_str) {
                Some(Some("compact")) => runner::execute_compact(task, agent, command),
                None | Some(Some("full")) => Ok(serde_json::to_value(runner::execute(
                    task, agent, command,
                )?)?),
                _ => bail!("execute view must be compact or full"),
            }
        }
        "result_read" => {
            let stream = input
                .get("stream")
                .map(|_| field(input, "stream"))
                .transpose()?
                .unwrap_or("stdout");
            let offset = input
                .get("offset")
                .map(|value| {
                    value
                        .as_u64()
                        .context("offset must be a non-negative integer")
                })
                .transpose()?
                .unwrap_or_default();
            runner::read_result(task, agent, field(input, "result_id")?, stream, offset)
        }
        "checkpoint" => {
            Ok(json!({"changed":runner::checkpoint(task,agent,field(input,"summary")?)?}))
        }
        "check" => {
            ensure!(
                worker.writes,
                "reviewer/planner cannot run implementation checks"
            );
            let order = state.open_order_of(agent).context("no assignment")?;
            Ok(crate::check_profiles::run(task, &order.id, Some(pane))?.response())
        }
        "report" => {
            ensure!(
                worker.role != "code-reviewer",
                "a reviewer answers with review_submit"
            );
            let order = state.open_order_of(agent).context("no assignment")?;
            let outcome = match field(input, "outcome")? {
                "done" => crate::orders::Outcome::Done,
                "blocked" => crate::orders::Outcome::Blocked,
                _ => bail!("outcome must be done or blocked"),
            };
            if !worker.writes {
                return Ok(
                    json!({"result":crate::orders::report_findings(task,agent,&order.id,outcome,field(input,"summary")?)?}),
                );
            }
            Ok(
                json!({"result":crate::orders::report(task,pane,&order.id,outcome,field(input,"summary")?,crate::orders::check_timeout())?}),
            )
        }
        "ask" => Ok(json!({"result":crate::questions::ask(task,pane,field(input,"question")?)?})),
        "answer" => Ok(
            json!({"result":crate::questions::answer(task,crate::questions::Answerer::Pane(pane),field(input,"question")?,field(input,"answer")?)?}),
        ),
        "review_diff" => {
            let review = state
                .reviews
                .iter()
                .find(|review| review.id == input["review"] && review.reviewer == agent)
                .context("foreign review")?;
            let author = state.agent(&review.author).context("unknown author")?;
            let repo = crate::lanes::workplace(task, author)?;
            Ok(
                json!({"review":review,"diff":crate::checks::git(&repo.worktree,&["diff","--no-ext-diff","--no-textconv",&review_range(review)])?,"plan":state.orders.iter().find(|order|order.id==review.order).and_then(|order|order.plan.clone()),"checks":crate::reviews::check_results(&state,review)}),
            )
        }
        "graph" => {
            let (repo, commit) = graph_source(task, &state, agent)?;
            Ok(json!({"answer": crate::graph::answer(
                &crate::graph::Graphify::from_env()?,
                task,
                &repo,
                &commit,
                input,
            )?}))
        }
        "image_build" | "image_try" => {
            ensure!(
                worker.role == INFRA_ROLE,
                "only an infra agent builds and tries runner images"
            );
            if action == "image_build" {
                let request = serde_json::from_value(input.clone())?;
                return Ok(serde_json::to_value(crate::images::build(task, &request)?)?);
            }
            let request = serde_json::from_value(input.clone())?;
            Ok(serde_json::to_value(crate::images::try_image(
                task, agent, &request,
            )?)?)
        }
        "review_submit" => {
            let verdict = serde_json::from_value(input["verdict"].clone())?;
            let findings = serde_json::from_value(input["findings"].clone())?;
            crate::reviews::submit(
                task,
                pane,
                field(input, "review")?,
                verdict,
                findings,
                field(input, "summary")?,
            )?;
            Ok(json!({"recorded":true}))
        }
        _ => bail!("action is not in the worker capability catalog"),
    }
}
/// The plan an agent works by: its open order's, or the reviewed order's.
fn followed_plan(state: &crate::state::State, agent: &str) -> Option<String> {
    let order = state.open_order_of(agent).or_else(|| {
        let review = crate::reviews::pending(state, agent)?;
        state.orders.iter().find(|order| order.id == review.order)
    })?;
    order.plan.clone()
}

/// The plan the latest order for `step` followed (B1-1's is B1-1-plan).
fn plan_of_step(state: &crate::state::State, step: &str) -> Option<String> {
    state
        .orders
        .iter()
        .rev()
        .find(|order| order.step == step)
        .and_then(|order| order.plan.clone())
}

/// The code an agent's graph questions are about: a reviewer's pending
/// review candidate (the exact merge it judges), else the agent's own lane
/// or repo at its last commit.
fn graph_source(
    task: &Task,
    state: &crate::state::State,
    agent: &str,
) -> Result<(crate::task::Repo, String)> {
    if let Some(review) = crate::reviews::pending(state, agent) {
        let author = state.agent(&review.author).context("unknown author")?;
        return Ok((
            crate::lanes::workplace(task, author)?,
            review.candidate.commit.clone(),
        ));
    }
    let worker = state.agent(agent).context("unknown agent")?;
    let repo = crate::lanes::workplace(task, worker)?;
    let head = crate::checks::head(&repo.worktree)?;
    Ok((repo, head))
}

/// The picker reads an agent's model from its pane. An interactive agent's
/// runner sets it; a managed panel has no runner, so it sets its own.
fn show_model(devws: &dyn Devws, pane: &str, model: Option<&str>) -> Result<()> {
    let model = model.unwrap_or("default model");
    devws.set_pane_option(pane, "@devws_model", model)?;
    devws.set_pane_option(pane, "@devws_model_configured", model)
}

fn finish_probe(
    tx: &mut crate::task::Tx,
    agent: &str,
    id: u64,
    model: Option<&str>,
    outcome: &str,
) {
    if tx
        .state
        .worker_turns
        .get(agent)
        .is_some_and(|message| message.id == id)
    {
        tx.state.worker_turns.remove(agent);
        let model = model.unwrap_or("default model");
        let notice = format!("OpenCode response probe {id} for {agent} ({model}) {outcome}");
        tx.log(Kind::Delivered, Some(agent), None, &notice);
        tx.state.say(crate::state::LEADER, notice);
    }
}

/// The error a managed worker's last turn ended with, as its program said
/// it (Claude's `result` of an error, Codex's `error` message): why a panel
/// that closed at once went. None when its last turn did not fail.
pub fn last_error(task: &Task, agent: &str) -> Option<String> {
    let raw = task
        .dir()
        .join("workers")
        .join(agent)
        .join("control")
        .join("turns.jsonl");
    let text = std::fs::read_to_string(raw).ok()?;
    let last: Value =
        serde_json::from_str(text.lines().rev().find(|line| !line.trim().is_empty())?).ok()?;
    let message = if last["is_error"] == true {
        last["result"].as_str()
    } else if last["type"] == "error" {
        last["message"].as_str()
    } else {
        None
    }?;
    Some(message.chars().take(300).collect())
}

/// The panel remains a supervisor, never an interactive editable model prompt.
/// A resumed session already holds the role text, so later turns get a short
/// re-anchor. The full text goes again every this many turns, because a long
/// conversation the provider compacted can drop it.
const FULL_ROLE_EVERY: u32 = 10;

/// What a panel sent its session so far. A new panel (after a restart, an
/// account switch or a failed turn) starts empty and sends the full role.
#[derive(Default)]
struct RoleAnchor {
    turns_since_full: Option<u32>,
}

impl RoleAnchor {
    fn send_full(&self, session_continues: bool) -> bool {
        !session_continues
            || self
                .turns_since_full
                .is_none_or(|turns| turns + 1 >= FULL_ROLE_EVERY)
    }

    fn sent(&mut self, full: bool) {
        self.turns_since_full = Some(if full {
            0
        } else {
            self.turns_since_full.unwrap_or(0) + 1
        });
    }
}

struct TurnText<'a> {
    role: &'a str,
    standards: &'a str,
    agent: &'a str,
    tools: &'a str,
    graph: &'a str,
    message: &'a str,
}

fn worker_prompt(text: &TurnText, full: bool) -> String {
    let TurnText {
        role,
        standards,
        agent,
        tools,
        graph,
        message,
    } = text;
    let rules = "Execute commands only through the isolated execute action. Checkpoint before check/report. Do not use host paths or attempt external/database actions. Process this message, report or submit review, then finish this turn";
    if !full {
        return format!(
            "You are {agent}. Your role instructions earlier in this conversation still apply. {tools} Call status with input.view=brief first. {rules}:\n{message}"
        );
    }
    // Provisioned roles already carry the shared standards; custom ones may not.
    let standards = if role.contains(standards.trim()) {
        ""
    } else {
        standards
    };
    format!(
        "{role}\n{standards}\nYou are {agent}. {tools}{graph} Call status with input.view=brief first; read full status when you need contract details or history. {rules}:\n{message}"
    )
}

/// Waits on the state as it was at `stamp`: the parsed copy just dropped
/// goes back to the system, and the next pass reads the state again only
/// once it changes.
fn wait_on(
    waited_on: &mut Option<(std::time::SystemTime, u64)>,
    stamp: Option<(std::time::SystemTime, u64)>,
) {
    *waited_on = stamp;
    // The allocator keeps a freed big state for the process otherwise.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::malloc_trim(0);
    }
    std::thread::sleep(Duration::from_millis(250));
}

pub fn panel(task: &Task, agent: &str, identity: &Path, model: Option<&str>) -> Result<()> {
    let devws = CommandDevws::from_env();
    let pane = std::env::var("TMUX_PANE").context("managed worker requires a pane")?;
    show_model(&devws, &pane, model)?;
    let directory = task.dir().join("workers").join(agent).join("control");
    std::fs::create_dir_all(&directory)?;
    let program_name = crate::providers::Kind::name;
    if let Some(worker) = task.state()?.agent(agent) {
        println!(
            "{} · {} · {} · waiting for its first order",
            agent,
            program_name(devws.agent_kind(&worker.agent)?),
            model.unwrap_or("default model")
        );
    }
    let executable = crate::executable::own()?;
    let config = directory.join("mcp.json");
    std::fs::write(
        &config,
        serde_json::to_vec_pretty(
            &json!({"mcpServers":{"devagentics_worker":{"command":executable,"args":["worker-mcp",task.config()?.id,agent,"--identity",identity]}}}),
        )?,
    )?;
    let secret = std::fs::read_to_string(identity)?;
    let session = format!(
        "{}-{}-4{}-a{}-{}",
        &secret[..8],
        &secret[8..12],
        &secret[13..16],
        &secret[17..20],
        &secret[20..32]
    );
    let mut resumed = false;
    let mut anchor = RoleAnchor::default();
    let program = task
        .state()?
        .agent(agent)
        .context("unknown managed worker")?
        .agent
        .clone();
    use crate::providers::ReviewedAdapter;
    let opencode = if devws.agent_kind(&program)?.provider().reviewed_worker()
        == Some(ReviewedAdapter::Opencode)
    {
        // A reused pane must not attribute its previous runtime to this worker.
        devws.set_pane_option(&pane, "@devws_startup_error", "")?;
        devws.set_pane_option(&pane, "@devws_transcript", "")?;
        let adapter = crate::opencode_worker::Adapter::prepare(
            &directory,
            crate::opencode_worker::configuration(
                &executable,
                &task.config()?.id,
                agent,
                identity,
                task.dir()
                    .parent()
                    .and_then(Path::parent)
                    .context("task home missing")?,
            ),
        )?;
        if let Err(error) = adapter.verify() {
            devws.set_pane_option(
                &pane,
                "@devws_startup_error",
                &json!({"type":"error","error":{"code":"adapter_unavailable"}}).to_string(),
            )?;
            return Err(error);
        }
        Some(adapter)
    } else {
        None
    };
    // The state a wait was decided on: while it is the same, waiting goes
    // on without reading it again. A panel that parsed a big state four times
    // a second used a CPU core and hundreds of MB for nothing.
    let mut waited_on = None;
    loop {
        let stamp = task.state_stamp()?;
        if waited_on.is_some() && waited_on == stamp {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        let state = task.state()?;
        let Some(worker) = state.agent(agent) else {
            break;
        };
        if !worker.live {
            break;
        }
        let program = devws.agent_kind(&worker.agent)?;
        if worker.pane != pane {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        runner::authenticate(task, agent, identity)?;
        if state.paused {
            drop(state);
            wait_on(&mut waited_on, stamp);
            continue;
        }
        if !worker.ready {
            task.transaction(|tx| {
                if let Some(worker) = tx.state.agents.iter_mut().find(|worker| worker.id == agent) {
                    worker.ready = true;
                }
                Ok(())
            })?;
            devws.mark(&pane, "done")?;
        }
        let Some(message) = state
            .queue
            .iter()
            .find(|message| message.to == agent)
            .cloned()
        else {
            drop(state);
            wait_on(&mut waited_on, stamp);
            continue;
        };
        waited_on = None;
        let _control = task.control_lock()?;
        let latest = task.state()?;
        if latest.paused || !latest.queue.iter().any(|current| current.id == message.id) {
            continue;
        }
        if let Some(order) = &message.order {
            runner::bind_order(task, agent, order)?;
        }
        task.transaction(|tx| {
            ensure!(
                !tx.state.worker_turns.contains_key(agent),
                "an uncertain previous turn requires explicit recovery"
            );
            let now = crate::log::now();
            if let Some(created) = tx.state.message_created.remove(&message.id) {
                tx.state
                    .timing
                    .record("queue_wait", created, now.max(created))?;
            }
            tx.state.worker_turns.insert(agent.into(), message.clone());
            tx.state.queue.retain(|current| current.id != message.id);
            if let Some(order) = &message.order {
                if let Some(order) = tx
                    .state
                    .orders
                    .iter_mut()
                    .find(|current| current.id == *order)
                {
                    order.delivered = true;
                }
            }
            tx.log(
                Kind::Delivered,
                Some(agent),
                None,
                "queued message claimed by managed worker turn",
            );
            Ok(())
        })?;
        drop(_control);
        devws.mark(&pane, "working")?;
        let role = task
            .state()?
            .worker_launches
            .get(agent)
            .map(|launch| launch.role.instructions.clone())
            .unwrap_or_default();
        let standards = include_str!("../roles/standards/shared.md");
        let adapter = program.provider().reviewed_worker();
        let tools = if adapter == Some(ReviewedAdapter::Codex) {
            "Your own shell runs on the host, outside the isolated runner, and can write only your working directory (your source snapshot), with no network. Read and change only files there, never other host files. Run checks and tests only through devagentics_worker worker tools (check, execute), and checkpoint before you report. Review, report and ask only through those tools."
        } else {
            "Native tools are disabled. Use only devagentics_worker worker tools."
        };
        let graph = if task.state()?.graph_enabled {
            " A code graph of your source is on: worker action=graph with action=explain and symbol (definition, callers, callees; an ambiguous name lists candidates, retry with path::symbol), path with from and to, or query with question. Use it to find what to reuse, the real interfaces and who a change affects, before reading files one by one."
        } else {
            ""
        };
        let expected = message
            .response_probe
            .then(|| format!("DEVWS-PROBE-{}", message.id));
        let turn_state = task.state()?;
        let session_continues = match adapter {
            Some(ReviewedAdapter::Claude) => resumed,
            Some(ReviewedAdapter::Codex) => turn_state.worker_sessions.contains_key(agent),
            Some(ReviewedAdapter::Opencode) => turn_state
                .worker_sessions
                .contains_key(&format!("opencode/{agent}")),
            None => false,
        };
        let full_role = anchor.send_full(session_continues);
        let prompt = if let Some(expected) = &expected {
            format!(
                "This is a connection check. Reply with exactly {expected} and finish. Do not use any tools."
            )
        } else {
            worker_prompt(
                &TurnText {
                    role: &role,
                    standards,
                    agent,
                    tools,
                    graph,
                    message: &message.text,
                },
                full_role,
            )
        };
        let mut reply = String::new();
        let turn_start = crate::log::now();
        let effort = turn_state
            .worker_launches
            .get(agent)
            .and_then(|launch| launch.selected.as_ref())
            .and_then(|program| program.effort.clone());
        let resumed_codex_thread = turn_state.worker_sessions.get(agent).map(String::as_str);
        let order = message.order.as_ref().map(|id| {
            let step = task
                .state()
                .ok()
                .and_then(|state| {
                    state
                        .orders
                        .iter()
                        .find(|order| order.id == *id)
                        .map(|order| order.step.clone())
                })
                .unwrap_or_default();
            format!("{id} ({step})")
        });
        let width = crate::panel_view::width();
        println!(
            "\n{}",
            crate::panel_view::header(
                &crate::panel_view::Turn {
                    agent,
                    program: program_name(program),
                    model,
                    effort: effort.as_deref(),
                    order,
                },
                &chrono::Local::now().format("%H:%M").to_string(),
                width,
            )
        );
        let raw = raw_turns(&directory)?;
        let raw_start = std::fs::metadata(&raw)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let telemetry_started = crate::telemetry::start(task);
        let telemetry_attempt = telemetry_started.map(|_| {
            format!(
                "{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            )
        });
        if let Some(attempt) = &telemetry_attempt {
            crate::telemetry::record(
                task,
                &json!({
                    "version": 1, "kind": "worker_turn_started", "at": turn_start,
                    "agent": agent, "provider": program_name(program), "model": model,
                    "message_id": message.id, "order": message.order, "attempt": attempt,
                    "probe": message.response_probe,
                    "prompt_bytes": prompt.len(),
                    "full_role": expected.is_none().then_some(full_role),
                    "role_bytes": expected.is_none().then_some(if full_role { role.len() } else { 0 }),
                    "standards_bytes": expected.is_none().then_some(
                        if full_role && !role.contains(standards.trim()) { standards.len() } else { 0 },
                    ),
                    "tool_instructions_bytes": expected.is_none().then_some(tools.len()),
                    "graph_instructions_bytes": expected.is_none().then_some(graph.len()),
                    "message_bytes": expected.is_none().then_some(message.text.len()),
                }),
            );
        }
        let result = match adapter {
            Some(ReviewedAdapter::Codex) => codex_turn(
                task,
                agent,
                identity,
                &prompt,
                model,
                effort.as_deref(),
                &raw,
            ),
            Some(ReviewedAdapter::Opencode) => opencode_turn(
                task,
                agent,
                opencode
                    .as_ref()
                    .context("OpenCode adapter was not verified")?,
                &prompt,
                model,
                &raw,
                |event| {
                    if expected.is_some()
                        && reply.len() <= 256
                        && let crate::panel_view::Event::Message(text) = event
                    {
                        reply.extend(text.chars().take(257));
                    }
                },
            ),
            // team.rs's reviewed_adapter never starts one.
            None => Err(anyhow::anyhow!(
                "{} has no reviewed worker adapter",
                program.name()
            )),
            Some(ReviewedAdapter::Claude) => {
                let mut arguments = claude_arguments(&config, &prompt, model, &session, resumed);
                if let Some(effort) = effort {
                    let position = arguments.len() - 1;
                    arguments.splice(position..position, ["--effort".into(), effort]);
                }
                claude_turn(arguments, &directory, &raw)
            }
        };
        let failed = result
            .as_ref()
            .map_or(true, |(status, _)| !status.success());
        if let Some((started, attempt)) = telemetry_started.zip(telemetry_attempt) {
            let stream_usage = adapter.and_then(|provider| {
                match crate::telemetry::worker_usage(
                    &raw,
                    raw_start,
                    provider,
                    resumed_codex_thread,
                ) {
                    Ok(usage) => usage,
                    Err(error) => {
                        eprintln!("devagentics benchmark provider usage: {error}");
                        None
                    }
                }
            });
            crate::telemetry::record(
                task,
                &json!({
                    "version": 2, "kind": "worker_turn_finished", "at": crate::log::now(),
                    "agent": agent, "provider": program_name(program), "model": model,
                    "message_id": message.id, "order": message.order, "attempt": attempt,
                    "failed": failed,
                    "duration_ms": started.elapsed().as_millis(),
                    "stream_usage": stream_usage,
                    "usage_source": if adapter == Some(ReviewedAdapter::Codex) {
                        "codex_thread_delta"
                    } else {
                        "managed_provider_stream"
                    },
                }),
            );
        }
        if failed
            && !message.response_probe
            && let Some(adapter) = adapter
            && let Some(refusal) = account_refusal(&turn_lines(&raw, raw_start), adapter)
        {
            hold_and_restart(task, agent, &refusal)?;
            devws.mark(&pane, "waiting")?;
            // The panel goes, so the dispatcher starts this worker again.
            bail!("{agent}: {}", refusal.reason);
        }
        let (status, usage) = match result {
            Ok(result) => result,
            Err(error) => {
                task.transaction(|tx| {
                    if message.response_probe {
                        finish_probe(tx, agent, message.id, model, &format!("failed: {error:#}"));
                    } else {
                        tx.state.say(crate::state::LEADER, format!("Managed worker {agent} failed. Its claimed message is retained for explicit recovery; do not resend automatically."));
                    }
                    Ok(())
                })?;
                devws.mark(&pane, "waiting")?;
                return Err(error);
            }
        };
        let failure = (!status.success()).then(|| status.to_string());
        println!(
            "{}",
            crate::panel_view::footer(
                crate::log::now().saturating_sub(turn_start),
                usage,
                failure.as_deref(),
                width,
            )
        );
        task.transaction(|tx| {
            tx.state.timing.record("worker_turn",turn_start,crate::log::now().max(turn_start))?;
            if let Some(expected) = &expected {
                let outcome = match &failure {
                    Some(failure) => format!("failed: {failure}"),
                    None if reply.trim() == expected => "verified the expected reply.".into(),
                    None => "finished without the expected reply.".into(),
                };
                finish_probe(tx, agent, message.id, model, &outcome);
            } else if status.success() {
                tx.state.worker_turns.remove(agent);
            } else {
                tx.state.say(crate::state::LEADER, format!("Managed worker {agent} failed. Its claimed message is retained for explicit recovery; do not resend automatically."));
            }
            Ok(())
        })?;
        devws.mark(&pane, if status.success() { "done" } else { "waiting" })?;
        ensure!(
            status.success(),
            "managed provider turn failed; inspect the worker output before recovery"
        );
        if expected.is_none() {
            anchor.sent(full_role);
        }
        resumed = true;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    fn turn<'a>(role: &'a str, standards: &'a str) -> TurnText<'a> {
        TurnText {
            role,
            standards,
            agent: "hammer-3",
            tools: "Use only devagentics_worker worker tools.",
            graph: "",
            message: "fix the slug bug",
        }
    }

    #[test]
    fn a_resumed_session_gets_a_short_reanchor_instead_of_the_role() {
        let role = "R".repeat(3000);
        let full = worker_prompt(&turn(&role, "shared rules"), true);
        let short = worker_prompt(&turn(&role, "shared rules"), false);
        assert!(full.contains(&role) && full.contains("shared rules"));
        assert!(!short.contains(&role) && !short.contains("shared rules"));
        assert!(short.contains("You are hammer-3") && short.contains("fix the slug bug"));
        assert!(short.contains("Use only devagentics_worker worker tools."));
        assert!(
            short.len() * 4 <= full.len(),
            "{} vs {}",
            short.len(),
            full.len()
        );
    }

    #[test]
    fn standards_already_in_the_role_are_not_sent_twice() {
        let standards = include_str!("../roles/standards/shared.md");
        let role = format!("writer template\n{standards}\ndomain");
        let prompt = worker_prompt(&turn(&role, standards), true);
        assert_eq!(prompt.matches(standards.trim()).count(), 1);
        let custom = worker_prompt(&turn("custom role", standards), true);
        assert!(custom.contains(standards.trim()));
    }

    #[test]
    fn the_full_role_goes_first_after_a_new_session_and_every_tenth_turn() {
        let mut anchor = RoleAnchor::default();
        assert!(anchor.send_full(true), "a new panel knows nothing it sent");
        anchor.sent(true);
        assert!(!anchor.send_full(true));
        assert!(anchor.send_full(false), "a lost provider session");
        let mut fulls = Vec::new();
        for turn in 1..=20 {
            let full = anchor.send_full(true);
            if full {
                fulls.push(turn);
            }
            anchor.sent(full);
        }
        assert_eq!(fulls, [10, 20]);
    }

    #[test]
    fn a_working_writer_sees_only_its_items_contract_and_interfaces() {
        let example: Value =
            serde_json::from_str(include_str!("../roles/standards/proposal-example.json")).unwrap();
        let mut state: crate::state::State = serde_json::from_value(json!({
            "agents":[{"id":"writer-1","role":"executor","agent":"codex","repo":"example-repo",
                "session":"test","pane":"%1","writes":true,"live":true,"ready":true}],
        }))
        .unwrap();
        state.application = serde_json::from_value(example["application"].clone()).unwrap();
        let worker = state.agents[0].clone();
        let idle = assigned_contracts(&state, &worker);
        assert_eq!(idle.len(), 2, "no order: every item of its role and repo");
        assert!(assigned_interfaces(&state, &worker).is_empty());

        state.orders = serde_json::from_value(json!([{"id":"o1","step":"status-command",
            "agent":"writer-1","text":"build it","allowed":["src/**"],"forbidden":[],"base":"base","status":"open"}]))
        .unwrap();
        let working = assigned_contracts(&state, &worker);
        assert_eq!(working.keys().collect::<Vec<_>>(), ["status-command"]);
        let interfaces = assigned_interfaces(&state, &worker);
        assert_eq!(interfaces.keys().collect::<Vec<_>>(), ["status-cli"]);
    }

    #[test]
    fn brief_status_keeps_scoped_assignment_and_pending_references() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let state: crate::state::State = serde_json::from_value(json!({
            "agents":[{"id":"writer-1","role":"executor","agent":"codex","repo":"notes",
                "session":"test","pane":"%1","writes":true,"live":true,"ready":true}],
            "orders":[{"id":"o1","step":"api","agent":"writer-1","text":"long order body",
                "allowed":["api.py"],"forbidden":["tests.py"],"base":"base",
                "status":"open","read":["contract.json"],"targets":["tests.py"],"plan":"plan-api"}],
            "order_profiles":{"o1":["unit"]},
            "questions":[
                {"id":"q1","from":"writer-1","to":"planner","order":"o1","text":"open question"},
                {"id":"q2","from":"writer-1","to":"planner","order":"o1","text":"old question","answer":"reply"},
                {"id":"q3","from":"someone","to":"planner","order":"o2","text":"foreign question"}
            ]
        })).unwrap();
        task.transaction(|tx| {
            tx.state = state;
            Ok(())
        })
        .unwrap();

        let brief = call(&task, "writer-1", "status", &json!({"view":"brief"})).unwrap();
        assert_eq!(brief["order"]["plan"], "plan-api");
        assert_eq!(brief["order"]["allowed"], json!(["api.py"]));
        assert_eq!(brief["order"]["forbidden"], json!(["tests.py"]));
        assert_eq!(brief["order"]["read"], json!(["contract.json"]));
        assert_eq!(brief["order"]["targets"], json!(["tests.py"]));
        assert_eq!(brief["order"]["profiles"], json!(["unit"]));
        assert_eq!(brief["questions"].as_array().unwrap().len(), 1);
        assert_eq!(brief["questions"][0]["id"], "q1");
        assert!(brief["order"].get("text").is_none());

        let full = call(&task, "writer-1", "status", &json!({})).unwrap();
        assert_eq!(full["order"]["text"], "long order body");
        assert_eq!(full["questions"].as_array().unwrap().len(), 2);
        assert!(call(&task, "writer-1", "status", &json!({"view":"bad"})).is_err());
    }

    #[test]
    fn only_an_infra_agent_builds_or_tries_images() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let agent = |id: &str, role: &str, writes: bool| crate::state::Agent {
            id: id.into(),
            role: role.into(),
            agent: "codex".into(),
            repo: "app".into(),
            session: String::new(),
            pane: format!("%{id}"),
            writes,
            group: None,
            lane: None,
            seat: 0,
            live: true,
            unavailable: false,
            ready: true,
            lost: false,
            usage: None,
            shows_references: false,
        };
        task.transaction(|tx| {
            tx.state.agents.push(agent("hammer-1", "hammer", true));
            tx.state.agents.push(agent("planner-1", "planner", false));
            tx.state.agents.push(agent("infra-1", INFRA_ROLE, false));
            Ok(())
        })
        .unwrap();
        let build = serde_json::json!({"name": "back", "dockerfile": "FROM alpine:3.20"});
        for (worker, action) in [("hammer-1", "image_build"), ("planner-1", "image_try")] {
            let error = call(&task, worker, action, &build).unwrap_err().to_string();
            assert!(error.contains("only an infra agent"), "{worker}: {error}");
        }
        let error = call(
            &task,
            "infra-1",
            "image_build",
            &serde_json::json!({"name": "Bad Name", "dockerfile": "FROM x"}),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("image name"),
            "the infra agent gets as far as the build: {error}"
        );
    }

    #[test]
    fn an_account_error_before_any_tool_is_a_refusal_to_start_elsewhere() {
        use crate::providers::ReviewedAdapter;
        let balance = r#"{"type":"error","error":{"name":"APIError","data":{"statusCode":402,"message":"Insufficient Balance"}}}"#;
        let refusal = account_refusal(balance, ReviewedAdapter::Opencode).expect("a refusal");
        assert!(
            refusal.reason.contains("insufficient_quota"),
            "{}",
            refusal.reason
        );

        let tool = r#"{"type":"tool_use","sessionID":"ses_1","part":{"id":"p3","tool":"devagentics_worker_worker","state":{"status":"error","input":{"action":"checkpoint","input":{"summary":"Add test"}},"error":"task is paused"}}}"#;
        assert_eq!(
            account_refusal(&format!("{tool}\n{balance}"), ReviewedAdapter::Opencode),
            None,
            "a worker that already used a tool goes to explicit recovery"
        );
        let quota = r#"{"type":"assistant","isApiErrorMessage":true,"error":"rate_limit"}"#;
        assert!(account_refusal(quota, ReviewedAdapter::Claude).is_some());
        let codex = r#"{"type":"event_msg","payload":{"type":"error","codex_error_info":"usage_limit_reached"}}"#;
        assert!(account_refusal(codex, ReviewedAdapter::Codex).is_some());
        let model = r#"{"type":"event_msg","payload":{"type":"error","codex_error_info":"model_not_found"}}"#;
        assert_eq!(
            account_refusal(model, ReviewedAdapter::Codex),
            None,
            "only an account error"
        );
    }

    #[test]
    fn a_refused_worker_is_held_and_marked_for_a_restart() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        task.transaction(|tx| {
            tx.state.agents.push(crate::state::Agent {
                id: "hammer-1".into(),
                role: "hammer".into(),
                agent: "opencode".into(),
                repo: "app".into(),
                session: String::new(),
                pane: "%9".into(),
                writes: true,
                group: None,
                lane: None,
                seat: 0,
                live: true,
                unavailable: false,
                ready: true,
                lost: false,
                usage: None,
                shows_references: false,
            });
            tx.state.probe("hammer-1");
            let claimed = tx.state.queue.pop().unwrap();
            tx.state.worker_turns.insert("hammer-1".into(), claimed);
            Ok(())
        })
        .unwrap();
        let failure = crate::worker::StartupFailure {
            scope: crate::worker::FailureScope::Account,
            reason: "provider startup error: insufficient_quota".into(),
        };
        hold_and_restart(&task, "hammer-1", &failure).unwrap();
        let state = task.state().unwrap();
        assert!(state.account_holds["opencode"].until > crate::log::now());
        assert!(state.worker_turns.is_empty(), "nothing of the turn ran");
        assert!(state.restart_on_next_account.contains("hammer-1"));
        assert!(
            crate::accounts::disabled_for(&state, crate::roles::Group::Floor).contains("opencode")
        );
        assert!(
            state
                .queue
                .iter()
                .any(|message| message.to == crate::state::LEADER
                    && message.text.contains("nothing to do unless"))
        );
    }

    #[test]
    fn a_worker_saves_notes_for_the_next_and_searches_the_tasks() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, devws, task, _repo) = crate::board::tests::started(&directory);
        let devws = devws.with_memory();
        task.transaction(|tx| {
            tx.state.agents.push(crate::state::Agent {
                id: "hammer-1".into(),
                role: "hammer".into(),
                agent: "opencode".into(),
                repo: "app".into(),
                session: String::new(),
                pane: "%9".into(),
                writes: true,
                group: None,
                lane: None,
                seat: 0,
                live: true,
                unavailable: false,
                ready: true,
                lost: false,
                usage: None,
                shows_references: false,
            });
            Ok(())
        })
        .unwrap();
        let saved = call_in(
            &task,
            &devws,
            "hammer-1",
            "memory_save",
            &serde_json::json!({"kind": "gotcha", "title": "Biome", "body": "Run biome from web/.", "step": "B3"}),
        )
        .unwrap();
        assert_eq!(saved["saved"], true);
        let (_, path, note) = &devws.notes()[0];
        assert!(path.starts_with("tasks/feat-x/steps/b3/gotcha-"), "{path}");
        assert_eq!(note.author, "hammer-1");
        assert_eq!(note.step.as_deref(), Some("B3"));

        let found = call_in(
            &task,
            &devws,
            "hammer-1",
            "memory_search",
            &serde_json::json!({"query": "biome"}),
        )
        .unwrap();
        assert_eq!(found["notes"].as_array().unwrap().len(), 1);

        let refused = call_in(
            &task,
            &devws,
            "hammer-1",
            "memory_save",
            &serde_json::json!({"kind": "report", "title": "x", "body": "y"}),
        )
        .unwrap_err();
        assert!(refused.to_string().contains("kind is"), "{refused}");
        task.transaction(|tx| {
            tx.state.agents[0].live = false;
            Ok(())
        })
        .unwrap();
        assert!(
            call_in(
                &task,
                &devws,
                "hammer-1",
                "memory_search",
                &serde_json::json!({"query": "biome"})
            )
            .is_err(),
            "a stopped worker has no tools"
        );
    }

    #[test]
    fn a_reviewer_reads_the_plan_of_the_order_it_reviews() {
        let order = |id: &str, step: &str, plan: Option<&str>| crate::state::Order {
            read: Vec::new(),
            targets: Vec::new(),
            id: id.into(),
            step: step.into(),
            agent: "hammer-7".into(),
            text: String::new(),
            allowed: vec![],
            forbidden: vec![],
            check: None,
            base: String::new(),
            status: crate::state::OrderStatus::Closed,
            delivered: true,
            stalled: false,
            plan: plan.map(str::to_string),
        };
        let state = crate::state::State {
            orders: vec![
                order("o140", "B1-1", None),
                order("o161", "B1-1", Some("B1-1-plan")),
            ],
            reviews: vec![crate::reviews::Review {
                id: "r171".into(),
                order: "o161".into(),
                reviewer: "code-reviewer-3".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            followed_plan(&state, "code-reviewer-3").as_deref(),
            Some("B1-1-plan")
        );
        assert_eq!(followed_plan(&state, "nobody"), None);
        assert_eq!(plan_of_step(&state, "B1-1").as_deref(), Some("B1-1-plan"));
        assert_eq!(plan_of_step(&state, "B9"), None);
    }

    #[test]
    fn a_review_shows_the_whole_step_even_after_several_orders() {
        let review = crate::reviews::Review {
            base: "49b4c48".into(),
            source: "49b4c48".into(),
            target: "113e846".into(),
            ..Default::default()
        };
        // The last order started from the work an earlier one committed.
        assert_eq!(review_range(&review), "113e846...49b4c48");
    }

    #[test]
    fn a_managed_panel_shows_its_model_in_the_picker() {
        let folder = tempfile::tempdir().unwrap();
        let devws = crate::devws::fake::FakeDevws::new(folder.path());
        show_model(&devws, "%7", Some("gpt-6.1-sol")).unwrap();
        assert_eq!(
            devws.option("%7", "@devws_model").as_deref(),
            Some("gpt-6.1-sol")
        );
        assert_eq!(
            devws.option("%7", "@devws_model_configured").as_deref(),
            Some("gpt-6.1-sol")
        );
        show_model(&devws, "%8", None).unwrap();
        assert_eq!(
            devws.option("%8", "@devws_model").as_deref(),
            Some("default model")
        );
    }
    #[test]
    fn native_tools_and_other_mcp_configs_are_disabled() {
        let args = claude_arguments(
            Path::new("/scoped/mcp.json"),
            "work",
            Some("sonnet"),
            "uuid",
            false,
        );
        let position = args
            .iter()
            .position(|arg| arg == "--output-format")
            .unwrap();
        assert_eq!(
            args[position + 1],
            "stream-json",
            "the panel draws events as they come"
        );
        assert!(
            args.contains(&"--verbose".to_string()),
            "stream-json needs --verbose with --print"
        );
        for flag in [
            "--print",
            "--restricted",
            "--tools",
            "--strict-mcp-config",
            "--disable-slash-commands",
            "--no-chrome",
        ] {
            assert!(args.contains(&flag.into()));
        }
        let position = args.iter().position(|arg| arg == "--tools").unwrap();
        assert_eq!(args[position + 1], "");
        assert!(
            !args
                .iter()
                .any(|arg| arg == "--dangerously-skip-permissions")
        );
    }

    fn codex(thread: Option<&str>) -> Vec<String> {
        codex_arguments(&CodexTurn {
            executable: Path::new("/bin/devagentics"),
            task: "feat-x",
            agent: "code-reviewer-1",
            identity: Path::new("/task/workers/code-reviewer-1/identity"),
            prompt: "review r1",
            model: Some("gpt-6.1-sol"),
            effort: Some("high"),
            thread,
            writes: false,
        })
    }

    #[test]
    fn a_codex_reviewer_runs_read_only_with_only_the_worker_tools() {
        let first = codex(None);
        assert_eq!(&first[..1], ["exec"]);
        for flag in [
            "--json",
            "--ignore-user-config",
            "--ignore-rules",
            "--skip-git-repo-check",
        ] {
            assert!(first.contains(&flag.into()), "{flag}: {first:?}");
        }
        for setting in [
            "sandbox_mode=\"read-only\"",
            "approval_policy=\"never\"",
            "mcp_servers.devagentics_worker.default_tools_approval_mode=\"approve\"",
            "mcp_servers.devagentics_worker.env_vars=[\"DEVAGENTICS_HOME\",\"XDG_RUNTIME_DIR\",\"TMUX\",\"TMUX_PANE\"]",
            "model_reasoning_effort=\"high\"",
            "mcp_servers.devagentics_worker.command=\"/bin/devagentics\"",
            "mcp_servers.devagentics_worker.args=[\"worker-mcp\", \"feat-x\", \"code-reviewer-1\", \"--identity\", \"/task/workers/code-reviewer-1/identity\"]",
            "mcp_servers.devagentics_worker.tool_timeout_sec=3600",
        ] {
            let position = first.iter().position(|arg| arg == setting);
            assert!(
                position.is_some_and(|position| first[position - 1] == "-c"),
                "{setting}: {first:?}"
            );
        }
        assert!(
            !first.iter().any(|arg| arg.contains("danger")
                || arg.contains("workspace-write")
                || arg == "--approve-for-me"),
            "{first:?}"
        );
        assert_eq!(first.last().unwrap(), "review r1");
        assert_eq!(
            first[first.iter().position(|arg| arg == "-m").unwrap() + 1],
            "gpt-6.1-sol"
        );

        let resumed = codex(Some("thread-9"));
        assert_eq!(&resumed[..3], ["exec", "resume", "thread-9"]);
        assert!(resumed.contains(&"sandbox_mode=\"read-only\"".to_string()));
        assert_eq!(resumed.last().unwrap(), "review r1");
    }

    #[test]
    fn a_codex_writer_writes_only_in_its_snapshot_without_network() {
        let args = codex_arguments(&CodexTurn {
            executable: Path::new("/bin/devagentics"),
            task: "feat-x",
            agent: "backend-data-1",
            identity: Path::new("/task/workers/backend-data-1/identity"),
            prompt: "build it",
            model: None,
            effort: None,
            thread: None,
            writes: true,
        });
        for setting in [
            "sandbox_mode=\"workspace-write\"",
            "sandbox_workspace_write.network_access=false",
            "sandbox_workspace_write.exclude_tmpdir_env_var=true",
            "sandbox_workspace_write.exclude_slash_tmp=true",
            "approval_policy=\"never\"",
        ] {
            assert!(args.contains(&setting.to_string()), "{setting}: {args:?}");
        }
        assert!(
            !args
                .iter()
                .any(|arg| arg.contains("danger") || arg.contains("read-only"))
        );
        assert!(
            codex(None).contains(&"sandbox_mode=\"read-only\"".to_string()),
            "a reviewer stays read-only"
        );
    }

    #[test]
    fn a_codex_leader_reads_only_and_uses_only_the_leader_tools_without_prompts() {
        let args = codex_leader_arguments(
            "/bin/devagentics",
            "feat-x",
            "lead",
            Some("gpt-6.1-sol"),
            Some("medium"),
            false,
        );
        let pair = |flag: &str| args[args.iter().position(|arg| arg == flag).unwrap() + 1].clone();
        assert_eq!(pair("-s"), "read-only");
        assert_eq!(pair("-a"), "never");
        assert_eq!(pair("-m"), "gpt-6.1-sol");
        for setting in [
            "mcp_servers.devagentics.command=\"/bin/devagentics\"",
            "mcp_servers.devagentics.args=[\"mcp\", \"feat-x\"]",
            "mcp_servers.devagentics.default_tools_approval_mode=\"approve\"",
            "mcp_servers.devagentics.env_vars=[\"DEVAGENTICS_HOME\",\"XDG_RUNTIME_DIR\",\"TMUX\",\"TMUX_PANE\"]",
            "model_reasoning_effort=\"medium\"",
        ] {
            assert!(args.contains(&setting.to_string()), "{setting}: {args:?}");
        }
        assert!(
            !args
                .iter()
                .any(|arg| arg.contains("danger") || arg == "--approve-for-me")
        );
        assert_eq!(args.last().unwrap(), "lead");
    }

    #[test]
    fn the_codex_thread_comes_from_its_json_events() {
        assert_eq!(
            thread_id(r#"{"type":"thread.started","thread_id":"0199-abc"}"#),
            Some("0199-abc".into())
        );
        assert_eq!(thread_id(r#"{"type":"turn.started"}"#), None);
        assert_eq!(thread_id("not json"), None);
    }
}
