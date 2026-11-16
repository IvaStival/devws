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
        "--output-format".into(),
        "json".into(),
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
        format!("sandbox_mode={}", toml_string("read-only")),
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
) -> Result<std::process::ExitStatus> {
    let state = task.state()?;
    let snapshot = state
        .runner_workspaces
        .get(agent)
        .filter(|workspace| workspace.readonly)
        .map(|workspace| workspace.snapshot.clone())
        .context("a Codex reviewer needs its read-only review snapshot")?;
    let thread = state.worker_sessions.get(agent).cloned();
    let executable = std::env::current_exe()?;
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
        }))
        .current_dir(&snapshot)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .context("starting codex")?;
    let mut started = None;
    let output = child.stdout.take().context("codex output")?;
    for line in std::io::BufReader::new(output).lines() {
        let line = line?;
        println!("{line}");
        if started.is_none() {
            started = thread_id(&line);
        }
    }
    let status = child.wait()?;
    if let (None, Some(started)) = (thread, started) {
        task.transaction(|tx| {
            tx.state
                .worker_sessions
                .insert(agent.into(), started.clone());
            Ok(())
        })?;
    }
    Ok(status)
}

/// The conversation id in a line of `codex exec --json` output.
pub fn thread_id(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line)
        .ok()?
        .get("thread_id")?
        .as_str()
        .map(str::to_string)
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
                json!({"tools":[{"name":"worker","description":"Scoped worker tools. action=status, execute, checkpoint, check, report, plan_proposal, ask_user, review_diff, review_submit, ask or answer. Execute runs only inside the isolated source container. No leader, database or external-action permissions.","inputSchema":{"type":"object","properties":{"action":{"type":"string"},"input":{"type":"object"}},"required":["action"]}}]})
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
            json!({"agent":agent,"role":worker.role,"order":state.open_order_of(agent),"application_revision":state.application.revision,"planning_proposal":if !worker.writes{state.application_proposal.clone()}else{None},"contracts":state.application.items.iter().filter(|(_,item)|item.role==worker.role && item.repo==worker.repo).collect::<std::collections::BTreeMap<_,_>>(),"questions":state.questions.iter().filter(|question|question.to==agent||question.from==agent).collect::<Vec<_>>(),"reviews":state.reviews.iter().filter(|review|review.reviewer==agent).collect::<Vec<_>>()}),
        ),
        "plan_proposal" => {
            ensure!(
                !worker.writes && worker.role != "code-reviewer",
                "only a planning specialist proposes a baseline"
            );
            let application = serde_json::from_value(input["application"].clone())?;
            let affected = crate::execution::store(task, application, field(input, "reason")?)?;
            task.transaction(|tx|{if let Some(order)=tx.state.open_order_of(agent).cloned(){if let Some(step)=tx.state.step_mut(&order.step){step.set_status(crate::state::StepStatus::Reported,crate::log::now());}
                if let Some(order)=tx.state.orders.iter_mut().find(|assignment|assignment.id==order.id){order.status=crate::state::OrderStatus::Closed;}}tx.state.say(crate::state::LEADER,format!("Planner {agent} proposed an application baseline. Inspect application_workflow status, interview the user and obtain explicit confirmation and approval before implementation."));Ok(())})?;
            Ok(json!({"proposed":true,"affected":affected}))
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
                worker.writes,
                "planning and review use their scoped result tools"
            );
            let order = state.open_order_of(agent).context("no assignment")?;
            let outcome = match field(input, "outcome")? {
                "done" => crate::orders::Outcome::Done,
                "blocked" => crate::orders::Outcome::Blocked,
                _ => bail!("outcome must be done or blocked"),
            };
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
                json!({"review":review,"diff":crate::checks::git(&repo.worktree,&["diff","--no-ext-diff","--no-textconv",&format!("{}..{}",review.base,review.source)])?}),
            )
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
/// The panel remains a supervisor, never an interactive editable model prompt.
pub fn panel(task: &Task, agent: &str, identity: &Path, model: Option<&str>) -> Result<()> {
    let devws = CommandDevws::from_env();
    let pane = std::env::var("TMUX_PANE").context("managed worker requires a pane")?;
    let directory = task.dir().join("workers").join(agent).join("control");
    std::fs::create_dir_all(&directory)?;
    let executable = std::env::current_exe()?;
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
            "Your own shell is read-only and outside the isolated runner: use it only to read the review checkout in your working directory, never other host files. Review and submit only through devagentics_worker worker tools."
        } else {
            "Native tools are disabled. Use only devagentics_worker worker tools."
        };
        let prompt = format!(
            "{role}\n{standards}\nYou are {agent}. {tools} Call status first. Execute commands only through the isolated execute action. Checkpoint before check/report. Do not use host paths or attempt external/database actions. Process this message, report or submit review, then finish this turn:\n{}",
            message.text
        );
        let turn_start = crate::log::now();
        let effort = task
            .state()?
            .worker_launches
            .get(agent)
            .and_then(|launch| launch.selected.as_ref())
            .and_then(|program| program.effort.clone());
        let status = if program == crate::agent_args::Kind::Codex {
            codex_turn(task, agent, identity, &prompt, model, effort.as_deref())?
        } else {
            let mut arguments = claude_arguments(&config, &prompt, model, &session, resumed);
            if let Some(effort) = effort {
                let position = arguments.len() - 1;
                arguments.splice(position..position, ["--effort".into(), effort]);
            }
            Command::new("claude")
                .args(arguments)
                .current_dir(&directory)
                .stdin(Stdio::null())
                .status()?
        };
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
    fn native_tools_and_other_mcp_configs_are_disabled() {
        let args = claude_arguments(
            Path::new("/scoped/mcp.json"),
            "work",
            Some("sonnet"),
            "uuid",
            false,
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
    fn the_codex_thread_comes_from_its_json_events() {
        assert_eq!(
            thread_id(r#"{"type":"thread.started","thread_id":"0199-abc"}"#),
            Some("0199-abc".into())
        );
        assert_eq!(thread_id(r#"{"type":"turn.started"}"#), None);
        assert_eq!(thread_id("not json"), None);
    }
}
