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
            "mcp_servers.devagentics.args=[{}, {}]",
            toml_string("mcp"),
            toml_string(task)
        ),
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
pub fn leader_arguments(
    config: &Path,
    prompt: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Vec<String> {
    let mut arguments = vec![
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
        "mcp__devagentics".into(),
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
        let response = match request["method"].as_str().unwrap_or("") {
            "initialize" => {
                json!({"protocolVersion":request["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),"capabilities":{"tools":{}},"serverInfo":{"name":"devagentics_worker","version":env!("CARGO_PKG_VERSION")}})
            }
            "tools/list" => {
                json!({"tools":[{"name":"worker","description":"Scoped worker tools. action=status, execute, checkpoint, check, report, plan_proposal (input.application: the whole next revision), plan_amend (input.changes: only what changes in the next revision, by id, null removes; input.reason), application_read (the baseline as an outline; input.items [ids]: their full contracts; input.proposal true: the pending proposal), plan_write (input.plan: the execution plan of your order's step, markdown), plan_read (the plan your order follows, or input.step), ask_user, review_diff, review_submit, graph, ask or answer. Execute runs only inside the isolated source container. No leader, database or external-action permissions.","inputSchema":{"type":"object","properties":{"action":{"type":"string"},"input":{"type":"object"}},"required":["action"]}}]})
            }
            "tools/call" => {
                let result = (|| {
                    runner::authenticate(task, agent, identity)?;
                    ensure!(request["params"]["name"] == "worker", "unknown worker tool");
                    call(
                        task,
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
        writeln!(
            output,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"result":response})
        )?;
        output.flush()?;
    }
    Ok(())
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

fn field<'a>(input: &'a Value, name: &str) -> Result<&'a str> {
    input[name]
        .as_str()
        .with_context(|| format!("{name} is required"))
}
pub fn call(task: &Task, agent: &str, action: &str, input: &Value) -> Result<Value> {
    let state = task.state()?;
    let worker = state.agent(agent).context("unknown worker")?;
    ensure!(worker.live, "worker stopped");
    let pane = &worker.pane;
    match action {
        "status" => Ok(
            json!({"agent":agent,"role":worker.role,"order":state.open_order_of(agent),"application_revision":state.application.revision,"planning_proposal":if !worker.writes{state.application_proposal.as_ref().map(crate::execution::outline)}else{None},"contracts":state.application.items.iter().filter(|(_,item)|item.role==worker.role && item.repo==worker.repo).collect::<std::collections::BTreeMap<_,_>>(),"questions":state.questions.iter().filter(|question|question.to==agent||question.from==agent).collect::<Vec<_>>(),"reviews":state.reviews.iter().filter(|review|review.reviewer==agent).collect::<Vec<_>>(),"reviewed_contracts":reviewed_contracts(&state,agent)}),
        ),
        "plan_proposal" | "plan_amend" => {
            ensure!(
                !worker.writes && worker.role != "code-reviewer",
                "only a planning specialist proposes a baseline"
            );
            let reason = field(input, "reason")?;
            let affected = if action == "plan_amend" {
                let changes = serde_json::from_value(input["changes"].clone())?;
                crate::execution::amend(task, changes, reason)?
            } else {
                let application = serde_json::from_value(input["application"].clone())?;
                crate::execution::store(task, application, reason)?
            };
            task.transaction(|tx|{if let Some(order)=tx.state.open_order_of(agent).cloned(){if let Some(step)=tx.state.step_mut(&order.step){step.set_status(crate::state::StepStatus::Reported,crate::log::now());}
                if let Some(order)=tx.state.orders.iter_mut().find(|assignment|assignment.id==order.id){order.status=crate::state::OrderStatus::Closed;}}tx.state.say(crate::state::LEADER,format!("Planner {agent} proposed an application baseline. Inspect application_workflow status, interview the user and obtain explicit confirmation and approval before implementation."));Ok(())})?;
            Ok(json!({"proposed":true,"affected":affected}))
        }
        "application_read" => {
            ensure!(!worker.writes, "only a read-only specialist reads the baseline");
            let application = if input["proposal"].as_bool() == Some(true) {
                state
                    .application_proposal
                    .as_ref()
                    .context("no baseline proposal awaiting decision")?
            } else {
                &state.application
            };
            match input.get("items") {
                Some(ids) => crate::execution::items(application, &serde_json::from_value::<Vec<String>>(ids.clone())?),
                None => Ok(crate::execution::outline(application)),
            }
        }
        "plan_read" => {
            // The plan its order follows, unless it names another step.
            let step = match input["step"].as_str() {
                Some(step) => step.to_string(),
                None => state
                    .open_order_of(agent)
                    .and_then(|order| order.plan.clone())
                    .context("your order follows no plan: name the step")?,
            };
            Ok(json!({"step":step,"plan":crate::plans::read(task,&step)?}))
        }
        "plan_write" => {
            ensure!(
                !worker.writes && worker.role != "code-reviewer",
                "only a planning specialist writes an execution plan"
            );
            let step = crate::plans::store_execution(task, agent, field(input, "plan")?)?;
            Ok(json!({"stored":true,"step":step,"next":"report the order done; the leader reads it with read_plan"}))
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
        "execute" => Ok(serde_json::to_value(runner::execute(
            task,
            agent,
            field(input, "command")?,
        )?)?),
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
                return Ok(json!({"result":crate::orders::report_findings(task,agent,&order.id,outcome,field(input,"summary")?)?}));
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
                json!({"review":review,"diff":crate::checks::git(&repo.worktree,&["diff","--no-ext-diff","--no-textconv",&review_range(review)])?}),
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

/// The panel remains a supervisor, never an interactive editable model prompt.
pub fn panel(task: &Task, agent: &str, identity: &Path, model: Option<&str>) -> Result<()> {
    let devws = CommandDevws::from_env();
    let pane = std::env::var("TMUX_PANE").context("managed worker requires a pane")?;
    show_model(&devws, &pane, model)?;
    let directory = task.dir().join("workers").join(agent).join("control");
    std::fs::create_dir_all(&directory)?;
    let program_name = |program| match program {
        crate::agent_args::Kind::Codex => "codex",
        _ => "claude",
    };
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
    loop {
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
            std::thread::sleep(Duration::from_millis(250));
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
            std::thread::sleep(Duration::from_millis(250));
            continue;
        };
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
        let tools = if program == crate::agent_args::Kind::Codex {
            "Your own shell runs on the host, outside the isolated runner, and can write only your working directory (your source snapshot), with no network. Read and change only files there, never other host files. Run checks and tests only through devagentics_worker worker tools (check, execute), and checkpoint before you report. Review, report and ask only through those tools."
        } else {
            "Native tools are disabled. Use only devagentics_worker worker tools."
        };
        let graph = if task.state()?.graph_enabled {
            " A code graph of your source is on: worker action=graph with action=explain and symbol (definition, callers, callees; an ambiguous name lists candidates, retry with path::symbol), path with from and to, or query with question. Use it to find what to reuse, the real interfaces and who a change affects, before reading files one by one."
        } else {
            ""
        };
        let prompt = format!(
            "{role}\n{standards}\nYou are {agent}. {tools}{graph} Call status first. Execute commands only through the isolated execute action. Checkpoint before check/report. Do not use host paths or attempt external/database actions. Process this message, report or submit review, then finish this turn:\n{}",
            message.text
        );
        let turn_start = crate::log::now();
        let effort = task
            .state()?
            .worker_launches
            .get(agent)
            .and_then(|launch| launch.selected.as_ref())
            .and_then(|program| program.effort.clone());
        let order = message.order.as_ref().map(|id| {
            let step = task
                .state()
                .ok()
                .and_then(|state| state.orders.iter().find(|order| order.id == *id).map(|order| order.step.clone()))
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
        let (status, usage) = if program == crate::agent_args::Kind::Codex {
            codex_turn(task, agent, identity, &prompt, model, effort.as_deref(), &raw)?
        } else {
            let mut arguments = claude_arguments(&config, &prompt, model, &session, resumed);
            if let Some(effort) = effort {
                let position = arguments.len() - 1;
                arguments.splice(position..position, ["--effort".into(), effort]);
            }
            claude_turn(arguments, &directory, &raw)?
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
            if status.success(){tx.state.worker_turns.remove(agent);}else{tx.state.say(crate::state::LEADER,format!("Managed worker {agent} failed. Its claimed message is retained for explicit recovery; do not resend automatically."));}
            Ok(())
        })?;
        devws.mark(&pane, if status.success() { "done" } else { "waiting" })?;
        ensure!(
            status.success(),
            "managed provider turn failed; inspect the retained turn before recovery"
        );
        resumed = true;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(devws.option("%7", "@devws_model").as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(
            devws.option("%7", "@devws_model_configured").as_deref(),
            Some("gpt-6.1-sol")
        );
        show_model(&devws, "%8", None).unwrap();
        assert_eq!(devws.option("%8", "@devws_model").as_deref(), Some("default model"));
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
        let position = args.iter().position(|arg| arg == "--output-format").unwrap();
        assert_eq!(args[position + 1], "stream-json", "the panel draws events as they come");
        assert!(args.contains(&"--verbose".to_string()), "stream-json needs --verbose with --print");
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
            "model_reasoning_effort=\"high\"",
            "mcp_servers.devagentics_worker.command=\"/bin/devagentics\"",
            "mcp_servers.devagentics_worker.args=[\"worker-mcp\", \"feat-x\", \"code-reviewer-1\", \"--identity\", \"/task/workers/code-reviewer-1/identity\"]",
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
        );
        let pair = |flag: &str| args[args.iter().position(|arg| arg == flag).unwrap() + 1].clone();
        assert_eq!(pair("-s"), "read-only");
        assert_eq!(pair("-a"), "never");
        assert_eq!(pair("-m"), "gpt-6.1-sol");
        for setting in [
            "mcp_servers.devagentics.command=\"/bin/devagentics\"",
            "mcp_servers.devagentics.args=[\"mcp\", \"feat-x\"]",
            "mcp_servers.devagentics.default_tools_approval_mode=\"approve\"",
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
