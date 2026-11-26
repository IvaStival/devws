//! Orders and what happens to them. The leader gives an agent one order at a
//! time; when the agent says it is done, devAgentics checks the work itself
//! (checks.rs) and tells the leader what it found; the leader accepts or
//! rejects the step.

use crate::checks::{self, CheckRun, Work};
use crate::log::Kind;
use crate::roles::Group;
use crate::state::{LEADER, Order, OrderStatus, State, Step, StepStatus};
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

#[derive(Clone)]
pub struct OrderRequest {
    pub agent: String,
    pub step: String,
    pub title: String,
    pub text: String,
    pub allowed: Vec<String>,
    pub forbidden: Vec<String>,
    pub check: Option<String>,
    pub profiles: Vec<String>,
    /// The senior step whose execution plan it follows; required for the
    /// factory floor.
    pub plan: Option<String>,
    /// The spec or plan part the step belongs to; kept when not given again.
    pub spec: Option<String>,
    /// The related files the worker reads first.
    pub read: Vec<String>,
    /// Where its checks run; none: everywhere.
    pub targets: Vec<String>,
    /// Why the step failed and what this order changes: required when it
    /// failed since its last order (failures.rs).
    pub diagnosis: Option<crate::failures::Diagnosis>,
    /// For a read-only agent: the lane whose work this order reads, unmerged
    /// work included, instead of the task branch.
    pub read_lane: Option<String>,
}

/// The attempts a step gets: the task's, or what the user allowed it.
pub fn attempt_limit(state: &State, step: &str, max_attempts: u32) -> u32 {
    state
        .attempt_limits
        .get(step)
        .copied()
        .unwrap_or(max_attempts)
}

/// Refuses another try at a step that has had `used` of its attempts: the
/// leader allows more when the next one can succeed (`allow_attempts`).
pub fn ensure_attempt_left(state: &State, used: u32, step: &str, max_attempts: u32) -> Result<()> {
    let limit = attempt_limit(state, step, max_attempts);
    if used >= limit {
        bail!(
            "step {step} has had {used} attempts, the most it is allowed: when the next one can \
             succeed, give it more with allow_attempts and say why; when it keeps failing the same \
             way, change the approach or close it"
        );
    }
    Ok(())
}

/// The user allows a step `limit` attempts in all, more than it has had
/// (`devagentics task attempts`); it also lets the next order past the
/// same-cause rule once.
pub fn allow_attempts(task: &Task, step: &str, limit: u32) -> Result<()> {
    set_attempt_limit(
        task,
        step,
        limit,
        format!("the user allowed {limit} attempts"),
    )?;
    task.transaction(|tx| {
        tx.state.diagnosis_waivers.insert(step.to_string());
        Ok(())
    })
}

/// The leader gives a step `limit` attempts in all, more than it has had,
/// when the next one can succeed: its reason goes in the log.
pub fn leader_allows_attempts(task: &Task, step: &str, limit: u32, reason: &str) -> Result<()> {
    let reason = reason.trim();
    anyhow::ensure!(!reason.is_empty(), "say why the next attempt can succeed");
    set_attempt_limit(
        task,
        step,
        limit,
        format!("the leader allowed {limit} attempts: {reason}"),
    )
}

fn set_attempt_limit(task: &Task, step: &str, limit: u32, what: String) -> Result<()> {
    task.transaction(|tx| {
        anyhow::ensure!(
            tx.state.steps.iter().any(|found| found.id == step),
            "there is no step {step}"
        );
        let used = crate::failures::work_failures(&tx.state, step);
        if limit <= used {
            bail!("step {step} has had {used} attempts already: allow more than that");
        }
        tx.state.attempt_limits.insert(step.to_string(), limit);
        tx.state.step_retries.remove(step);
        tx.log(Kind::Review, None, Some(step), &what);
        Ok(())
    })
}

/// Gives an agent an order: records it, and queues the message that tells the
/// agent (the dispatcher types it in when the agent can take it).
/// Paths the leader names for an order (`read`, `targets`): from the repo's
/// root, without `..`, and safe to hand a check command (they become
/// `DEVAGENTICS_TARGETS`, words of a shell line).
fn validate_paths(what: &str, paths: &[String]) -> Result<()> {
    anyhow::ensure!(paths.len() <= 50, "{what}: at most 50 paths");
    for path in paths {
        let safe = !path.is_empty()
            && path.len() <= 200
            && !path.starts_with(['/', '-'])
            && !path.split('/').any(|part| part == "..")
            && path
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._/@+-".contains(character));
        anyhow::ensure!(
            safe,
            "{what}: {path:?} is not a path from the repo's root (letters, digits, . _ / @ + -, no ..)"
        );
    }
    Ok(())
}

/// Where a read-only agent's next snapshot is taken from: the named lane's
/// worktree; or, after an order that read a lane, its repo's again (the task
/// branch). None leaves its source alone, as a reviewer's review checkout.
fn read_source(
    task: &Task,
    agent: &crate::state::Agent,
    read_lane: Option<&str>,
) -> Result<Option<std::path::PathBuf>> {
    if let Some(name) = read_lane {
        anyhow::ensure!(
            !agent.writes,
            "read_lane is for a read-only agent: {} writes in its own worktree",
            agent.id
        );
        let lane = crate::lanes::get(task, name)?;
        anyhow::ensure!(
            lane.repo == agent.repo,
            "read_lane {name} is in repo {}, not {}'s {}",
            lane.repo,
            agent.id,
            agent.repo
        );
        return Ok(Some(lane.worktree.canonicalize()?));
    }
    if agent.writes {
        return Ok(None);
    }
    let Some(current) = task
        .state()?
        .runner_workspaces
        .get(&agent.id)
        .map(|workspace| workspace.source.clone())
    else {
        return Ok(None);
    };
    let reads_a_lane = task.config()?.lanes.iter().any(|lane| {
        lane.worktree
            .canonicalize()
            .is_ok_and(|path| path == current)
    });
    if !reads_a_lane {
        return Ok(None);
    }
    Ok(Some(
        crate::lanes::workplace(task, agent)?
            .worktree
            .canonicalize()?,
    ))
}

/// Work in `lane` not merged yet whose files may overlap `allowed`, with its
/// writer: two seats write at once only apart. An order for `step` itself
/// is left out: a new order for that step takes its work over.
pub fn overlapping_orders<'s>(
    state: &'s crate::state::State,
    lane: &str,
    step: &str,
    allowed: &[String],
) -> Vec<(&'s crate::state::Order, &'s crate::state::Agent)> {
    state
        .orders
        .iter()
        .enumerate()
        .filter_map(|(index, order)| {
            let author = state.agent(&order.agent)?;
            let in_lane = author.writes && author.lane.as_deref() == Some(lane);
            // A step's newest order holds its work; older ones were retried.
            let newest = !state.orders[index + 1..]
                .iter()
                .any(|later| later.step == order.step);
            let merged = state.steps.iter().any(|found| {
                found.id == order.step
                    && matches!(found.status, StepStatus::Accepted | StepStatus::Closed)
            });
            (in_lane
                && newest
                && !merged
                && order.step != step
                && !checks::disjoint(allowed, &order.allowed))
            .then_some((order, author))
        })
        .collect()
}

/// The first such work in another seat than the writer's: its own seat's
/// work is in the same worktree, one step after the other.
fn overlapping_work<'s>(
    state: &'s crate::state::State,
    writer: &crate::state::Agent,
    step: &str,
    allowed: &[String],
) -> Option<&'s crate::state::Order> {
    overlapping_orders(state, writer.lane.as_deref()?, step, allowed)
        .into_iter()
        .find(|(_, author)| author.seat.max(1) != writer.seat.max(1))
        .map(|(order, _)| order)
}

fn globs_text(globs: &[String]) -> String {
    if globs.is_empty() {
        "any file".to_string()
    } else {
        globs.join(", ")
    }
}

pub fn send_order(task: &Task, mut request: OrderRequest) -> Result<String> {
    validate_paths("read", &request.read)?;
    validate_paths("targets", &request.targets)?;
    let _control = task.control_lock()?;
    crate::workflow::block_unverified_runner(task)?;
    let agent = task
        .state()?
        .agent(&request.agent)
        .filter(|agent| agent.live)
        .cloned()
        .with_context(|| format!("there is no live agent {}", request.agent))?;
    let max_attempts = task.config()?.max_attempts;
    let read_source = read_source(task, &agent, request.read_lane.as_deref())?;
    let reviewed = task.state()?.workflow == crate::execution::Workflow::Reviewed;
    let contract = if reviewed && agent.writes {
        let state = task.state()?;
        let item = state.application.ready(&request.step, &state)?.clone();
        ensure_contract(task, &agent, &item)?;
        anyhow::ensure!(
            request.check.is_none(),
            "reviewed assignments use named checks from the accepted contract"
        );
        anyhow::ensure!(
            request.allowed.is_empty() || request.allowed == item.allowed,
            "assignment scope differs from accepted contract"
        );
        anyhow::ensure!(
            request.profiles.is_empty() || request.profiles == item.profiles,
            "assignment check profiles differ from accepted contract"
        );
        request.allowed = item.allowed.clone();
        request.profiles = item.profiles.clone();
        request.forbidden = state.application.order_forbidden(&item, &request.forbidden);
        Some(item)
    } else {
        None
    };
    // A lane's agent: its lane takes the other lanes' accepted work first,
    // and its files are the lane's unless the order narrows them.
    let lane = crate::lanes::of_agent(task, &agent)?;
    if let Some(lane) = &lane
        && task.state()?.open_order_of(&agent.id).is_none()
    {
        crate::lanes::catch_up(task, lane)?;
    }
    let repo = crate::lanes::workplace(task, &agent)?;
    if contract.is_some() {
        crate::runner::preflight(task, &request.step, &repo.worktree)?;
    }
    let base = checks::head(&repo.worktree)?;
    let mut profiles = task
        .config()?
        .check_profiles
        .get(&agent.repo)
        .cloned()
        .unwrap_or_default();
    if !request.profiles.is_empty() {
        profiles.selected = request.profiles.clone();
    }
    crate::check_profiles::validate(&profiles)?;
    let check = request.check.clone().or(repo.check.clone());
    if check.is_some() && !profiles.selected.is_empty() {
        bail!(
            "this repo checks with named profiles ({}): leave out check, or choose profiles",
            profiles.selected.join(", ")
        );
    }
    let allowed = match &lane {
        Some(lane) if request.allowed.is_empty() => lane.globs.clone(),
        _ => request.allowed.clone(),
    };

    task.transaction(|tx| {
        if agent.group() == Group::Floor && !reviewed {
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
        if let Some(other) = overlapping_work(&tx.state, &agent, &request.step, &allowed) {
            bail!(
                "{} can't run next to {} ({}, {}) in the lane {}: their files may overlap ({} and {}). Send it after {} is accepted, or narrow the files so they part at a folder",
                request.step,
                other.id,
                other.step,
                other.agent,
                agent.lane.as_deref().unwrap_or_default(),
                globs_text(&allowed),
                globs_text(&other.allowed),
                other.step
            );
        }
        // Its snapshot is taken from there when the order is delivered
        // (runner::bind_order); with no order open, none is reading it.
        if let Some(source) = &read_source {
            let workspace = tx.state.runner_workspaces.get_mut(&agent.id);
            match workspace {
                Some(workspace) => workspace.source = source.clone(),
                None => bail!("read_lane needs the reviewed runner: {} has no snapshot", agent.id),
            }
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
        let id = format!("o{}", tx.state.next_number());
        if tx.state.steps.iter().any(|step| step.id == request.step) {
            let approach = crate::failures::Approach {
                agent: agent.id.clone(),
                plan: request.plan.clone(),
                contract: crate::failures::contract_hash(contract.as_ref())?,
                allowed: allowed.clone(),
                read: request.read.clone(),
                targets: request.targets.clone(),
            };
            crate::failures::resolve(
                &mut tx.state,
                &request.step,
                request.diagnosis.clone(),
                &approach,
                &id,
                crate::log::now(),
            )?;
            let used = crate::failures::work_failures(&tx.state, &request.step);
            ensure_attempt_left(&tx.state, used, &request.step, max_attempts)?;
            if let Some(diagnosis) = &request.diagnosis {
                tx.log(
                    Kind::Review,
                    None,
                    Some(&request.step),
                    &format!(
                        "diagnosed {:?}: {} → {}",
                        diagnosis.cause, diagnosis.evidence, diagnosis.change
                    ),
                );
            }
        }
        match tx.state.step_mut(&request.step) {
            Some(step) => {
                step.title = request.title.clone();
                step.set_status(StepStatus::Ordered, crate::log::now());
                step.attempts += 1;
                if request.spec.is_some() {
                    step.spec = request.spec.clone();
                }
            }
            None => tx.state.steps.push(Step {
                id: request.step.clone(),
                title: request.title.clone(),
                status: StepStatus::Ordered,
                attempts: 1,
                spec: request.spec.clone(),
                review_since: None,
            }),
        }

        // A senior plans and answers; any file it changes fails its report.
        let mut forbidden = request.forbidden.clone();
        if agent.group() == Group::Senior && !forbidden.iter().any(|glob| glob == "**") {
            forbidden.push("**".to_string());
        }
        // The interfaces travel in the order's text, so the worker reads the
        // exact spec it builds or uses, and a resent order keeps it.
        let interfaces = tx.state.application.interface_brief(&request.step);
        let order = Order {
            read: request.read.clone(),
            targets: request.targets.clone(),
            id: id.clone(),
            step: request.step.clone(),
            agent: agent.id.clone(),
            text: format!("{}{interfaces}", request.text),
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
            order_message(&order, &request.title, &profiles.selected),
            Some(order.id.clone()),
        );
        tx.log(
            Kind::Order,
            Some(&agent.id),
            Some(&order.step),
            &request.text,
        );
        tx.state.order_profiles.insert(id.clone(), profiles.selected.clone());
        if let Some(contract)=contract {tx.state.contract_bindings.insert(id.clone(),contract);}
        tx.state.orders.push(order);
        Ok(id)
    })
}

/// Changes authorization without replacing the implementation base or assignment.
pub fn amend(
    task: &Task,
    id: &str,
    additions: Vec<String>,
    reason: &str,
    source: &str,
) -> Result<String> {
    if reason.trim().is_empty() || additions.is_empty() {
        bail!("an amendment requires allowed paths and a reason");
    }
    if additions
        .iter()
        .any(|path| path.starts_with('/') || path.split('/').any(|part| part == ".."))
    {
        bail!("allowed paths must be relative to the repository");
    }
    checks::glob_set(&additions)?;
    let _control = task.control_lock()?;
    let config = task.config()?;
    task.transaction(|tx| {
        let order = tx
            .state
            .orders
            .iter()
            .find(|order| order.id == id && order.status == OrderStatus::Open)
            .context("no open order with that ID")?
            .clone();
        let agent = tx
            .state
            .agent(&order.agent)
            .context("order agent is missing")?;
        if tx.state.open_order_of(&agent.id).map(|open| &open.id) != Some(&order.id) {
            bail!("order {id} is no longer authoritative");
        }
        if order.allowed.is_empty() {
            return Ok("No change: this order already allows any file.".into());
        }
        if let Some(lane) = agent
            .lane
            .as_ref()
            .and_then(|name| config.lanes.iter().find(|lane| &lane.name == name))
        {
            for addition in &additions {
                let safe = checks::within(&lane.globs, addition)?;
                if !safe {
                    bail!("cannot prove {addition} is within lane {}", lane.name);
                }
            }
        }
        let mut added = additions;
        added.sort();
        added.dedup();
        added.retain(|path| !order.allowed.contains(path));
        if added.is_empty() {
            return Ok("No change: paths are already allowed.".into());
        }
        // Wider files must still be apart from another seat's work.
        let widened: Vec<String> = order.allowed.iter().chain(&added).cloned().collect();
        if let Some(other) = overlapping_work(&tx.state, agent, &order.step, &widened) {
            bail!(
                "{} can't widen to {} next to {} ({}, {}) in the same lane: their files may overlap. Amend it after {} is accepted",
                id,
                globs_text(&added),
                other.id,
                other.step,
                other.agent,
                other.step
            );
        }
        let revision = tx.state.revision(id) + 1;
        tx.state.order_revisions.insert(id.into(), revision);
        tx.state.check_evidence.remove(id);
        tx.state.amendments.push(crate::state::Amendment {
            order: id.into(),
            revision,
            allowed: added.clone(),
            reason: reason.into(),
            source: source.into(),
        });
        let current = tx
            .state
            .orders
            .iter_mut()
            .find(|candidate| candidate.id == id)
            .unwrap();
        current.allowed.extend(added);
        current.delivered = false;
        current.stalled = false;
        let current = current.clone();
        let title = tx
            .state
            .steps
            .iter()
            .find(|step| step.id == current.step)
            .context("order step missing")?
            .title
            .clone();
        tx.state
            .queue
            .retain(|message| message.order.as_deref() != Some(id));
        tx.state.say_about(
            &current.agent,
            order_message(
                &current,
                &title,
                tx.state
                    .order_profiles
                    .get(id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            ),
            Some(id.into()),
        );
        tx.log(
            Kind::Order,
            Some(&current.agent),
            Some(&current.step),
            &format!("amended {id} revision {revision}: {reason}"),
        );
        Ok(format!(
            "Amended {id}, revision {revision}; original base {} retained.",
            current.base
        ))
    })
}

/// What the agent is told.
pub(crate) fn order_message(order: &Order, title: &str, profiles: &[String]) -> String {
    let list = |globs: &[String], none: &str| {
        if globs.is_empty() {
            none.to_string()
        } else {
            globs.join(", ")
        }
    };
    let plan = order.plan.as_ref().map_or_else(String::new, |plan| {
        format!(
            "Follow the plan of step {plan}: `devagentics plan show {plan}` (in an isolated team: worker action=plan_read). \
             If something in it is unclear, ask: `devagentics ask \"<your question>\"` (worker action=ask), then end your turn and wait for the answer.\n\n"
        )
    });
    let scope = format!(
        "{}{}",
        if order.read.is_empty() {
            String::new()
        } else {
            format!(
                "Read these first; they are what you need to know beyond the files you change: {}\n",
                order.read.join(", ")
            )
        },
        if order.targets.is_empty() {
            String::new()
        } else {
            format!(
                "Your checks run on: {} (the full suite runs once at review)\n",
                order.targets.join(", ")
            )
        }
    );
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
         {files}{scope}\
         Done means your work is committed (a clean tree) and the check passes: {check}\n\
         When you are done: devagentics report done --order {id} \"<what you did>\" — or, if something stops you: \
         devagentics report blocked --order {id} \"<what stops you>\".",
        id = order.id,
        step = order.step,
        text = order.text,
        check = if profiles.is_empty() {
            order.check.clone().unwrap_or_else(|| "none is set".into())
        } else {
            format!(
                "named profiles: {} (devagentics check run --order {})",
                profiles.join(", "),
                order.id
            )
        },
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
    order_id: &str,
    outcome: Outcome,
    summary: &str,
    timeout: Duration,
) -> Result<String> {
    let state = task.state()?;
    let agent = state
        .agent_in_pane(pane)
        .with_context(|| format!("{pane} is not a team agent's pane"))?
        .clone();
    let order = authoritative(&state, &agent.id, order_id)?.clone();
    let revision = state.revision(order_id);
    let _operation = crate::check_profiles::operation(task, order_id)?;

    if outcome == Outcome::Blocked {
        return task.transaction(|tx| {
            validate_snapshot(&tx.state, &order, revision)?;
            close(tx, &order, StepStatus::Blocked);
            crate::failures::record(
                &mut tx.state,
                &order.step,
                Some(&order.id),
                crate::failures::FailureKind::AgentBlocked,
                summary,
                crate::log::now(),
            );
            tx.state.say(
                LEADER,
                format!(
                    "{} is blocked on step {}: {summary}\n{}",
                    agent.id,
                    order.step,
                    crate::failures::guidance(&order.step, Some(&order.id))
                ),
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
    let checked_head = checks::head(&repo.worktree)?;
    let work = checks::inspect(
        &repo.worktree,
        &order.base,
        &order.allowed,
        &order.forbidden,
    )?;
    let evidence = crate::check_profiles::execute(task, &order, timeout)?;
    let has_check =
        order.check.is_some() || crate::check_profiles::configuration(task, &order)?.is_some();
    let check = if has_check && work.uncommitted.is_empty() && work.follows_the_rules() {
        let failure = evidence
            .phases
            .iter()
            .find(|phase| phase.outcome != crate::check_profiles::Outcome::Passed);
        Some(CheckRun {
            command: order
                .check
                .clone()
                .unwrap_or_else(|| "named profiles".into()),
            passed: evidence.passed(),
            timed_out: failure
                .is_some_and(|phase| phase.outcome == crate::check_profiles::Outcome::Timeout),
            exit_code: if evidence.passed() {
                Some(0)
            } else {
                failure.and_then(|phase| phase.exit_code)
            },
            log: evidence
                .phases
                .first()
                .map(|phase| phase.log.clone())
                .unwrap_or_else(|| evidence.run.clone()),
            seconds: evidence.phases.iter().map(|phase| phase.seconds).sum(),
        })
    } else {
        None
    };

    let passed = evidence.passed()
        && work.uncommitted.is_empty()
        && work.follows_the_rules()
        && check.as_ref().is_none_or(|run| run.passed);
    let mut message = result_message(
        &agent.id,
        &order,
        summary,
        &work,
        check.as_ref(),
        &repo,
        passed,
    );
    message.push_str(&format!("\nEvidence: {}\n", evidence.run.display()));
    for phase in &evidence.phases {
        message.push_str(&format!(
            "{}: {:?}: {}\n",
            phase.profile, phase.outcome, phase.diagnostic
        ));
    }
    let _control = task.control_lock()?;
    task.transaction(|tx| {
        validate_snapshot(&tx.state, &order, revision)?;
        if checks::head(&repo.worktree)? != checked_head
            || checks::inspect(
                &repo.worktree,
                &order.base,
                &order.allowed,
                &order.forbidden,
            )? != work
        {
            bail!("HEAD changed during check; run the report again");
        }
        crate::check_profiles::validate_configuration(task, &tx.state, &order, &evidence)?;
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
        let mut told = message.clone();
        if !passed {
            crate::failures::record(
                &mut tx.state,
                &order.step,
                Some(&order.id),
                check_failure(&work, &evidence, check.as_ref()),
                &first_lines(&message),
                crate::log::now(),
            );
            told.push_str(&crate::failures::guidance(&order.step, Some(&order.id)));
        }
        tx.state.say(LEADER, told);
        Ok(if passed {
            "Reported to the leader, who will review it. Wait for the next order.".to_string()
        } else {
            "Reported, but the checks found problems: the leader has them. Wait for the next order."
                .to_string()
        })
    })
}

/// What kind of failure a report's checks found: the work's (tests, file
/// rules), maybe the work's (a timeout), or only the environment's.
fn check_failure(
    work: &checks::Work,
    evidence: &crate::check_profiles::Evidence,
    legacy: Option<&CheckRun>,
) -> crate::failures::FailureKind {
    use crate::check_profiles::Outcome;
    use crate::failures::FailureKind;
    let work_failed = !work.uncommitted.is_empty()
        || !work.follows_the_rules()
        || legacy.is_some_and(|run| !run.passed && !run.timed_out)
        || evidence.phases.iter().any(|phase| {
            matches!(
                phase.outcome,
                Outcome::TestFailure | Outcome::RequiredTestsSkipped | Outcome::FilePolicyFailure
            )
        });
    if work_failed {
        FailureKind::CheckFailed
    } else if legacy.is_some_and(|run| run.timed_out)
        || evidence
            .phases
            .iter()
            .any(|phase| phase.outcome == Outcome::Timeout)
    {
        FailureKind::CheckTimeout
    } else {
        FailureKind::CheckEnvironment
    }
}

/// The start of a long message, for a failure's summary.
fn first_lines(message: &str) -> String {
    message.lines().take(6).collect::<Vec<_>>().join(" | ")
}

pub(crate) fn authoritative<'a>(
    state: &'a crate::state::State,
    agent: &str,
    id: &str,
) -> Result<&'a Order> {
    if !state.agent(agent).is_some_and(|agent| agent.live) {
        bail!("order agent is no longer live");
    }
    let current = state
        .open_order_of(agent)
        .with_context(|| {
            format!("order {id} is not open ({agent} has no open order): a new order for its step runs its checks again")
        })?;
    if current.id != id || current.agent != agent || !current.delivered {
        bail!(
            "order {id} is stale, foreign or undelivered; current order is {}",
            current.id
        );
    }
    Ok(current)
}

pub(crate) fn validate_snapshot(
    state: &crate::state::State,
    order: &Order,
    revision: u64,
) -> Result<()> {
    let current = authoritative(state, &order.agent, &order.id)?;
    if current != order || state.revision(&order.id) != revision {
        bail!(
            "order {} changed during execution; retry with its current revision",
            order.id
        );
    }
    Ok(())
}

/// A read-only adviser's (a planner's) answer to its order: it wrote no code,
/// so there is nothing to check; the order closes and the leader hears it.
/// Without this, an order that asked for findings, not a baseline, ended in
/// a stall and the findings were lost.
pub fn report_findings(
    task: &Task,
    agent: &str,
    order_id: &str,
    outcome: Outcome,
    summary: &str,
) -> Result<String> {
    task.transaction(|tx| {
        let order = authoritative(&tx.state, agent, order_id)?.clone();
        let (status, verb) = match outcome {
            Outcome::Done => (StepStatus::Reported, "reports"),
            Outcome::Blocked => (StepStatus::Blocked, "is blocked"),
        };
        close(tx, &order, status);
        if outcome == Outcome::Blocked {
            crate::failures::record(
                &mut tx.state,
                &order.step,
                Some(&order.id),
                crate::failures::FailureKind::AgentBlocked,
                summary,
                crate::log::now(),
            );
        }
        tx.state.say(
            LEADER,
            format!(
                "{agent} {verb} on step {} (order {}): {summary}",
                order.step, order.id
            ),
        );
        tx.log(Kind::Report, Some(agent), Some(&order.step), summary);
        Ok("Reported to the leader. Wait for the next order.".to_string())
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
    let _control = task.control_lock()?;
    if crate::integration::recover(task, step)? {
        return Ok(());
    }
    crate::reviews::require_approval(task, step)?;
    let state = task.state()?;
    let reported = state
        .steps
        .iter()
        .any(|found| found.id == step && found.status == StepStatus::Reported);
    let author = state
        .orders
        .iter()
        .rev()
        .find(|order| order.step == step)
        .and_then(|order| state.agent(&order.agent));
    let lane = author
        .map(|author| crate::lanes::of_agent(task, author))
        .transpose()?
        .flatten();
    if let (true, Some(lane)) = (reported, lane) {
        crate::lanes::merge_into_task(task, &lane, step)?;
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
        if let Some(journal) = tx.state.integrations.get_mut(step) {
            journal.accepted = true;
        }
        tx.log(Kind::Review, None, Some(step), "accepted");
        Ok(())
    })?;
    // The step stays accepted when its record cannot be committed: the
    // leader reads why in the log.
    if let Err(error) = crate::history::save(task, Some(step)) {
        task.transaction(|tx| {
            tx.log(
                Kind::Review,
                None,
                Some(step),
                &format!("team history not saved: {error:#}"),
            );
            Ok(())
        })?;
    }
    Ok(())
}

/// The leader closes a step that waits for it (reported, check failed,
/// blocked or sent back) when its work is not needed or its report was used:
/// a read-only step's plan, preflight or audit. Nothing merges, and it counts
/// as neither accepted nor open. A writer's step is not closed (dropping
/// written work changes the scope: reject it or amend the baseline), nor one
/// another baseline item or a milestone needs accepted.
pub fn close_step(task: &Task, step: &str, reason: &str) -> Result<()> {
    let reason = reason.trim();
    anyhow::ensure!(!reason.is_empty(), "say why the step is closed");
    task.transaction(|tx| {
        let state = &tx.state;
        let found = state
            .steps
            .iter()
            .find(|found| found.id == step)
            .with_context(|| format!("there is no step {step}"))?;
        match found.status {
            StepStatus::Reported
            | StepStatus::Failed
            | StepStatus::Blocked
            | StepStatus::Rejected
            | StepStatus::Exhausted => {}
            StepStatus::Ordered => {
                bail!("step {step} is still being worked on: stop its agent first")
            }
            other => bail!("step {step} is {other:?}, not waiting for you"),
        }
        let author = state
            .orders
            .iter()
            .rev()
            .find(|order| order.step == step)
            .and_then(|order| state.agent(&order.agent));
        if author.is_some_and(|author| author.writes) {
            bail!(
                "step {step} was ordered to an agent that writes: reject it, or propose an amendment that drops it"
            );
        }
        let items = state
            .application
            .items
            .iter()
            .filter(|(_, item)| item.dependencies.iter().any(|dependency| dependency == step));
        let milestones = state
            .application
            .milestones
            .iter()
            .filter(|(_, milestone)| milestone.required.iter().any(|item| item == step));
        let dependents: Vec<&str> = items
            .map(|(id, _)| id.as_str())
            .chain(milestones.map(|(id, _)| id.as_str()))
            .collect();
        if !dependents.is_empty() {
            bail!(
                "{} depend on step {step}: accept it instead",
                dependents.join(", ")
            );
        }
        tx.state
            .step_mut(step)
            .context("the step went away")?
            .set_status(StepStatus::Closed, crate::log::now());
        tx.log(Kind::Review, None, Some(step), &format!("closed: {reason}"));
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
        let last = tx
            .state
            .orders
            .iter()
            .rev()
            .find(|order| order.step == step)
            .map(|order| order.id.clone());
        crate::failures::record(
            &mut tx.state,
            step,
            last.as_deref(),
            crate::failures::FailureKind::LeaderRejected,
            reason,
            crate::log::now(),
        );
        tx.log(
            Kind::Review,
            None,
            Some(step),
            &format!("rejected: {reason}"),
        );
        Ok(())
    })
}

/// What a contract may write stays in its lane; the files it only reads
/// (`supporting_files`) need to be in the runner's source, not in the lane:
/// a docs step reads the code it documents.
fn writes_within_lane(lane: &[String], item: &crate::execution::Contract) -> Result<()> {
    for path in &item.allowed {
        anyhow::ensure!(
            checks::within(lane, path)?,
            "planned file scope exceeds lane: {path}"
        );
    }
    Ok(())
}

pub(crate) fn ensure_contract(
    task: &Task,
    agent: &crate::state::Agent,
    item: &crate::execution::Contract,
) -> Result<()> {
    anyhow::ensure!(
        item.role == agent.role
            && item.repo == agent.repo
            && agent.lane.as_deref() == Some(item.lane.as_str()),
        "specialist/repository/lane does not match accepted contract"
    );
    let lane = crate::lanes::get(task, &item.lane)?;
    let state = task.state()?;
    let runner = state
        .runner
        .as_ref()
        .context("isolated runner not configured")?;
    for path in item.allowed.iter().chain(&item.supporting_files) {
        anyhow::ensure!(
            checks::within(&runner.source_paths, path)?,
            "planned source path is unavailable in the isolated runner"
        );
    }
    // The implementer reads its protected tests: they must be in its source
    // snapshot, which leaves the runner's protected paths out entirely.
    for path in item
        .protected_tests
        .iter()
        .chain(state.application.existing_tests(&item.repo))
    {
        anyhow::ensure!(
            checks::within(&runner.source_paths, path)?
                && !checks::within(&runner.protected_paths, path)?,
            "protected tests {path} must be runner source the implementer can read"
        );
    }
    writes_within_lane(&lane.globs, item)?;
    let profiles = task
        .config()?
        .check_profiles
        .get(&item.repo)
        .cloned()
        .context("configure named check profiles before dispatch")?;
    for name in &item.profiles {
        anyhow::ensure!(
            profiles.checks.contains_key(name),
            "unknown contract check {name}"
        );
    }
    Ok(())
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
            max_attempts: 3,
            max_seats: crate::task::DEFAULT_MAX_SEATS,
            check_profiles: Default::default(),
            lanes: Vec::new(),
            routing: Default::default(),
            budget: Default::default(),
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
                seat: 0,
                live: true,
                unavailable: false,
                ready: true,
                lost: false,
                usage: None,
            });
            tx.state.steps.push(Step {
                id: "p0".to_string(),
                title: "plan the login".to_string(),
                status: StepStatus::Accepted,
                attempts: 1,
                spec: None,
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
            diagnosis: None,
            read: Vec::new(),
            targets: Vec::new(),
            profiles: Vec::new(),
            agent: "backend-1".to_string(),
            step: step.to_string(),
            title: "Add the login".to_string(),
            text: "Write src/login.rs".to_string(),
            allowed: vec!["src/**".to_string()],
            forbidden: vec!["src/secret/".to_string()],
            check: None,
            plan: Some("p0".to_string()),
            spec: None,
            read_lane: None,
        }
    }

    #[test]
    fn a_read_only_agent_reads_a_lanes_unmerged_work_for_one_order() {
        // A planner re-planning a lane's step read only the task branch, where
        // the lane's work is not merged yet (system-observer, D2, 25 Nov).
        let f = fixture(None);
        let lane = crate::lanes::add(&f.task, "front", &["web/**".into()], None).unwrap();
        f.task
            .transaction(|tx| {
                tx.state.runner = Some(crate::runner::tests::configuration());
                let mut planner = tx.state.agents[0].clone();
                planner.id = "planner-1".into();
                planner.role = "planner".into();
                planner.writes = false;
                planner.group = Some(Group::Senior);
                planner.pane = "%6".into();
                tx.state.agents.push(planner);
                Ok(())
            })
            .unwrap();
        crate::runner::prepare(&f.task, "planner-1", f.repo.path(), true).unwrap();
        let source = |task: &Task| {
            task.state().unwrap().runner_workspaces["planner-1"]
                .source
                .clone()
        };
        let planning = |step: &str, read_lane: Option<&str>| OrderRequest {
            agent: "planner-1".into(),
            allowed: Vec::new(),
            forbidden: Vec::new(),
            plan: None,
            read_lane: read_lane.map(str::to_string),
            ..request(step)
        };

        send_order(&f.task, planning("p1", Some("front"))).unwrap();
        assert_eq!(source(&f.task), lane.worktree.canonicalize().unwrap());

        f.task
            .transaction(|tx| {
                tx.state.orders.last_mut().unwrap().status = OrderStatus::Closed;
                Ok(())
            })
            .unwrap();
        send_order(&f.task, planning("p2", None)).unwrap();
        assert_eq!(
            source(&f.task),
            f.repo.path().canonicalize().unwrap(),
            "the next order reads the task branch again"
        );

        let writer = OrderRequest {
            read_lane: Some("front".into()),
            ..request("s1")
        };
        let error = send_order(&f.task, writer).unwrap_err().to_string();
        assert!(error.contains("read_lane"), "{error}");
    }

    #[test]
    fn an_order_names_what_to_read_and_where_its_checks_run() {
        let f = fixture(None);
        let mut scoped = request("s1");
        scoped.read = vec!["src/session.rs".into()];
        scoped.targets = vec!["tests/login.rs".into()];
        let refused = |field: &str, path: &str| {
            let mut bad = scoped.clone();
            match field {
                "read" => bad.read = vec![path.into()],
                _ => bad.targets = vec![path.into()],
            }
            send_order(&f.task, bad).unwrap_err().to_string()
        };
        for path in ["../etc/passwd", "/abs", "a; rm -rf b", "-rf", "$(x)"] {
            assert!(refused("targets", path).contains("is not a path"), "{path}");
        }
        assert!(refused("read", "src/../x").contains("read:"));
        let id = send_order(&f.task, scoped).unwrap();
        let state = f.task.state().unwrap();
        let order = state.orders.iter().find(|order| order.id == id).unwrap();
        assert_eq!(order.targets, ["tests/login.rs"]);
        let message = state
            .queue
            .iter()
            .find(|message| message.order.as_deref() == Some(&id))
            .unwrap();
        assert!(message.text.contains("Read these first; they are what you need to know beyond the files you change: src/session.rs"), "{}", message.text);
        assert!(
            message.text.contains(
                "Your checks run on: tests/login.rs (the full suite runs once at review)"
            ),
            "{}",
            message.text
        );
    }

    #[test]
    fn the_floor_only_runs_a_seniors_accepted_plan() {
        let f = fixture(None);
        let error = send_order(
            &f.task,
            OrderRequest {
                diagnosis: None,
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
                diagnosis: None,
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

    fn delivered_report(
        task: &Task,
        pane: &str,
        outcome: Outcome,
        summary: &str,
        timeout: Duration,
    ) -> Result<String> {
        let id = task.transaction(|tx| {
            let agent = tx
                .state
                .agent_in_pane(pane)
                .context("not a team agent")?
                .id
                .clone();
            let id = tx
                .state
                .open_order_of(&agent)
                .context("no open order")?
                .id
                .clone();
            tx.state
                .orders
                .iter_mut()
                .find(|order| order.id == id)
                .unwrap()
                .delivered = true;
            Ok(id)
        })?;
        report(task, pane, &id, outcome, summary, timeout)
    }

    #[test]
    fn every_report_is_kept_in_the_teams_memory() {
        let f = fixture(None);
        let id = send_order(&f.task, request("s1")).unwrap();
        let agent = f
            .task
            .transaction(|tx| {
                tx.state.orders[0].delivered = true;
                let agent = tx
                    .state
                    .agents
                    .iter_mut()
                    .find(|agent| agent.pane == "%5")
                    .unwrap();
                // A read-only worker reports findings without checks.
                agent.writes = false;
                agent.role = "planner".into();
                Ok(agent.id.clone())
            })
            .unwrap();
        let folder = tempfile::TempDir::new().unwrap();
        let devws = crate::devws::fake::FakeDevws::new(folder.path()).with_memory();
        crate::worker_tools::call_in(
            &f.task,
            &devws,
            &agent,
            "report",
            &serde_json::json!({"outcome": "done", "summary": "H1-AT fits docs/**"}),
        )
        .unwrap();
        let notes = devws.notes();
        assert_eq!(notes.len(), 1, "{notes:?}");
        let (_, path, note) = &notes[0];
        assert!(path.starts_with("tasks/t/steps/s1/report-"), "{path}");
        assert_eq!(note.title, format!("s1 done · order {id}"));
        assert_eq!(note.body, "H1-AT fits docs/**");
        assert_eq!(note.author, agent);
    }

    #[test]
    fn a_planners_findings_close_its_order_and_reach_the_leader() {
        let f = fixture(None);
        let id = send_order(&f.task, request("s1")).unwrap();
        let agent = f
            .task
            .state()
            .unwrap()
            .agent_in_pane("%5")
            .unwrap()
            .id
            .clone();
        assert!(
            report_findings(&f.task, &agent, &id, Outcome::Done, "early").is_err(),
            "an undelivered order was reported"
        );
        f.task
            .transaction(|tx| {
                tx.state.orders[0].delivered = true;
                Ok(())
            })
            .unwrap();

        report_findings(&f.task, &agent, &id, Outcome::Done, "H1-AT fits docs/**").unwrap();

        let state = f.task.state().unwrap();
        assert_eq!(state.orders[0].status, OrderStatus::Closed);
        assert!(
            state
                .steps
                .iter()
                .any(|step| step.id == "s1" && step.status == StepStatus::Reported)
        );
        assert!(
            state
                .queue
                .iter()
                .any(|message| message.to == LEADER && message.text.contains("H1-AT fits docs/**")),
            "{:?}",
            state.queue
        );
        assert!(report_findings(&f.task, &agent, &id, Outcome::Done, "again").is_err());
    }

    #[test]
    fn a_contract_writes_only_in_its_lane_but_reads_outside_it() {
        let lane = vec!["docs/**".to_string(), "CLAUDE.md".to_string()];
        let mut item = crate::execution::Contract {
            allowed: vec!["docs/plans/x/acceptance/H1.md".into()],
            supporting_files: vec!["web/src/hooks/business/observe/useReplayPlayer.ts".into()],
            ..Default::default()
        };
        writes_within_lane(&lane, &item).unwrap();
        item.allowed.push("web/src/new.ts".into());
        let error = writes_within_lane(&lane, &item).unwrap_err().to_string();
        assert!(error.contains("exceeds lane: web/src/new.ts"), "{error}");
    }

    #[test]
    fn reports_reject_undelivered_and_obsolete_order_ids_without_mutation() {
        let f = fixture(None);
        let id = send_order(&f.task, request("s1")).unwrap();
        let before = f.task.state().unwrap();
        assert!(report(&f.task, "%5", &id, Outcome::Done, "done", SHORT).is_err());
        assert_eq!(f.task.state().unwrap(), before);
        delivered_report(&f.task, "%5", Outcome::Blocked, "blocked", SHORT).unwrap();
        let next = send_order(
            &f.task,
            diagnosed("s1", crate::failures::Cause::MissingContext),
        )
        .unwrap();
        f.task
            .transaction(|tx| {
                tx.state
                    .orders
                    .iter_mut()
                    .find(|order| order.id == next)
                    .unwrap()
                    .delivered = true;
                Ok(())
            })
            .unwrap();
        let before = f.task.state().unwrap();
        for outcome in [Outcome::Done, Outcome::Blocked] {
            assert!(report(&f.task, "%5", &id, outcome, "old", SHORT).is_err());
            assert_eq!(f.task.state().unwrap(), before);
        }
    }

    #[test]
    fn amendments_preserve_identity_base_and_forbidden_rules() {
        let f = fixture(Some("true"));
        let id = send_order(&f.task, request("s1")).unwrap();
        let original = f.task.state().unwrap().orders[0].clone();
        amend(
            &f.task,
            &id,
            vec!["tests/login.rs".into()],
            "approved regression coverage",
            "test leader",
        )
        .unwrap();
        let state = f.task.state().unwrap();
        assert_eq!(state.orders[0].id, original.id);
        assert_eq!(state.orders[0].base, original.base);
        assert_eq!(state.orders[0].forbidden, original.forbidden);
        assert_eq!(state.steps[1].attempts, 1);
        assert_eq!(state.revision(&id), 1);
        assert_eq!(state.amendments.len(), 1);
        assert_eq!(
            state
                .queue
                .iter()
                .filter(|message| message.order.as_deref() == Some(&id))
                .count(),
            1
        );
        amend(
            &f.task,
            &id,
            vec!["tests/login.rs".into()],
            "same",
            "test leader",
        )
        .unwrap();
        assert_eq!(f.task.state().unwrap().revision(&id), 1);
        assert!(amend(&f.task, &id, vec!["[".into()], "invalid", "test leader").is_err());
        commit_file(f.repo.path(), "tests/login.rs", "test", "test");
        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Reported);
    }

    #[test]
    fn amendment_during_a_check_cannot_certify_an_old_revision() {
        let f = fixture(None);
        let marker = f.task.dir().join("started");
        let release = f.task.dir().join("release");
        let mut request = request("s1");
        request.check = Some(format!(
            "touch '{}'; while test ! -f '{}'; do sleep 0.02; done",
            marker.display(),
            release.display()
        ));
        let id = send_order(&f.task, request).unwrap();
        f.task
            .transaction(|tx| {
                tx.state.orders[0].delivered = true;
                Ok(())
            })
            .unwrap();
        let task = f.task.clone();
        let report_id = id.clone();
        let worker = std::thread::spawn(move || {
            report(&task, "%5", &report_id, Outcome::Done, "done", SHORT)
        });
        for _ in 0..200 {
            if marker.is_file() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.is_file());
        amend(
            &f.task,
            &id,
            vec!["tests/login.rs".into()],
            "new authorization",
            "test leader",
        )
        .unwrap();
        fs::write(release, "go").unwrap();
        assert!(worker.join().unwrap().is_err());
        assert_eq!(status(&f, "s1"), StepStatus::Ordered);
        assert!(f.task.state().unwrap().check_evidence.is_empty());
    }

    #[test]
    fn order_and_check_errors_say_what_to_do() {
        let f = fixture(None);
        let profiles = serde_json::json!({"environments":{"testing":{}}, "checks":{"unit":{"command":"true","environment":"testing"}}, "selected":["unit"]});
        crate::check_profiles::configure(&f.task, "app", serde_json::from_value(profiles).unwrap())
            .unwrap();
        let mut with_check = request("s1");
        with_check.check = Some("true".into());
        let error = send_order(&f.task, with_check).unwrap_err().to_string();
        assert!(
            error.contains("named profiles (unit): leave out check"),
            "{error}"
        );

        let id = send_order(&f.task, request("s1")).unwrap();
        f.task
            .transaction(|tx| {
                tx.state.orders[0].status = OrderStatus::Closed;
                Ok(())
            })
            .unwrap();
        let error = crate::check_profiles::run(&f.task, &id, None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("order {id} is not open")),
            "{error}"
        );
        assert!(error.contains("a new order for its step"), "{error}");
    }

    #[test]
    fn named_evidence_reuse_requires_matching_head_revision_and_configuration() {
        let f = fixture(None);
        let profiles = serde_json::json!({"environments":{"testing":{}}, "checks":{"unit":{"command":"true","environment":"testing"}}, "selected":["unit"]});
        crate::check_profiles::configure(
            &f.task,
            "app",
            serde_json::from_value(profiles.clone()).unwrap(),
        )
        .unwrap();
        let id = send_order(&f.task, request("s1")).unwrap();
        f.task
            .transaction(|tx| {
                tx.state.orders[0].delivered = true;
                Ok(())
            })
            .unwrap();
        let first = crate::check_profiles::run(&f.task, &id, Some("%5")).unwrap();
        assert_eq!(
            first.run,
            crate::check_profiles::run(&f.task, &id, Some("%5"))
                .unwrap()
                .run
        );
        commit_file(f.repo.path(), "src/new.rs", "new", "new");
        let second = crate::check_profiles::run(&f.task, &id, Some("%5")).unwrap();
        assert_ne!(first.run, second.run);
        let mut changed = profiles;
        changed["checks"]["unit"]["command"] = "test -f src/new.rs".into();
        crate::check_profiles::configure(&f.task, "app", serde_json::from_value(changed).unwrap())
            .unwrap();
        let third = crate::check_profiles::run(&f.task, &id, Some("%5")).unwrap();
        assert_ne!(second.run, third.run);
        amend(
            &f.task,
            &id,
            vec!["tests/new.rs".into()],
            "coverage",
            "leader",
        )
        .unwrap();
        assert!(f.task.state().unwrap().check_evidence.is_empty());
        assert!(crate::check_profiles::run(&f.task, &id, Some("%5")).is_err());
        let _lock = crate::check_profiles::operation(&f.task, &id).unwrap();
        assert!(crate::check_profiles::run(&f.task, &id, None).is_err());
    }

    // A repo with a lane's own checks: the run's checks are the lane's view of
    // them, and comparing them with the repo's whole configuration made every
    // run stale ("check inputs changed; evidence is stale").
    #[test]
    fn a_check_in_a_task_with_lane_checks_records_its_evidence() {
        let f = fixture(None);
        crate::lanes::add(&f.task, "back", &["backend/**".into()], None).unwrap();
        let profiles = serde_json::json!({
            "environments": {"testing": {}},
            "checks": {"unit": {"command": "true", "environment": "testing"}},
            "selected": ["unit"],
            "lanes": {"back": {"unit": {"command": "true", "environment": "testing"}}},
        });
        crate::check_profiles::configure(&f.task, "app", serde_json::from_value(profiles).unwrap())
            .unwrap();
        let id = send_order(&f.task, request("s1")).unwrap();
        f.task
            .transaction(|tx| {
                tx.state.orders[0].delivered = true;
                Ok(())
            })
            .unwrap();

        let evidence = crate::check_profiles::run(&f.task, &id, Some("%5")).unwrap();
        assert!(evidence.passed());
        let state = f.task.state().unwrap();
        assert!(state.check_evidence.contains_key(&id));
        let order = state.orders.iter().find(|order| order.id == id).unwrap();
        crate::check_profiles::validate_configuration(&f.task, &state, order, &evidence)
            .expect("the evidence matches the checks the order runs");
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
    fn an_order_carries_the_specs_of_the_interfaces_its_item_uses() {
        let f = fixture(None);
        f.task
            .transaction(|tx| {
                let application = &mut tx.state.application;
                application.items.insert(
                    "s1".into(),
                    crate::execution::Contract {
                        consumes: vec!["auth-api".into()],
                        ..Default::default()
                    },
                );
                application.interfaces.insert(
                    "auth-api".into(),
                    crate::execution::Interface {
                        version: 2,
                        owner: "s0".into(),
                        kind: crate::execution::InterfaceKind::Http,
                        spec: "POST /login {email,password} -> 204 + Set-Cookie: session".into(),
                        check: None,
                    },
                );
                Ok(())
            })
            .unwrap();
        let id = send_order(&f.task, request("s1")).unwrap();
        let state = f.task.state().unwrap();
        let message = state
            .queue
            .iter()
            .find(|message| message.order.as_deref() == Some(&id))
            .unwrap();
        assert!(
            message
                .text
                .contains("You use interface auth-api v2 (Http)")
                && message.text.contains("POST /login {email,password} -> 204"),
            "{}",
            message.text
        );
    }

    #[test]
    fn a_step_keeps_its_spec_when_ordered_again_without_one() {
        let f = fixture(None);
        send_order(
            &f.task,
            OrderRequest {
                diagnosis: None,
                spec: Some("H2f".into()),
                ..request("s1")
            },
        )
        .unwrap();
        assert_eq!(
            f.task.state().unwrap().steps[1].spec.as_deref(),
            Some("H2f")
        );

        commit_file(f.repo.path(), "src/a.rs", "x\n", "a");
        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        reject_step(&f.task, "s1", "again").unwrap();
        send_order(
            &f.task,
            diagnosed("s1", crate::failures::Cause::ImplementationDefect),
        )
        .unwrap();
        assert_eq!(
            f.task.state().unwrap().steps[1].spec.as_deref(),
            Some("H2f"),
            "a retry without a spec keeps the first one"
        );
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

        let told = delivered_report(&f.task, "%5", Outcome::Done, "added login", SHORT).unwrap();
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

        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
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
        assert!(
            f.task
                .state()
                .unwrap()
                .check_evidence
                .values()
                .all(|evidence| evidence
                    .phases
                    .iter()
                    .all(|phase| phase.profile == "file-policy"))
        );
    }

    #[test]
    fn uncommitted_work_fails_the_step() {
        let f = fixture(Some("true"));
        send_order(&f.task, request("s1")).unwrap();
        fs::write(f.repo.path().join("scratch.txt"), "x").unwrap();

        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Failed);
        assert!(told_the_leader(&f).contains("NOT clean, uncommitted: scratch.txt"));
    }

    #[test]
    fn a_failing_check_fails_the_step_and_names_its_log() {
        let f = fixture(Some("echo nope; exit 4"));
        send_order(&f.task, request("s1")).unwrap();
        commit_file(f.repo.path(), "src/a.rs", "x\n", "a");

        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Failed);
        let message = told_the_leader(&f);
        assert!(message.contains("Check: FAILED (exit 4"), "{message}");
        let log = f
            .task
            .state()
            .unwrap()
            .check_evidence
            .values()
            .next()
            .unwrap()
            .phases[0]
            .log
            .clone();
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
        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert_eq!(
            status(&f, "s1"),
            StepStatus::Reported,
            "the order's check replaced the repo's"
        );

        let none = fixture(None);
        send_order(&none.task, request("s1")).unwrap();
        commit_file(none.repo.path(), "src/a.rs", "x\n", "a");
        delivered_report(&none.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        assert!(told_the_leader(&none).contains("Check: none is set"));
    }

    #[test]
    fn a_blocked_agent_tells_the_leader_and_the_step_waits() {
        let f = fixture(None);
        send_order(&f.task, request("s1")).unwrap();
        delivered_report(
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
            delivered_report(&f.task, "%9", Outcome::Done, "x", SHORT)
                .unwrap_err()
                .to_string()
                .contains("not a team agent")
        );
        assert!(
            delivered_report(&f.task, "%5", Outcome::Done, "x", SHORT)
                .unwrap_err()
                .to_string()
                .contains("no open order")
        );
    }

    /// Reports s1 done and sends it back, as a leader does with work it
    /// does not accept.
    /// An order after a failure, with the leader's diagnosis.
    fn diagnosed(step: &str, cause: crate::failures::Cause) -> OrderRequest {
        OrderRequest {
            diagnosis: Some(crate::failures::Diagnosis {
                cause,
                evidence: "the report and the diff show it".into(),
                change: "do it the other way".into(),
                premise: Some("both attempts trusted the same fixture: ran it alone".into()),
            }),
            ..request(step)
        }
    }

    fn send_back(f: &Fixture, file: &str) {
        commit_file(f.repo.path(), file, "x\n", file);
        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        reject_step(&f.task, "s1", "not yet").unwrap();
    }

    // Attempts are failures of the work: a rejection the leader diagnosed
    // as the work's counts, and the limit stops the next order.
    #[test]
    fn a_step_gets_at_most_the_tasks_attempts_until_more_are_allowed() {
        use crate::failures::Cause;
        let f = fixture(None);
        send_order(&f.task, request("s1")).unwrap();
        send_back(&f, "src/a.rs");
        for (cause, file) in [
            (Cause::ImplementationDefect, "src/b.rs"),
            (Cause::TestDefect, "src/c.rs"),
        ] {
            send_order(&f.task, diagnosed("s1", cause)).unwrap();
            assert_eq!(status(&f, "s1"), StepStatus::Ordered);
            send_back(&f, file);
        }

        let error = send_order(&f.task, diagnosed("s1", Cause::PlanDefect))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("3 attempts") && error.contains("allow_attempts"),
            "{error}"
        );
        assert_eq!(
            status(&f, "s1"),
            StepStatus::Rejected,
            "a refused order leaves the step as it was"
        );

        // The refused order kept nothing: two failures are diagnosed as the
        // work's, the third is diagnosed with the next order.
        assert!(
            allow_attempts(&f.task, "s1", 2).is_err(),
            "a limit the step already reached allows nothing"
        );
        allow_attempts(&f.task, "s1", 4).unwrap();
        send_order(&f.task, diagnosed("s1", Cause::PlanDefect)).unwrap();
        assert_eq!(f.task.state().unwrap().steps[1].attempts, 4, "orders");
        send_back(&f, "src/d.rs");
        assert!(send_order(&f.task, diagnosed("s1", Cause::MissingContext)).is_err());
        assert!(allow_attempts(&f.task, "nope", 9).is_err());
    }

    // A failure that was not the work's (the environment, devAgentics) costs
    // nothing: the step is ordered again as often as it takes.
    #[test]
    fn failures_that_are_not_the_works_use_no_attempts() {
        use crate::failures::Cause;
        let f = fixture(None);
        send_order(&f.task, request("s1")).unwrap();
        send_back(&f, "src/a.rs");
        for (round, cause) in [Cause::Environment, Cause::Devagentics, Cause::OtherWork]
            .into_iter()
            .cycle()
            .take(5)
            .enumerate()
        {
            send_order(&f.task, diagnosed("s1", cause)).unwrap();
            send_back(&f, &format!("src/round{round}.rs"));
        }
        let state = f.task.state().unwrap();
        assert_eq!(crate::failures::work_failures(&state, "s1"), 0);
        send_order(&f.task, diagnosed("s1", Cause::Environment)).unwrap();
    }

    // A failed step is not ordered again blind: the order says why it failed.
    #[test]
    fn a_reorder_after_a_failed_step_needs_a_diagnosis() {
        let f = fixture(None);
        send_order(&f.task, request("s1")).unwrap();
        send_back(&f, "src/a.rs");
        let error = send_order(&f.task, request("s1")).unwrap_err().to_string();
        assert!(error.contains("diagnosis"), "{error}");
        assert_eq!(status(&f, "s1"), StepStatus::Rejected);
        send_order(
            &f.task,
            diagnosed("s1", crate::failures::Cause::ImplementationDefect),
        )
        .unwrap();
        let log = f.task.log_lines().unwrap().join("\n");
        assert!(log.contains("diagnosed ImplementationDefect"), "{log}");
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
        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();

        reject_step(&f.task, "s1", "use the existing helper").unwrap();
        assert_eq!(status(&f, "s1"), StepStatus::Rejected);
        assert!(
            accept_step(&f.task, "s1").is_err(),
            "a rejected step is not waiting for a review"
        );
        assert!(accept_step(&f.task, "nope").is_err());

        // Sent back: ordered again with the leader's diagnosis.
        send_order(
            &f.task,
            diagnosed("s1", crate::failures::Cause::ImplementationDefect),
        )
        .unwrap();
        assert_eq!(f.task.state().unwrap().steps[1].attempts, 2);
        commit_file(f.repo.path(), "src/b.rs", "x\n", "b");
        delivered_report(&f.task, "%5", Outcome::Done, "done again", SHORT).unwrap();
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

    fn read_only(f: &Fixture) {
        f.task
            .transaction(|tx| {
                tx.state.agents[0].writes = false;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn the_leader_closes_a_waiting_step_whose_report_it_used() {
        let f = fixture(None);
        read_only(&f);
        send_order(&f.task, request("s1")).unwrap();
        assert!(
            close_step(&f.task, "s1", "plan read and followed")
                .unwrap_err()
                .to_string()
                .contains("stop"),
            "a step still being worked on cannot be closed"
        );
        delivered_report(&f.task, "%5", Outcome::Done, "plan written", SHORT).unwrap();
        assert!(
            close_step(&f.task, "s1", " ").is_err(),
            "closing needs a reason"
        );

        close_step(&f.task, "s1", "plan read and followed").unwrap();
        let state = f.task.state().unwrap();
        let step = state.steps.iter().find(|step| step.id == "s1").unwrap();
        assert_eq!(step.status, StepStatus::Closed);
        assert_eq!(step.review_since, None, "it no longer waits for the leader");
        assert!(close_step(&f.task, "s1", "again").is_err());
        assert!(close_step(&f.task, "nope", "gone").is_err());
        let log = f.task.log_lines().unwrap().join("\n");
        assert!(log.contains("closed: plan read and followed"), "{log}");
    }

    #[test]
    fn a_writers_step_is_not_closed() {
        let f = fixture(None);
        send_order(&f.task, request("s1")).unwrap();
        commit_file(f.repo.path(), "src/a.rs", "x\n", "a");
        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();

        let error = close_step(&f.task, "s1", "not needed")
            .unwrap_err()
            .to_string();
        assert!(error.contains("writes"), "{error}");
        assert_eq!(status(&f, "s1"), StepStatus::Reported);
    }

    #[test]
    fn a_step_another_item_depends_on_is_not_closed() {
        let f = fixture(None);
        read_only(&f);
        f.task
            .transaction(|tx| {
                tx.state.application.items.insert(
                    "s2".to_string(),
                    crate::execution::Contract {
                        dependencies: vec!["s1".to_string()],
                        ..Default::default()
                    },
                );
                Ok(())
            })
            .unwrap();
        send_order(&f.task, request("s1")).unwrap();
        delivered_report(&f.task, "%5", Outcome::Done, "plan written", SHORT).unwrap();

        let error = close_step(&f.task, "s1", "superseded")
            .unwrap_err()
            .to_string();
        assert!(error.contains("s2"), "{error}");
        assert_eq!(status(&f, "s1"), StepStatus::Reported);
    }

    #[test]
    fn accepting_a_step_commits_the_teams_history() {
        let f = fixture(None);
        crate::checks::tests::run(f.repo.path(), &["config", "user.name", "t"]);
        crate::checks::tests::run(f.repo.path(), &["config", "user.email", "t@t"]);
        send_order(&f.task, request("s1")).unwrap();
        commit_file(f.repo.path(), "src/a.rs", "x\n", "a");
        delivered_report(&f.task, "%5", Outcome::Done, "done", SHORT).unwrap();
        accept_step(&f.task, "s1").unwrap();
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(f.repo.path())
            .args(["log", "-1", "--format=%s"])
            .output()
            .unwrap();
        let subject = String::from_utf8_lossy(&output.stdout);
        assert!(subject.starts_with("chore(team): record "), "{subject}");
        assert!(subject.contains("Add the login"), "{subject}");
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
