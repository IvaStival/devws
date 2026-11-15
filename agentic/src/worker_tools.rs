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
        let prompt = format!(
            "{role}\n{standards}\nYou are {agent}. Native tools are disabled. Use only devagentics_worker worker tools. Call status first. Execute commands only through the isolated execute action. Checkpoint before check/report. Do not use host paths or attempt external/database actions. Process this message, report or submit review, then finish this turn:\n{}",
            message.text
        );
        let turn_start = crate::log::now();
        let mut arguments = claude_arguments(&config, &prompt, model, &session, resumed);
        if let Some(effort) = task
            .state()?
            .worker_launches
            .get(agent)
            .and_then(|launch| launch.selected.as_ref())
            .and_then(|program| program.effort.clone())
        {
            let position = arguments.len() - 1;
            arguments.splice(position..position, ["--effort".into(), effort]);
        }
        let status = Command::new("claude")
            .args(arguments)
            .current_dir(&directory)
            .stdin(Stdio::null())
            .status()?;
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
}
