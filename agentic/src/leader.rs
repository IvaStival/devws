//! The leader's program. The leader keeps the team going, so it has two
//! programs: its role's and its role's fallback (`[fallback]` in leader.md).
//! When the account it runs on has used most of its week, it moves to the
//! other program if that one's week has room; when only its 5-hour window is
//! used up, it moves only if the other has room in both (`reason_to_move`):
//! at start, or, while it runs, by a handover once it has finished its turn. It also moves
//! when asked (the leader's `switch_leader`, `devagentics leader switch`).
//! Its conversation does not go with it: the new leader catches up from
//! task_status and the log.

use crate::devws::{Devws, Limits, PaneState};
use crate::home::Home;
use crate::log::Kind;
use crate::roles::{Fallback, Role, Roles};
use crate::start::write_leader_args;
use crate::state::State;
use crate::task::Task;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::fmt;

/// An account that has used this much of its 5-hour window or of its week
/// counts as run out.
pub const LIMIT_PERCENT: u32 = 95;

/// Why the other program leads when nobody said why.
const ASKED: &str = "the user asked";

/// The window of an account's plan that ran out, and how much of it is used.
#[derive(Debug, PartialEq)]
struct RunOut {
    window: &'static str,
    used: u32,
}

impl fmt::Display for RunOut {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}% of its {}", self.used, self.window)
    }
}

/// The fullest of the account's windows past the limit, if one is.
fn run_out(devws: &dyn Devws, agent: &str) -> Result<Option<RunOut>> {
    let limits = devws.limits(agent)?;
    Ok([("5-hour window", limits.five_hour), ("week", limits.week)]
        .into_iter()
        .filter_map(|(window, used)| past_limit(used).map(|used| RunOut { window, used }))
        .max_by_key(|out| out.used))
}

fn past_limit(used: Option<u32>) -> Option<u32> {
    used.filter(|used| *used >= LIMIT_PERCENT)
}

/// Why the leader leaves the account it runs on (`now`) for the other one,
/// if it does. The week comes first: it takes days to come back, the 5-hour
/// window hours. A week used up moves the leader to any account whose week is
/// not, even one waiting out its 5-hour window. A 5-hour window used up moves
/// it only to an account with room in both. So the leader never moves back
/// and forth: an account it left for its week never qualifies again.
fn reason_to_move(now: Limits, other: Limits) -> Option<RunOut> {
    if let Some(used) = past_limit(now.week) {
        return past_limit(other.week).is_none().then_some(RunOut {
            window: "week",
            used,
        });
    }
    let used = past_limit(now.five_hour)?;
    (past_limit(other.week).is_none() && past_limit(other.five_hour).is_none()).then_some(RunOut {
        window: "5-hour window",
        used,
    })
}

fn program(role: &Role) -> String {
    match &role.model {
        Some(model) => format!("{} ({model})", role.agent),
        None => role.agent.clone(),
    }
}

/// The program the leader runs on now and the other one; `None` when its role
/// has no fallback.
fn now_and_other(role: &Role, state: &State) -> Option<(Role, Role)> {
    let fallback = role.on_fallback()?;
    let own = Role {
        fallback: None,
        ..role.clone()
    };
    if fallback.agent != role.agent && state.leader_agent.as_deref() == Some(&fallback.agent) {
        Some((fallback, own))
    } else {
        Some((own, fallback))
    }
}

/// A team is run from a program that takes the leader's MCP tools (its
/// provider's can_lead); opencode works on the floor and among the seniors.
pub fn refuse_non_leader(agent: &str, kind: crate::providers::Kind) -> Result<()> {
    if !kind.provider().can_lead() {
        let leaders: Vec<_> = crate::providers::Kind::ALL
            .into_iter()
            .filter(|kind| kind.provider().can_lead())
            .map(crate::providers::Kind::name)
            .collect();
        bail!(
            "{agent} runs {}, which does not lead a team: pick an agent on {}",
            kind.name(),
            leaders.join(", ")
        );
    }
    Ok(())
}

/// Whether a reviewed task's coordinator may run on `kind`: a program with
/// the Claude leader adapter, or the Codex one when the user put the team on
/// Codex (`devagentics task codex-team`).
pub fn reviewed_leader_allowed(kind: crate::providers::Kind, codex_team: bool) -> bool {
    match kind.provider().reviewed_leader() {
        Some(crate::providers::ReviewedAdapter::Claude) => true,
        Some(crate::providers::ReviewedAdapter::Codex) => codex_team,
        _ => false,
    }
}

/// The leader's role led by the agent the user picked. Its own agent leaves
/// it as it is; its fallback's swaps the two, so the role's own program is
/// the one to move to; any other agent keeps the role's settings when it runs
/// the same program, and takes its program's defaults when not (a model of
/// one program means nothing to the other), with the fallback kept.
pub fn led_by(role: &Role, agent: &str, devws: &dyn Devws) -> Result<Role> {
    let kind = devws.agent_kind(agent)?;
    refuse_non_leader(agent, kind)?;
    if agent == role.agent {
        return Ok(role.clone());
    }
    if let Some(fallback) = role
        .on_fallback()
        .filter(|fallback| fallback.agent == agent)
    {
        return Ok(Role {
            fallback: Some(Fallback {
                agent: role.agent.clone(),
                model: role.model.clone(),
                effort: role.effort.clone(),
                approvals: role.approvals.clone(),
                sandbox: role.sandbox.clone(),
            }),
            ..fallback
        });
    }
    if devws.agent_kind(&role.agent).ok() == Some(kind) {
        return Ok(Role {
            agent: agent.to_string(),
            ..role.clone()
        });
    }
    Ok(Role {
        agent: agent.to_string(),
        model: None,
        effort: None,
        approvals: None,
        sandbox: None,
        ..role.clone()
    })
}

/// The leader's role as its file has it.
fn leader_role_file(home: &Home, task: &Task) -> Result<Role> {
    let config = task.config()?;
    let roles = Roles::load(
        home,
        config.repos.first().map(|repo| repo.worktree.as_path()),
    )?;
    roles
        .get("leader")
        .cloned()
        .context("there is no leader role")
}

/// The leader's role for the task: the role file's, led by the agent the user
/// picked when there is one.
fn leader_role(home: &Home, task: &Task, devws: &dyn Devws) -> Result<Role> {
    let role = leader_role_file(home, task)?;
    match task.state()?.leader_pick {
        Some(agent) => led_by(&role, &agent, devws),
        None => Ok(role),
    }
}

/// What the new leader does first after a handover.
fn take_over(before: &Role, why: &str) -> String {
    format!(
        "You take over as the leader from {}: {why}. Its conversation is gone. Call task_status and read_log to catch up, read_file the project's CLAUDE.md and memory_search the team's knowledge, then tell the user in one line that you took over, and go on.",
        program(before)
    )
}

/// The program the leader starts on: its role's, or its fallback's when the
/// role's account has run out and the fallback's may take over
/// (`reason_to_move`). With a reason for the log when the account has run out.
pub fn choose(role: &Role, devws: &dyn Devws) -> Result<(Role, Option<String>)> {
    let Some(fallback) = role.on_fallback() else {
        return Ok((role.clone(), None));
    };
    let Some(out) = reason_to_move(devws.limits(&role.agent)?, devws.limits(&fallback.agent)?)
    else {
        // Run out, but the fallback cannot take over: the log says why.
        if let (Some(out), Some(other)) = (
            run_out(devws, &role.agent)?,
            run_out(devws, &fallback.agent)?,
        ) {
            return Ok((
                role.clone(),
                Some(format!(
                    "{} has used {out} and {} {other}: the leader stays on {}",
                    role.agent, fallback.agent, role.agent
                )),
            ));
        }
        return Ok((role.clone(), None));
    };
    let why = format!(
        "the leader runs on {}: {} has used {out}",
        program(&fallback),
        role.agent
    );
    Ok((fallback, Some(why)))
}

/// Asks for the leader to move to its other program once its turn is over.
/// Refused when there is no other program or its account has run out. Says
/// where it will move.
pub fn request_switch(
    home: &Home,
    task: &Task,
    devws: &dyn Devws,
    reason: Option<&str>,
) -> Result<String> {
    let role = leader_role(home, task, devws)?;
    let Some((now, other)) = now_and_other(&role, &task.state()?) else {
        bail!("the leader's role has no [fallback]: there is no other program to move to");
    };
    anyhow::ensure!(
        task.state()?.workflow != crate::execution::Workflow::Reviewed
            || reviewed_leader_allowed(devws.agent_kind(&other.agent)?, task.state()?.codex_team),
        "reviewed coordinator switching requires a supported MCP-only adapter; Codex is not enabled for this workflow"
    );
    if !crate::accounts::is_enabled(&task.state()?, &other.agent) {
        bail!(
            "{} is turned off for this task: the leader stays on {}",
            other.agent,
            program(&now)
        );
    }
    if let Some(out) = run_out(devws, &other.agent)? {
        bail!(
            "{} has used {out}: the leader stays on {}",
            other.agent,
            program(&now)
        );
    }
    let reason = reason
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or(ASKED)
        .to_string();
    task.transaction(|tx| {
        tx.log(
            Kind::Leader,
            None,
            None,
            &format!("the leader will move to {}: {reason}", program(&other)),
        );
        tx.state.leader_switch = Some(reason.clone());
        Ok(())
    })?;
    Ok(format!(
        "The leader moves to {} once its turn is over.",
        program(&other)
    ))
}

/// The user picks the account that leads, any that can: the leader moves to
/// it once its turn is over (`hand_over`), and later starts on it. Refused
/// for an account that does not lead, is off for this task, has run out, or
/// that a reviewed task does not allow.
pub fn request_lead(
    home: &Home,
    task: &Task,
    devws: &dyn Devws,
    agent: &str,
    reason: Option<&str>,
) -> Result<String> {
    let target = led_by(&leader_role_file(home, task)?, agent, devws)?;
    let state = task.state()?;
    if state.leader_agent.as_deref() == Some(agent) && state.leader_switch.is_none() {
        task.transaction(|tx| {
            tx.state.leader_pick = Some(agent.to_string());
            Ok(())
        })?;
        return Ok(format!("{agent} already leads this team."));
    }
    anyhow::ensure!(
        state.workflow != crate::execution::Workflow::Reviewed
            || reviewed_leader_allowed(devws.agent_kind(agent)?, state.codex_team),
        "a reviewed task's leader runs on an MCP-only adapter: {agent} is not allowed here (Codex needs `devagentics task codex-team <task> on`)"
    );
    if !crate::accounts::is_enabled(&state, agent) {
        bail!("{agent} is turned off for this task: turn it on on the board first");
    }
    if let Some(out) = run_out(devws, agent)? {
        bail!("{agent} has used {out}: pick another account");
    }
    let reason = reason
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or(ASKED)
        .to_string();
    task.transaction(|tx| {
        tx.log(
            Kind::Leader,
            None,
            None,
            &format!("the leader will move to {}: {reason}", program(&target)),
        );
        tx.state.leader_pick = Some(agent.to_string());
        tx.state.leader_switch = Some(reason.clone());
        Ok(())
    })?;
    Ok(format!(
        "The leader moves to {} once its turn is over.",
        program(&target)
    ))
}

/// Every account that can lead, as "name<TAB>program<TAB>how it stands":
/// leading now, turned off, run out, or nothing when it may take over.
pub fn choices(task: &Task, devws: &dyn Devws) -> Result<Vec<String>> {
    let state = task.state()?;
    let mut lines = Vec::new();
    for (agent, kind) in devws.accounts()? {
        if !kind.provider().can_lead() {
            continue;
        }
        let stands = if state.leader_agent.as_deref() == Some(agent.as_str()) {
            "leads now".to_string()
        } else if !crate::accounts::is_enabled(&state, &agent) {
            "turned off for this task".to_string()
        } else if let Some(out) = run_out(devws, &agent)? {
            format!("has used {out}")
        } else {
            String::new()
        };
        lines.push(format!("{agent}\t{}\t{stands}", kind.name()));
    }
    Ok(lines)
}

/// What a message from the board is about: the row whose details it was
/// written in.
#[derive(Clone, Debug, PartialEq)]
pub enum About {
    /// The leader's own details.
    General,
    Step(String),
    Permission(String),
    Question(String),
    Agent(String),
}

/// What the user writes to the leader from the board's details dialog, with
/// where its subject stands, so the leader needs nothing else. It is the
/// user's answer too: a leader waiting for them is woken.
pub fn tell(task: &Task, about: &About, text: &str) -> Result<()> {
    let text = text.trim();
    anyhow::ensure!(!text.is_empty(), "write something for the leader first");
    task.transaction(|tx| {
        let state = &tx.state;
        let (message, subject) = match about {
            About::General => (format!("The user says: {text}"), None),
            About::Step(id) => {
                let found = state
                    .steps
                    .iter()
                    .find(|candidate| candidate.id == *id)
                    .with_context(|| format!("there is no step {id}"))?;
                let status = serde_json::to_value(found.status)?
                    .as_str()
                    .unwrap_or_default()
                    .replace('_', " ");
                (
                    format!(
                        "The user, about step {id} ({status} · {} attempts): {text}",
                        found.attempts
                    ),
                    Some(format!("step {id}")),
                )
            }
            About::Permission(id) => {
                let found = state
                    .permissions
                    .iter()
                    .find(|candidate| candidate.id == *id)
                    .with_context(|| format!("there is no permission request {id}"))?;
                let action = found.arguments["action"]
                    .as_str()
                    .map(|action| format!(" {action}"))
                    .unwrap_or_default();
                let status = serde_json::to_value(found.status)?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                (
                    format!(
                        "The user, about permission {id} ({}{action}, {status}): {text}",
                        found.tool
                    ),
                    Some(format!("permission {id}")),
                )
            }
            About::Question(id) => {
                let found = state
                    .questions
                    .iter()
                    .find(|candidate| candidate.id == *id)
                    .with_context(|| format!("there is no question {id}"))?;
                let asked: String = found.text.chars().take(80).collect();
                (
                    format!(
                        "The user, about question {id} ({} → {}: {asked}): {text}",
                        found.from, found.to
                    ),
                    Some(format!("question {id}")),
                )
            }
            About::Agent(id) => (
                format!("The user, about agent {id}: {text}"),
                Some(format!("agent {id}")),
            ),
        };
        let step = match about {
            About::Step(id) => Some(id.clone()),
            _ => None,
        };
        tx.state.say(crate::state::LEADER, message);
        tx.state.leader_wait = crate::state::LeaderWait::No;
        let about = subject
            .map(|subject| format!(" about {subject}"))
            .unwrap_or_default();
        tx.log(
            Kind::Review,
            None,
            step.as_deref(),
            &format!("the user told the leader{about}"),
        );
        Ok(())
    })
}

/// Whether a move was asked for and not made yet: the board then looks every
/// time, not only now and then.
pub fn switch_pending(task: &Task) -> Result<bool> {
    Ok(task.state()?.leader_switch.is_some())
}

/// The leader's part of task_status: the program it runs on, the other one
/// and whether it has room, a move asked for. Null without a fallback.
pub fn status(home: &Home, task: &Task, devws: &dyn Devws) -> Result<Value> {
    let state = task.state()?;
    let Some((now, other)) = now_and_other(&leader_role(home, task, devws)?, &state) else {
        return Ok(Value::Null);
    };
    let limits = devws.limits(&other.agent)?;
    Ok(json!({
        "program": program(&now),
        "other": program(&other),
        "other_limits": { "five_hour_percent": limits.five_hour, "week_percent": limits.week },
        "other_run_out": run_out(devws, &other.agent)?.is_some(),
        "switch_pending": state.leader_switch,
    }))
}

/// Hands the running leader over to its other program when that was asked
/// for, or when its account has run out and the other's has not; never while
/// it is in the middle of a turn. Says what it did; nothing when it did
/// nothing. `command` is this program, for the leader's tools.
pub fn hand_over(
    home: &Home,
    task: &Task,
    devws: &dyn Devws,
    command: &str,
) -> Result<Option<String>> {
    let _control = task.control_lock()?;
    if task.state()?.paused {
        return Ok(None);
    }
    let role = leader_role(home, task, devws)?;
    let state = task.state()?;
    // A pick moves the leader to the picked account, wherever it runs now;
    // otherwise it moves between its role's program and the fallback.
    let moving_to_pick = state.leader_switch.is_some()
        && state.leader_pick.as_deref() == Some(role.agent.as_str())
        && state.leader_agent.as_deref() != Some(role.agent.as_str());
    let (now, other) = if moving_to_pick {
        let running = state.leader_agent.clone().unwrap_or_default();
        (
            Role {
                agent: running,
                model: None,
                ..role.clone()
            },
            Role {
                fallback: None,
                ..role.clone()
            },
        )
    } else {
        let Some(pair) = now_and_other(&role, &state) else {
            return Ok(None);
        };
        pair
    };
    if !crate::accounts::is_enabled(&state, &other.agent) {
        return Ok(None);
    }
    if state.workflow == crate::execution::Workflow::Reviewed
        && !reviewed_leader_allowed(devws.agent_kind(&other.agent)?, state.codex_team)
    {
        return Ok(None);
    }
    let why = match &state.leader_switch {
        Some(asked) => asked.clone(),
        None => {
            let Some(out) = reason_to_move(devws.limits(&now.agent)?, devws.limits(&other.agent)?)
            else {
                return Ok(None);
            };
            format!("{} has used {out}", now.agent)
        }
    };
    let workspace = task.config()?.workspace;
    let Some(pane) = devws.pane_with_role(&workspace, "agent")? else {
        return Ok(None);
    };
    if matches!(
        devws.pane_state(&pane)?,
        PaneState::Working | PaneState::Gone
    ) {
        return Ok(None);
    }
    if waits_for_handoff(task, devws, &state, &other)? {
        return Ok(None);
    }
    let handoff = match &state.leader_handoff {
        Some(request) if request.saved => "with its handoff",
        Some(_) => "without a handoff: none was saved in time",
        None => "without a handoff: there is no memory",
    };
    let memory = handed_memory(task, devws, &why)?;
    let first = format!("{}\n\n{memory}", take_over(&now, &why));
    let args_file = write_leader_args(task, devws, &other, command, first.trim_end())?;
    devws.restart_agent(&pane, &other.agent, &args_file)?;
    let what = format!("the leader moved to {}: {why}", program(&other));
    task.transaction(|tx| {
        tx.state.leader_agent = Some(other.agent.clone());
        tx.state.leader_switch = None;
        tx.state.leader_handoff = None;
        tx.log(Kind::Leader, None, None, &format!("{what} ({handoff})"));
        Ok(())
    })?;
    Ok(Some(what))
}

/// How long a move waits for the outgoing leader's handoff note.
const HANDOFF_WAIT: u64 = 10 * 60;

/// Whether the move waits for the outgoing leader to save its handoff: it is
/// asked once, and the move goes on when the note is saved or the wait is
/// over. With no memory there is nothing to wait for.
fn waits_for_handoff(task: &Task, devws: &dyn Devws, state: &State, other: &Role) -> Result<bool> {
    if !devws.memory().is_on() {
        return Ok(false);
    }
    let now = crate::log::now();
    match &state.leader_handoff {
        Some(request) => Ok(!request.saved && now < request.asked_at + HANDOFF_WAIT),
        None => {
            task.transaction(|tx| {
                tx.state.leader_handoff = Some(crate::state::HandoffRequest {
                    asked_at: now,
                    saved: false,
                });
                tx.state.say(
                    crate::state::LEADER,
                    format!(
                        "You hand over to {} after this turn. First save your handoff with memory_save (kind handoff): what you were doing, what you decided and why, what comes next and anything the next leader must not miss. Keep it short: the task's state goes with it anyway. Then stop; do not start new work.",
                        program(other)
                    ),
                );
                tx.log(Kind::Leader, None, None, "the leader was asked for its handoff before it moves");
                Ok(())
            })?;
            Ok(true)
        }
    }
}

/// What the new leader is handed from the team's memory: the task's digest
/// (also saved there) and the newest leader handoffs.
fn handed_memory(task: &Task, devws: &dyn Devws, why: &str) -> Result<String> {
    let digest = crate::notes::digest(task, &format!("The leader moved: {why}."))?;
    crate::notes::record(
        task,
        devws,
        &crate::memory::Note {
            area: None,
            kind: crate::memory::NoteKind::Digest,
            title: "Where the task stood when the leader moved".into(),
            body: digest.clone(),
            author: "devagentics".into(),
            step: None,
        },
    );
    let mut text = format!("Where the task stands (devAgentics, from its state):\n{digest}");
    // A memory that fails costs the handoffs, never the move.
    let handoffs =
        crate::notes::newest(task, devws, crate::memory::NoteKind::Handoff, 2).unwrap_or_default();
    let block = crate::notes::fenced("The last leaders' handoffs", &handoffs);
    if !block.is_empty() {
        text.push_str(&format!("\n\n{block}"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devws::Limits;
    use crate::devws::fake::FakeDevws;
    use crate::providers::Kind as Program;
    use crate::start::start_task;
    use tempfile::TempDir;

    fn week(used: u32) -> Limits {
        Limits {
            five_hour: Some(10),
            week: Some(used),
        }
    }

    fn five_hour(used: u32) -> Limits {
        Limits {
            five_hour: Some(used),
            week: Some(20),
        }
    }

    /// A task whose accounts have used that much.
    fn team_with(dir: &TempDir, claude: Limits, codex: Limits) -> (Home, FakeDevws, Task, TempDir) {
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_kind("codex", Program::Codex)
            .with_limits("claude", claude)
            .with_limits("codex", codex);
        start_task(&home, &devws, repo.path(), "/bin/devagentics", None).unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        (home, devws, task, repo)
    }

    /// A task whose accounts have used that much of their week.
    fn team(dir: &TempDir, claude: u32, codex: u32) -> (Home, FakeDevws, Task, TempDir) {
        team_with(dir, week(claude), week(codex))
    }

    /// The leader's pane, between turns.
    fn idle_leader(devws: &FakeDevws, task: &Task) {
        let workspace = task.config().unwrap().workspace;
        devws.set_role(&workspace, "agent", "%1");
        devws.set_state("%1", PaneState::Done);
    }

    fn leader_agent(task: &Task) -> Option<String> {
        task.state().unwrap().leader_agent
    }

    fn args(task: &Task) -> String {
        std::fs::read_to_string(task.dir().join("leader.args")).unwrap()
    }

    #[test]
    fn an_account_runs_out_at_95_percent_of_its_5_hour_window_or_of_its_week() {
        let devws = FakeDevws::new(std::path::Path::new("/w"))
            .with_limits("fresh", five_hour(94))
            .with_limits("hour", five_hour(95))
            .with_limits("week", week(97))
            .with_limits(
                "both",
                Limits {
                    five_hour: Some(96),
                    week: Some(99),
                },
            );
        assert_eq!(run_out(&devws, "fresh").unwrap(), None);
        assert_eq!(run_out(&devws, "unknown").unwrap(), None, "no limits known");
        assert_eq!(
            run_out(&devws, "hour").unwrap().unwrap().to_string(),
            "95% of its 5-hour window"
        );
        assert_eq!(
            run_out(&devws, "week").unwrap().unwrap().to_string(),
            "97% of its week"
        );
        assert_eq!(
            run_out(&devws, "both").unwrap().unwrap().to_string(),
            "99% of its week",
            "the fullest"
        );
    }

    #[test]
    fn the_leader_starts_on_codex_sol_once_claude_has_used_95_percent_of_its_week() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team(&dir, 95, 14);
        assert_eq!(leader_agent(&task).as_deref(), Some("codex"));
        assert!(args(&task).contains("gpt-6.1-sol"), "{}", args(&task));
        assert!(
            args(&task).contains("mcp_servers.devagentics"),
            "it keeps its tools"
        );
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("runs on codex (gpt-6.1-sol)")),
            "the log says why"
        );

        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team(&dir, 94, 14);
        assert_eq!(leader_agent(&task).as_deref(), Some("claude"));

        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team(&dir, 97, 96);
        assert_eq!(
            leader_agent(&task).as_deref(),
            Some("claude"),
            "both out: it stays"
        );
    }

    #[test]
    fn the_leader_starts_on_codex_when_claudes_5_hour_window_ran_out() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team_with(&dir, five_hour(95), five_hour(30));
        assert_eq!(leader_agent(&task).as_deref(), Some("codex"));
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("95% of its 5-hour window")),
            "{:?}",
            task.log_lines()
        );
    }

    #[test]
    fn a_running_leader_moves_once_its_turn_is_over_and_only_once() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 80, 14);
        idle_leader(&devws, &task);
        devws.set_state("%1", PaneState::Working);
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "not run out"
        );

        devws.set_limits("claude", week(96));
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "not while it works"
        );
        devws.set_limits("codex", week(99));
        devws.set_state("%1", PaneState::Done);
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "codex is out too"
        );

        devws.set_limits("codex", week(14));
        let what = hand_over(&home, &task, &devws, "d")
            .unwrap()
            .expect("handed over");
        assert!(
            what.contains("codex (gpt-6.1-sol)") && what.contains("96%"),
            "{what}"
        );
        assert!(
            devws
                .calls()
                .iter()
                .any(|call| call.starts_with("run %1 codex ")),
            "{:?}",
            devws.calls()
        );
        assert!(args(&task).contains("You take over as the leader"));
        assert_eq!(leader_agent(&task).as_deref(), Some("codex"));

        devws.set_state("%1", PaneState::Done);
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "only once: codex has room, so it does not move back"
        );

        let roles = Roles::load(&home, None).unwrap();
        let board = crate::board::Board::build(&task, &devws, &roles, Default::default()).unwrap();
        let leader = board.roles.iter().find(|row| row.name == "leader").unwrap();
        assert_eq!(leader.program, "codex · gpt-6.1-sol · medium · fallback");
    }

    fn limits(five_hour: u32, week: u32) -> Limits {
        Limits {
            five_hour: Some(five_hour),
            week: Some(week),
        }
    }

    #[test]
    fn the_week_comes_before_the_5_hour_window() {
        let moves =
            |now: Limits, other: Limits| reason_to_move(now, other).map(|out| out.to_string());
        // A week used up moves the leader even to a full 5-hour window: that
        // one comes back in hours, the week in days.
        assert_eq!(
            moves(limits(10, 96), limits(97, 20)).as_deref(),
            Some("96% of its week")
        );
        assert_eq!(
            moves(limits(97, 96), limits(10, 20)).as_deref(),
            Some("96% of its week")
        );
        assert_eq!(
            moves(limits(10, 96), limits(10, 95)),
            None,
            "both weeks used up"
        );
        // A 5-hour window used up moves it only to an account with room in both.
        assert_eq!(
            moves(limits(95, 20), limits(10, 20)).as_deref(),
            Some("95% of its 5-hour window")
        );
        assert_eq!(
            moves(limits(95, 20), limits(10, 95)),
            None,
            "the other's week is used up"
        );
        assert_eq!(
            moves(limits(95, 20), limits(96, 20)),
            None,
            "the other's 5h too"
        );
        assert_eq!(
            moves(limits(94, 94), limits(10, 10)),
            None,
            "nothing used up"
        );
        assert_eq!(
            moves(Limits::default(), limits(10, 10)),
            None,
            "nothing known"
        );
    }

    #[test]
    fn a_leader_whose_week_is_used_up_moves_to_a_full_5_hour_window_and_does_not_come_back() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team_with(&dir, limits(10, 96), limits(97, 20));
        assert_eq!(
            leader_agent(&task).as_deref(),
            Some("codex"),
            "it starts on codex"
        );

        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 40, 20);
        idle_leader(&devws, &task);
        devws.set_limits("claude", limits(10, 96));
        devws.set_limits("codex", limits(97, 20));
        let what = hand_over(&home, &task, &devws, "d")
            .unwrap()
            .expect("handed over");
        assert_eq!(
            what,
            "the leader moved to codex (gpt-6.1-sol): claude has used 96% of its week"
        );
        devws.set_state("%1", PaneState::Done);
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "codex's 5-hour window is full, but claude's week is used up: it stays"
        );
    }

    #[test]
    fn a_leader_on_codex_moves_back_to_claude_when_codexs_5_hour_window_runs_out() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 96, 14);
        assert_eq!(leader_agent(&task).as_deref(), Some("codex"));
        idle_leader(&devws, &task);

        devws.set_limits("codex", five_hour(95));
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "claude is out too"
        );

        devws.set_limits("claude", five_hour(10));
        let what = hand_over(&home, &task, &devws, "d")
            .unwrap()
            .expect("moved back");
        assert_eq!(
            what,
            "the leader moved to claude: codex has used 95% of its 5-hour window"
        );
        assert!(
            devws
                .calls()
                .iter()
                .any(|call| call.starts_with("run %1 claude ")),
            "{:?}",
            devws.calls()
        );
        assert!(
            args(&task)
                .contains("from codex (gpt-6.1-sol): codex has used 95% of its 5-hour window"),
            "{}",
            args(&task)
        );
        assert_eq!(leader_agent(&task).as_deref(), Some("claude"));
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "claude has room: it stays"
        );
    }

    #[test]
    fn an_asked_for_move_happens_once_the_turn_is_over_with_room_left() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 96, 14);
        idle_leader(&devws, &task);
        devws.set_state("%1", PaneState::Working);
        devws.set_limits("claude", week(40));

        assert!(!switch_pending(&task).unwrap());
        let said = request_switch(&home, &task, &devws, Some("the user wants Claude")).unwrap();
        assert_eq!(said, "The leader moves to claude once its turn is over.");
        assert!(switch_pending(&task).unwrap());
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "not while it works"
        );

        devws.set_state("%1", PaneState::Done);
        let what = hand_over(&home, &task, &devws, "d")
            .unwrap()
            .expect("moved");
        assert_eq!(what, "the leader moved to claude: the user wants Claude");
        assert_eq!(leader_agent(&task).as_deref(), Some("claude"));
        assert!(!switch_pending(&task).unwrap(), "the request is done");
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("the leader moved to claude: the user wants Claude"))
        );

        // And back again, with no reason given.
        request_switch(&home, &task, &devws, None).unwrap();
        let what = hand_over(&home, &task, &devws, "d")
            .unwrap()
            .expect("moved");
        assert_eq!(
            what,
            "the leader moved to codex (gpt-6.1-sol): the user asked"
        );
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "only once"
        );
    }

    #[test]
    fn the_user_picks_any_account_to_lead_and_it_takes_over_after_the_turn() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 40, 14);
        let devws = devws.with_kind("claude_work", Program::Claude);
        idle_leader(&devws, &task);
        devws.set_state("%1", PaneState::Working);
        assert_eq!(leader_agent(&task).as_deref(), Some("claude"));

        let said = request_lead(&home, &task, &devws, "claude_work", None).unwrap();
        assert_eq!(
            said,
            "The leader moves to claude_work once its turn is over."
        );
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "not while it works"
        );

        devws.set_state("%1", PaneState::Done);
        let what = hand_over(&home, &task, &devws, "d")
            .unwrap()
            .expect("moved");
        assert_eq!(what, "the leader moved to claude_work: the user asked");
        assert_eq!(leader_agent(&task).as_deref(), Some("claude_work"));
        assert_eq!(
            task.state().unwrap().leader_pick.as_deref(),
            Some("claude_work")
        );
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "only once"
        );
        assert!(
            request_lead(&home, &task, &devws, "claude_work", None)
                .unwrap()
                .contains("already leads")
        );
    }

    #[test]
    fn the_choices_are_the_accounts_that_can_lead_and_how_each_stands() {
        let dir = TempDir::new().unwrap();
        let (_home, devws, task, _repo) = team(&dir, 40, 99);
        let devws = devws
            .with_kind("opencode", Program::Opencode)
            .with_kind("claude_work", Program::Claude);
        crate::accounts::set_enabled(&task, &devws, "claude_work", false).unwrap();
        assert_eq!(
            choices(&task, &devws).unwrap(),
            [
                "claude\tclaude\tleads now",
                "claude_work\tclaude\tturned off for this task",
                "codex\tcodex\thas used 99% of its week",
            ]
        );
    }

    #[test]
    fn the_outgoing_leader_saves_its_handoff_and_the_new_one_opens_with_it() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 40, 14);
        let devws = devws.with_memory();
        idle_leader(&devws, &task);
        request_switch(&home, &task, &devws, Some("the user wants Codex")).unwrap();

        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "it asks first"
        );
        let state = task.state().unwrap();
        assert!(state.leader_handoff.is_some());
        assert!(
            state
                .queue
                .iter()
                .any(|message| message.to == crate::state::LEADER
                    && message.text.contains("save your handoff with memory_save")),
            "the leader is asked"
        );
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "and waits for it"
        );

        crate::notes::save(
            &task,
            &devws,
            &crate::memory::Note {
                area: None,
                kind: crate::memory::NoteKind::Handoff,
                title: "Mid B3".into(),
                body: "B3 waits for review r484; ask the reviewer first.".into(),
                author: crate::state::LEADER.into(),
                step: None,
            },
        )
        .unwrap();
        hand_over(&home, &task, &devws, "d")
            .unwrap()
            .expect("moved once saved");
        let first = args(&task);
        assert!(first.contains("Where the task stands"), "{first}");
        assert!(
            first.contains("B3 waits for review r484"),
            "the handoff is handed on: {first}"
        );
        assert!(first.contains("not instructions"), "fenced as history");
        assert!(
            devws
                .notes()
                .iter()
                .any(|(_, path, _)| path.starts_with("tasks/feat-x/digest/")),
            "the digest is kept too"
        );
        assert!(task.state().unwrap().leader_handoff.is_none());
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("(with its handoff)"))
        );
    }

    #[test]
    fn a_handoff_not_saved_in_time_does_not_hold_the_move() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 40, 14);
        let devws = devws.with_memory();
        idle_leader(&devws, &task);
        request_switch(&home, &task, &devws, None).unwrap();
        assert_eq!(hand_over(&home, &task, &devws, "d").unwrap(), None);
        task.transaction(|tx| {
            tx.state.leader_handoff.as_mut().unwrap().asked_at -= HANDOFF_WAIT;
            Ok(())
        })
        .unwrap();
        hand_over(&home, &task, &devws, "d")
            .unwrap()
            .expect("moved after the wait");
        assert!(
            args(&task).contains("Where the task stands"),
            "the digest still goes"
        );
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("none was saved in time"))
        );
    }

    #[test]
    fn an_account_that_cannot_lead_or_is_off_is_refused() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 40, 14);
        let devws = devws.with_kind("opencode", Program::Opencode);
        let error = request_lead(&home, &task, &devws, "opencode", None).unwrap_err();
        assert!(error.to_string().contains("does not lead"), "{error}");
        crate::accounts::set_enabled(&task, &devws, "codex", false).unwrap();
        let error = request_lead(&home, &task, &devws, "codex", None).unwrap_err();
        assert!(error.to_string().contains("turned off"), "{error}");
        assert!(!switch_pending(&task).unwrap());
    }

    #[test]
    fn what_the_user_writes_on_the_board_reaches_the_leader_with_its_step() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team(&dir, 40, 14);
        task.transaction(|tx| {
            tx.state.steps.push(crate::state::Step {
                id: "B1-1-AT".into(),
                title: "Registry tests".into(),
                status: crate::state::StepStatus::Blocked,
                attempts: 6,
                spec: None,
                review_since: None,
            });
            tx.state.leader_wait = crate::state::LeaderWait::Shown;
            Ok(())
        })
        .unwrap();

        tell(
            &task,
            &About::Step("B1-1-AT".into()),
            "  accept it if the checkpoint passed ",
        )
        .unwrap();
        tell(&task, &About::General, "pause the floor").unwrap();

        let state = task.state().unwrap();
        let said: Vec<_> = state
            .queue
            .iter()
            .filter(|message| message.to == crate::state::LEADER)
            .map(|message| message.text.as_str())
            .collect();
        assert!(said.contains(&"The user, about step B1-1-AT (blocked · 6 attempts): accept it if the checkpoint passed"), "{said:?}");
        assert!(said.contains(&"The user says: pause the floor"), "{said:?}");
        assert_eq!(
            state.leader_wait,
            crate::state::LeaderWait::No,
            "the user answered"
        );
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("the user told the leader about step B1-1-AT"))
        );
        assert!(
            tell(&task, &About::General, "   ").is_err(),
            "an empty message"
        );
        assert!(
            tell(&task, &About::Step("nope".into()), "x").is_err(),
            "an unknown step"
        );
    }

    #[test]
    fn a_message_from_needs_you_carries_what_it_is_about() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team(&dir, 40, 14);
        task.transaction(|tx| {
            tx.state.permissions.push(crate::state::PermissionRequest {
                id: "p32".into(),
                tool: "application_workflow".into(),
                arguments: serde_json::json!({"action": "runner_configure"}),
                effect: crate::state::Effect::External,
                status: crate::state::PermissionStatus::Pending,
            });
            tx.state.questions.push(crate::state::Question {
                id: "q7".into(),
                from: "hammer-5".into(),
                to: crate::state::LEADER.into(),
                order: "o1".into(),
                text: "Which manifest path?".into(),
                answer: None,
            });
            Ok(())
        })
        .unwrap();
        tell(
            &task,
            &About::Permission("p32".into()),
            "deny it: p33 applied it",
        )
        .unwrap();
        tell(&task, &About::Question("q7".into()), "the one in the plan").unwrap();
        tell(&task, &About::Agent("hammer-5".into()), "let it finish").unwrap();
        let said: Vec<String> = task
            .state()
            .unwrap()
            .queue
            .iter()
            .filter(|message| message.to == crate::state::LEADER)
            .map(|message| message.text.clone())
            .collect();
        assert!(said.iter().any(|text| text == "The user, about permission p32 (application_workflow runner_configure, pending): deny it: p33 applied it"), "{said:?}");
        assert!(said.iter().any(|text| text == "The user, about question q7 (hammer-5 → leader: Which manifest path?): the one in the plan"), "{said:?}");
        assert!(
            said.iter()
                .any(|text| text == "The user, about agent hammer-5: let it finish"),
            "{said:?}"
        );
        assert!(tell(&task, &About::Permission("p99".into()), "x").is_err());
    }

    #[test]
    fn the_leader_never_moves_to_an_account_the_user_turned_off() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 40, 14);
        task.transaction(|tx| {
            tx.state.disabled_accounts.insert("codex".into());
            Ok(())
        })
        .unwrap();
        let message = format!(
            "{:#}",
            request_switch(&home, &task, &devws, None).err().unwrap()
        );
        assert_eq!(
            message,
            "codex is turned off for this task: the leader stays on claude"
        );
        // Run out, the leader would move on its own; not to an account that is off.
        devws.set_limits("claude", five_hour(99));
        assert_eq!(hand_over(&home, &task, &devws, "d").unwrap(), None);
    }

    #[test]
    fn a_move_is_refused_when_the_other_has_run_out_or_there_is_none() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 40, 14);
        devws.set_limits("codex", five_hour(97));
        let message = format!(
            "{:#}",
            request_switch(&home, &task, &devws, None).err().unwrap()
        );
        assert_eq!(
            message,
            "codex has used 97% of its 5-hour window: the leader stays on claude"
        );
        assert!(!switch_pending(&task).unwrap());

        std::fs::create_dir_all(home.roles_dir()).unwrap();
        std::fs::write(
            home.roles_dir().join("leader.md"),
            "+++\nagent = \"claude\"\n+++\nLead without a fallback.\n",
        )
        .unwrap();
        let message = format!(
            "{:#}",
            request_switch(&home, &task, &devws, None).err().unwrap()
        );
        assert!(message.contains("no [fallback]"), "{message}");
    }

    /// The built-in leader role: claude, its fallback codex.
    fn builtin_leader(home: &Home) -> Role {
        Roles::load(home, None)
            .unwrap()
            .get("leader")
            .unwrap()
            .clone()
    }

    #[test]
    fn a_leader_led_by_its_roles_own_agent_is_the_role() {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let devws = FakeDevws::new(dir.path()).with_kind("claude", Program::Claude);
        let role = builtin_leader(&home);
        assert_eq!(led_by(&role, "claude", &devws).unwrap(), role);
    }

    #[test]
    fn a_leader_led_by_its_fallback_swaps_the_two_programs() {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let devws = FakeDevws::new(dir.path())
            .with_kind("claude", Program::Claude)
            .with_kind("codex", Program::Codex);
        let role = builtin_leader(&home);
        let fallback = role.on_fallback().unwrap();
        let led = led_by(&role, "codex", &devws).unwrap();
        assert_eq!(led.agent, "codex");
        assert_eq!(led.model, fallback.model);
        let back = led.fallback.unwrap();
        assert_eq!(back.agent, "claude");
        assert_eq!(back.effort, role.effort);
    }

    #[test]
    fn a_leader_led_by_another_account_keeps_the_settings_of_its_own_program_only() {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let devws = FakeDevws::new(dir.path())
            .with_kind("claude", Program::Claude)
            .with_kind("claude_work", Program::Claude)
            .with_kind("codex_work", Program::Codex);
        let role = builtin_leader(&home);

        let same = led_by(&role, "claude_work", &devws).unwrap();
        assert_eq!(same.agent, "claude_work");
        assert_eq!(same.effort.as_deref(), Some("high"));
        assert_eq!(
            same.fallback, role.fallback,
            "it can still move to the fallback"
        );

        let other = led_by(&role, "codex_work", &devws).unwrap();
        assert_eq!(other.agent, "codex_work");
        assert_eq!(
            (other.model, other.effort),
            (None, None),
            "the program's defaults"
        );
        assert_eq!(other.fallback, role.fallback);

        assert!(
            led_by(&role, "ghost", &devws).is_err(),
            "an agent devws does not know"
        );
    }

    #[test]
    fn an_opencode_agent_does_not_lead() {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let devws = FakeDevws::new(dir.path()).with_kind("opencode", Program::Opencode);
        let message = led_by(&builtin_leader(&home), "opencode", &devws)
            .unwrap_err()
            .to_string();
        assert!(message.contains("does not lead"), "{message}");
    }

    #[test]
    fn an_antigravity_agent_leads_with_the_task_s_mcp_server() {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_kind("gemini", Program::Antigravity);
        assert_eq!(
            led_by(&builtin_leader(&home), "gemini", &devws)
                .unwrap()
                .agent,
            "gemini"
        );
        start_task(
            &home,
            &devws,
            repo.path(),
            "/bin/devagentics",
            Some("gemini"),
        )
        .unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        let arguments: Vec<_> = args(&task).split('\0').map(str::to_string).collect();
        assert_eq!(arguments[0], "-i");
        let mcp = arguments
            .iter()
            .position(|argument| argument == "--mcp-config")
            .expect("the leader's MCP file");
        assert!(arguments[mcp + 1].ends_with("mcp.json"), "{arguments:?}");
    }

    /// A task started with the leader the user picked.
    fn team_led_by(dir: &TempDir, agent: &str, claude: u32) -> (Home, FakeDevws, Task, TempDir) {
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_kind("codex", Program::Codex)
            .with_limits("claude", week(claude))
            .with_limits("codex", week(10));
        start_task(&home, &devws, repo.path(), "/bin/devagentics", Some(agent)).unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        (home, devws, task, repo)
    }

    #[test]
    fn a_picked_leader_starts_even_when_its_account_has_run_out() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team_led_by(&dir, "claude", 99);
        assert_eq!(leader_agent(&task).as_deref(), Some("claude"));
        assert_eq!(task.state().unwrap().leader_pick.as_deref(), Some("claude"));
    }

    #[test]
    fn a_leader_picked_on_the_fallback_moves_to_the_roles_own_program() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team_led_by(&dir, "codex", 10);
        assert_eq!(leader_agent(&task).as_deref(), Some("codex"));
        assert!(args(&task).contains("gpt-6.1-sol"), "{}", args(&task));
        let status = status(&home, &task, &devws).unwrap();
        assert!(
            status["program"].as_str().unwrap().starts_with("codex"),
            "{status}"
        );
        assert!(
            status["other"].as_str().unwrap().starts_with("claude"),
            "{status}"
        );

        request_switch(&home, &task, &devws, None).unwrap();
        idle_leader(&devws, &task);
        hand_over(&home, &task, &devws, "/bin/devagentics")
            .unwrap()
            .unwrap();
        assert_eq!(leader_agent(&task).as_deref(), Some("claude"));
    }
}
