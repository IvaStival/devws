//! Orders and what happens to them. The leader gives an agent one order at a
//! time; when the agent says it is done, devAgentics checks the work itself
//! (checks.rs) and tells the leader what it found; the leader accepts or
//! rejects the step.

use crate::checks::{self, CheckRun, Work};
use crate::log::Kind;
use crate::roles::Group;
use crate::state::{LEADER, Order, OrderStatus, Step, StepStatus};
use crate::task::{Repo, Task};
use anyhow::{Context, Result, bail};
use std::time::Duration;

const DEFAULT_CHECK_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// How long a check may run: `DEVAGENTICS_CHECK_TIMEOUT` seconds, else 15 minutes.
pub fn check_timeout() -> Duration {
    std::env::var("DEVAGENTICS_CHECK_TIMEOUT")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_CHECK_TIMEOUT)
}

pub struct OrderRequest {
    pub agent: String,
    pub step: String,
    pub title: String,
    pub text: String,
    pub allowed: Vec<String>,
    pub forbidden: Vec<String>,
    pub check: Option<String>,
    /// The senior step whose execution plan it follows; required for the
    /// factory floor.
    pub plan: Option<String>,
}

/// Gives an agent an order: records it, and queues the message that tells the
/// agent (the dispatcher types it in when the agent can take it).
pub fn send_order(task: &Task, request: OrderRequest) -> Result<String> {
    let agent = task
        .state()?
        .agent(&request.agent)
        .filter(|agent| agent.live)
        .cloned()
        .with_context(|| format!("there is no live agent {}", request.agent))?;
    // A lane's agent: its lane takes the other lanes' accepted work first,
    // and its files are the lane's unless the order narrows them.
    let lane = agent
        .lane
        .as_deref()
        .map(|name| crate::lanes::get(task, name))
        .transpose()?;
    if let Some(lane) = &lane
        && task.state()?.open_order_of(&agent.id).is_none()
    {
        crate::lanes::catch_up(task, lane)?;
    }
    let repo = crate::lanes::workplace(task, &agent)?;
    let base = checks::head(&repo.worktree)?;
    let check = request.check.clone().or(repo.check.clone());
    let allowed = match &lane {
        Some(lane) if request.allowed.is_empty() => lane.globs.clone(),
        _ => request.allowed.clone(),
    };

    task.transaction(|tx| {
        if agent.group() == Group::Floor {
            let plan = request.plan.as_deref().with_context(|| {
                format!(
                    "{} is on the factory floor: it only runs a senior's accepted plan (send_order with plan = the senior's step)",
                    agent.id
                )
            })?;
            if tx.state.plan(plan).is_none() {
                bail!("step {plan} has no execution plan: a senior writes one first");
            }
            let accepted = tx
                .state
                .steps
                .iter()
                .any(|step| step.id == plan && step.status == StepStatus::Accepted);
            if !accepted {
                bail!("the plan of step {plan} is not accepted yet: read it (read_plan) and accept_step it first");
            }
        }
        if let Some(open) = tx.state.open_order_of(&agent.id) {
            bail!(
                "{} still has order {} open: wait for its report",
                agent.id,
                open.id
            );
        }
        match tx.state.steps.iter().find(|step| step.id == request.step) {
            Some(step) if step.status == StepStatus::Ordered => {
                bail!("step {} is being worked on", step.id)
            }
            Some(step) if step.status == StepStatus::Reported => {
                bail!("step {} is waiting for your review", step.id)
            }
            Some(step) if step.status == StepStatus::Accepted => {
                bail!("step {} is already accepted", step.id)
            }
            _ => {}
        }
        match tx.state.step_mut(&request.step) {
            Some(step) => {
                step.title = request.title.clone();
                step.set_status(StepStatus::Ordered, crate::log::now());
                step.attempts += 1;
            }
            None => tx.state.steps.push(Step {
                id: request.step.clone(),
                title: request.title.clone(),
                status: StepStatus::Ordered,
                attempts: 1,
                review_since: None,
            }),
        }

        // A senior plans and answers; any file it changes fails its report.
        let mut forbidden = request.forbidden.clone();
        if agent.group() == Group::Senior && !forbidden.iter().any(|glob| glob == "**") {
            forbidden.push("**".to_string());
        }
        let id = format!("o{}", tx.state.next_number());
        let order = Order {
            id: id.clone(),
            step: request.step.clone(),
            agent: agent.id.clone(),
            text: request.text.clone(),
            allowed,
            forbidden,
            check,
            base,
            status: OrderStatus::Open,
            delivered: false,
            stalled: false,
            plan: request.plan.clone(),
        };
        tx.state.say_about(
            &agent.id,
            order_message(&order, &request.title),
            Some(order.id.clone()),
        );
        tx.log(
            Kind::Order,
            Some(&agent.id),
            Some(&order.step),
            &request.text,
        );
        tx.state.orders.push(order);
        Ok(id)
    })
}

/// What the agent is told.
pub(crate) fn order_message(order: &Order, title: &str) -> String {
    let list = |globs: &[String], none: &str| {
        if globs.is_empty() {
            none.to_string()
        } else {
            globs.join(", ")
        }
    };
    let plan = order.plan.as_ref().map_or_else(String::new, |plan| {
        format!(
            "Follow the plan of step {plan}: `devagentics plan show {plan}`. \
             If something in it is unclear, ask: `devagentics ask \"<your question>\"`, then end your turn and wait for the answer.\n\n"
        )
    });
    let files = if order.forbidden.iter().any(|glob| glob == "**") {
        "You change no files: you plan and answer, and any file you change fails your report.\n"
            .to_string()
    } else {
        format!(
            "Files you may change: {}\nFiles you must not change: {}\n",
            list(&order.allowed, "any"),
            list(&order.forbidden, "none")
        )
    };
    format!(
        "ORDER {id} · step {step}: {title}\n\n{plan}{text}\n\n\
         {files}\
         Done means your work is committed (a clean tree) and the check passes: {check}\n\
         When you are done: devagentics report done \"<what you did>\" — or, if something stops you: \
         devagentics report blocked \"<what stops you>\".",
        id = order.id,
        step = order.step,
        text = order.text,
        check = order.check.as_deref().unwrap_or("none is set"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Blocked,
}

/// An agent says it is done, or stuck. For `Done` devAgentics checks the work
/// itself: a clean tree, only the allowed files, the repo's check. What it
/// found goes to the leader; the step is reported (the leader reviews it) or
/// failed. Returns what the agent is told.
pub fn report(
    task: &Task,
    pane: &str,
    outcome: Outcome,
    summary: &str,
    timeout: Duration,
) -> Result<String> {
    let state = task.state()?;
    let agent = state
        .agent_in_pane(pane)
        .with_context(|| format!("{pane} is not a team agent's pane"))?
        .clone();
    let order = state
        .open_order_of(&agent.id)
        .with_context(|| format!("{} has no open order to report on", agent.id))?
        .clone();

    if outcome == Outcome::Blocked {
        return task.transaction(|tx| {
            close(tx, &order, StepStatus::Blocked);
            tx.state.say(
                LEADER,
                format!("{} is blocked on step {}: {summary}", agent.id, order.step),
            );
            tx.log(
                Kind::Report,
                Some(&agent.id),
                Some(&order.step),
                &format!("blocked: {summary}"),
            );
            Ok("Reported to the leader. Wait for the next order.".to_string())
        });
    }

    let repo = crate::lanes::workplace(task, &agent)?;
    let work = checks::inspect(
        &repo.worktree,
        &order.base,
        &order.allowed,
        &order.forbidden,
    )?;
    let attempt = state
        .steps
        .iter()
        .find(|step| step.id == order.step)
        .map_or(1, |step| step.attempts);
    let check = match (
        &order.check,
        work.uncommitted.is_empty() && work.follows_the_rules(),
    ) {
        (Some(command), true) => {
            let logs = task.dir().join("checks");
            std::fs::create_dir_all(&logs)?;
            Some(checks::run_check(
                command,
                &repo.worktree,
                &logs.join(format!("{}-{attempt}.log", order.step)),
                timeout,
            )?)
        }
        _ => None,
    };

    let passed = work.uncommitted.is_empty()
        && work.follows_the_rules()
        && check.as_ref().is_none_or(|run| run.passed);
    let message = result_message(
        &agent.id,
        &order,
        summary,
        &work,
        check.as_ref(),
        &repo,
        passed,
    );
    task.transaction(|tx| {
        close(
            tx,
            &order,
            if passed {
                StepStatus::Reported
            } else {
                StepStatus::Failed
            },
        );
        tx.log(Kind::Report, Some(&agent.id), Some(&order.step), summary);
        tx.log(Kind::Check, Some(&agent.id), Some(&order.step), &message);
        tx.state.say(LEADER, message.clone());
        Ok(if passed {
            "Reported to the leader, who will review it. Wait for the next order.".to_string()
        } else {
            "Reported, but the checks found problems: the leader has them. Wait for the next order."
                .to_string()
        })
    })
}

fn close(tx: &mut crate::task::Tx, order: &Order, step_status: StepStatus) {
    if let Some(open) = tx
        .state
        .orders
        .iter_mut()
        .find(|candidate| candidate.id == order.id)
    {
        open.status = OrderStatus::Closed;
    }
    if let Some(step) = tx.state.step_mut(&order.step) {
        step.set_status(step_status, crate::log::now());
    }
}

/// What the leader is told about a report.
fn result_message(
    agent: &str,
    order: &Order,
    summary: &str,
    work: &Work,
    check: Option<&CheckRun>,
    repo: &Repo,
    passed: bool,
) -> String {
    let mut lines = vec![format!(
        "{agent} reports step {} done{}.",
        order.step,
        if passed {
            ""
        } else {
            " — but the checks FAILED"
        }
    )];
    lines.push(format!("Report: {summary}"));

    if work.uncommitted.is_empty() {
        lines.push("Tree: clean.".to_string());
    } else {
        lines.push(format!(
            "Tree: NOT clean, uncommitted: {}.",
            work.uncommitted.join(", ")
        ));
    }
    if work.follows_the_rules() {
        lines.push(format!(
            "Files: rules kept; {} changed (+{} −{}).",
            work.changed.len(),
            work.added,
            work.removed
        ));
    } else {
        if !work.forbidden.is_empty() {
            lines.push(format!(
                "Files: FORBIDDEN changed: {}.",
                work.forbidden.join(", ")
            ));
        }
        if !work.outside.is_empty() {
            lines.push(format!(
                "Files: OUTSIDE the allowed ones: {}.",
                work.outside.join(", ")
            ));
        }
    }
    match check {
        Some(run) if run.passed => lines.push(format!(
            "Check: passed ('{}', {}s).",
            run.command, run.seconds
        )),
        Some(run) if run.timed_out => lines.push(format!(
            "Check: TIMED OUT after {}s ('{}'), log {}.",
            run.seconds,
            run.command,
            run.log.display()
        )),
        Some(run) => lines.push(format!(
            "Check: FAILED (exit {}, '{}'), log {}.",
            run.exit_code
                .map_or("none".to_string(), |code| code.to_string()),
            run.command,
            run.log.display()
        )),
        None if !work.uncommitted.is_empty() || !work.follows_the_rules() => {
            lines.push("Check: not run.".to_string())
        }
        None => lines.push("Check: none is set for this order.".to_string()),
    }
    lines.push(format!(
        "Review with: git -C {} diff {}..HEAD",
        repo.worktree.display(),
        order.base
    ));
    if !passed {
        lines.push("Send it back (send_order again) or escalate; do not accept it.".to_string());
    }
    lines.join("\n")
}

/// The leader accepts a reported step.
/// A step done in a lane merges into the task's branch first; a clash leaves
/// it waiting for a review, with the files named.
pub fn accept_step(task: &Task, step: &str) -> Result<()> {
    let state = task.state()?;
    let reported = state
        .steps
        .iter()
        .any(|found| found.id == step && found.status == StepStatus::Reported);
    let lane = state
        .orders
        .iter()
        .rev()
        .find(|order| order.step == step)
        .and_then(|order| state.agent(&order.agent))
        .and_then(|agent| agent.lane.as_deref());
    if let (true, Some(lane)) = (reported, lane) {
        crate::lanes::merge_into_task(task, &crate::lanes::get(task, lane)?)?;
    }
    task.transaction(|tx| {
        let found = tx
            .state
            .step_mut(step)
            .with_context(|| format!("there is no step {step}"))?;
        if found.status != StepStatus::Reported {
            bail!(
                "step {step} is {:?}, not waiting for a review",
                found.status
            );
        }
        found.set_status(StepStatus::Accepted, crate::log::now());
        tx.log(Kind::Review, None, Some(step), "accepted");
        Ok(())
    })
}

/// The leader sends a reported step back.
pub fn reject_step(task: &Task, step: &str, reason: &str) -> Result<()> {
    task.transaction(|tx| {
        let found = tx
            .state
            .step_mut(step)
            .with_context(|| format!("there is no step {step}"))?;
        if found.status != StepStatus::Reported {
            bail!(
                "step {step} is {:?}, not waiting for a review",
                found.status
            );
        }
        found.set_status(StepStatus::Rejected, crate::log::now());
        tx.log(
            Kind::Review,
            None,
            Some(step),
            &format!("rejected: {reason}"),
        );
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::tests::{commit_file, repo};
    use crate::home::Home;
    use crate::state::Agent;
    use crate::task::TaskConfig;
    use std::fs;
    use tempfile::TempDir;

    struct Fixture {
        _home: TempDir,
        repo: TempDir,
        task: Task,
    }

    /// A task on a real repo with one live agent, `backend-1`, in pane %5,
    /// on the factory floor, and a senior's accepted plan, step `p0` (the
    /// first step, so the steps ordered come after it).
    fn fixture(check: Option<&str>) -> Fixture {
        let home_dir = TempDir::new().unwrap();
        let repo = repo();
        let config = TaskConfig {
            id: "t".to_string(),
            branch: "feat/x".to_string(),
            workspace: "dev-app".to_string(),
            max_agents: 3,
            lanes: Vec::new(),
            repos: vec![Repo {
                name: "app".to_string(),
                worktree: repo.path().to_path_buf(),
                check: check.map(str::to_string),
            }],
        };
        let task = Task::create(&Home::at(home_dir.path().to_path_buf()), &config).unwrap();
        task.transaction(|tx| {
            tx.state.agents.push(Agent {
                id: "backend-1".to_string(),
                role: "executor".to_string(),
                agent: "claude".to_string(),
                repo: "app".to_string(),
                session: "dev-app".to_string(),
                pane: "%5".to_string(),
                writes: true,
                group: None,
                lane: None,
                live: true,
                ready: true,
                lost: false,
                usage: None,
            });
            tx.state.steps.push(Step {
                id: "p0".to_string(),
                title: "plan the login".to_string(),
                status: StepStatus::Accepted,
                attempts: 1,
                review_since: None,
            });
            tx.state.plans.push(crate::state::ExecutionPlan {
                step: "p0".to_string(),
                author: "planner-1".to_string(),
            });
            Ok(())
        })
        .unwrap();
        Fixture {
            _home: home_dir,
            repo,
            task,
        }
    }

    fn request(step: &str) -> OrderRequest {
        OrderRequest {
            agent: "backend-1".to_string(),
            step: step.to_string(),
            title: "Add the login".to_string(),
            text: "Write src/login.rs".to_string(),
            allowed: vec!["src/**".to_string()],
            forbidden: vec!["src/secret/".to_string()],
            check: None,
            plan: Some("p0".to_string()),
        }
    }

    #[test]
    fn the_floor_only_runs_a_seniors_accepted_plan() {
        let f = fixture(None);
        let error = send_order(
            &f.task,
            OrderRequest {
                plan: None,
                ..request("s1")
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("factory floor"), "{error}");
        let error = send_order(
            &f.task,
            OrderRequest {
                plan: Some("s9".into()),
                ..request("s1")
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("no execution plan"), "{error}");
        f.task
            .transaction(|tx| {
                tx.state.step_mut("p0").unwrap().status = StepStatus::Reported;
                Ok(())
            })
            .unwrap();
        let error = send_order(&f.task, request("s1")).unwrap_err().to_string();
        assert!(error.contains("not accepted yet"), "{error}");
        f.task
            .transaction(|tx| {
                tx.state.step_mut("p0").unwrap().status = StepStatus::Accepted;
                Ok(())
            })
            .unwrap();
        send_order(&f.task, request("s1")).unwrap();
        let typed = &f.task.state().unwrap().queue[0].text;
        assert!(
            typed.contains("devagentics plan show p0") && typed.contains("devagentics ask"),
            "{typed}"
        );
    }

    fn status(fixture: &Fixture, step: &str) -> StepStatus {
        fixture
            .task
            .state()
            .unwrap()
            .steps
            .iter()
            .find(|candidate| candidate.id == step)
            .unwrap()
            .status
    }

    fn told_the_leader(fixture: &Fixture) -> String {
        fixture
            .task
            .state()
            .unwrap()
            .queue
            .iter()
            .filter(|message| message.to == LEADER)
            .map(|m| m.text.clone())
            .collect::<Vec<_>>()
            .join("\n---\n")
    }

    const SHORT: Duration = Duration::from_secs(10);

    #[test]
    fn an_order_is_recorded_from_the_current_commit_and_queued_for_the_agent() {
        let f = fixture(Some("true"));
        let id = send_order(&f.task, request("s1")).unwrap();

        let state = f.task.state().unwrap();
        let order = state.open_order_of("backend-1").unwrap();
        assert_eq!(order.id, id);
        assert_eq!(order.base, checks::head(f.repo.path()).unwrap());
        assert_eq!(
            order.check.as_deref(),
            Some("true"),
            "the repo's check is the default"
        );
        assert_eq!(state.steps[1].status, StepStatus::Ordered);
        assert_eq!(state.steps[1].attempts, 1);

        let message = &state.queue[0];
        assert_eq!(message.to, "backend-1");
        for wanted in [
            "ORDER o1",
            "step s1: Add the login",
            "Write src/login.rs",
            "src/**",
            "src/secret/",
            "devagentics report done",
        ] {
            assert!(
                message.text.contains(wanted),
                "the order does not say {wanted}: {}",
                message.text
            );
        }
        assert!(f.task.log_lines().unwrap()[0].contains("\"kind\":\"order\""));
    }

    #[test]
    fn an_agent_has_one_order_at_a_time_and_only_live_agents_get_one() {
        let f = fixture(None);
        send_order(&f.task, request("s1")).unwrap();
        let error = send_order(&f.task, request("s2")).unwrap_err().to_string();
        assert!(error.contains("still has order"), "{error}");

        let mut stranger = request("s3");
        stranger.agent = "nobody".to_string();
        assert!(send_order(&f.task, stranger).is_err());

        f.task
            .transaction(|tx| {
                tx.state.agents[0].live = false;
                Ok(())
            })
            .unwrap();
        assert!(
            send_order(&f.task, request("s4")).is_err(),
            "a stopped agent cannot take an order"
        );
    }

    #[test]
    fn good_work_is_reported_to_the_leader_for_review() {
        let f = fixture(Some("test -f src/login.rs"));
        send_order(&f.task, request("s1")).unwrap();
        commit_file(f.repo.path(), "src/login.rs", "fn login() {}\n", "login");

        let told = report(&f.task, "%5", Outcome::Done, "added login", SHORT).unwrap();
        assert!(told.contains("review"), "{told}");
        assert_eq!(status(&f, "s1"), StepStatus::Reported);
        let state = f.task.state().unwrap();
        assert!(
            state
                .steps
                .iter()
                .any(|step| step.id == "s1" && step.review_since.is_some()),
            "the board ages a review from its report"
        );
        assert!(state.open_order_of("backend-1").is_none());

        let message = told_the_leader(&f);
        for wanted in [
            "backend-1 reports step s1 done.",
            "Report: added login",
            "Tree: clean",
            "rules kept; 1 changed (+1 −0)",
            "Check: passed",
            "diff ",
        ] {
            assert!(
                message.contains(wanted),
                "the leader is not told '{wanted}': {message}"
            );
        }
        assert!(!message.contains("FAILED"));
    }

    #[test]
    fn a_forbidden_file_fails_the_step_and_the_check_is_not_run() {
        let f = fixture(Some("true"));
        send_order(&f.task, request("s1")).unwrap();
        commit_file(f.repo.path(), "src/secret/key.rs", "x\n", "oops");
        commit_file(f.repo.path(), "docs/readme.md", "x\n", "docs");

        report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Failed);
        let message = told_the_leader(&f);
        assert!(
            message.contains("FORBIDDEN changed: src/secret/key.rs"),
            "{message}"
        );
        assert!(
            message.contains("OUTSIDE the allowed ones: docs/readme.md"),
            "{message}"
        );
        assert!(message.contains("Check: not run"), "{message}");
        assert!(message.contains("do not accept it"), "{message}");
        assert!(!f.task.dir().join("checks").exists());
    }

    #[test]
    fn uncommitted_work_fails_the_step() {
        let f = fixture(Some("true"));
        send_order(&f.task, request("s1")).unwrap();
        fs::write(f.repo.path().join("scratch.txt"), "x").unwrap();

        report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Failed);
        assert!(told_the_leader(&f).contains("NOT clean, uncommitted: scratch.txt"));
    }

    #[test]
    fn a_failing_check_fails_the_step_and_names_its_log() {
        let f = fixture(Some("echo nope; exit 4"));
        send_order(&f.task, request("s1")).unwrap();
        commit_file(f.repo.path(), "src/a.rs", "x\n", "a");

        report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Failed);
        let message = told_the_leader(&f);
        assert!(message.contains("Check: FAILED (exit 4"), "{message}");
        let log = f.task.dir().join("checks/s1-1.log");
        assert!(message.contains(&log.display().to_string()), "{message}");
        assert_eq!(fs::read_to_string(log).unwrap(), "nope\n");
    }

    #[test]
    fn an_order_can_name_its_own_check_and_a_missing_one_is_said() {
        let f = fixture(Some("exit 1"));
        let mut order = request("s1");
        order.check = Some("true".to_string());
        send_order(&f.task, order).unwrap();
        commit_file(f.repo.path(), "src/a.rs", "x\n", "a");
        report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert_eq!(
            status(&f, "s1"),
            StepStatus::Reported,
            "the order's check replaced the repo's"
        );

        let none = fixture(None);
        send_order(&none.task, request("s1")).unwrap();
        commit_file(none.repo.path(), "src/a.rs", "x\n", "a");
        report(&none.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert!(told_the_leader(&none).contains("Check: none is set"));
    }

    #[test]
    fn a_blocked_agent_tells_the_leader_and_the_step_waits() {
        let f = fixture(None);
        send_order(&f.task, request("s1")).unwrap();
        report(
            &f.task,
            "%5",
            Outcome::Blocked,
            "the schema is missing",
            SHORT,
        )
        .unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Blocked);
        assert!(
            told_the_leader(&f).contains("backend-1 is blocked on step s1: the schema is missing")
        );
    }

    #[test]
    fn only_a_team_agent_with_an_open_order_can_report() {
        let f = fixture(None);
        assert!(
            report(&f.task, "%9", Outcome::Done, "x", SHORT)
                .unwrap_err()
                .to_string()
                .contains("not a team agent")
        );
        assert!(
            report(&f.task, "%5", Outcome::Done, "x", SHORT)
                .unwrap_err()
                .to_string()
                .contains("no open order")
        );
    }

    #[test]
    fn the_leader_accepts_or_rejects_only_a_reported_step() {
        let f = fixture(None);
        send_order(&f.task, request("s1")).unwrap();
        assert!(
            accept_step(&f.task, "s1").is_err(),
            "a step still being worked on cannot be accepted"
        );
        commit_file(f.repo.path(), "src/a.rs", "x\n", "a");
        report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();

        reject_step(&f.task, "s1", "use the existing helper").unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Rejected);
        assert!(
            accept_step(&f.task, "s1").is_err(),
            "a rejected step is not waiting for a review"
        );
        assert!(accept_step(&f.task, "nope").is_err());

        // Sent back: ordered again, and it counts as another attempt.
        send_order(&f.task, request("s1")).unwrap();
        assert_eq!(f.task.state().unwrap().steps[1].attempts, 2);
        commit_file(f.repo.path(), "src/b.rs", "x\n", "b");
        report(&f.task, "%5", Outcome::Done, "done again", SHORT).unwrap();
        accept_step(&f.task, "s1").unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Accepted);
        assert!(
            send_order(&f.task, request("s1"))
                .unwrap_err()
                .to_string()
                .contains("already accepted")
        );

        let log = f.task.log_lines().unwrap().join("\n");
        assert!(log.contains("rejected: use the existing helper") && log.contains("accepted"));
    }

    #[test]
    fn a_step_in_review_or_in_progress_cannot_be_ordered_again() {
        let f = fixture(None);
        send_order(&f.task, request("s1")).unwrap();
        f.task
            .transaction(|tx| {
                tx.state.orders[0].status = OrderStatus::Closed;
                Ok(())
            })
            .unwrap();
        assert!(
            send_order(&f.task, request("s1"))
                .unwrap_err()
                .to_string()
                .contains("being worked on")
        );
        f.task
            .transaction(|tx| {
                tx.state.steps[1].status = StepStatus::Reported;
                Ok(())
            })
            .unwrap();
        assert!(
            send_order(&f.task, request("s1"))
                .unwrap_err()
                .to_string()
                .contains("waiting for your review")
        );
    }
}
