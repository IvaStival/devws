//! The leader's program. The leader keeps the team going, so it has two
//! programs: its role's and its role's fallback (`[fallback]` in leader.md).
//! When the account it runs on has used most of its 5-hour window or of its
//! week, it moves to the other program, if that one's has not: at start, or,
//! while it runs, by a handover once it has finished its turn. It also moves
//! when asked (the leader's `switch_leader`, `devagentics leader switch`).
//! Its conversation does not go with it: the new leader catches up from
//! task_status and the log.

use crate::devws::{Devws, PaneState};
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
        .filter_map(|(window, used)| {
            used.filter(|used| *used >= LIMIT_PERCENT)
                .map(|used| RunOut { window, used })
        })
        .max_by_key(|out| out.used))
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

/// The leader's role led by the agent the user picked. Its own agent leaves
/// it as it is; its fallback's swaps the two, so the role's own program is
/// the one to move to; any other agent keeps the role's settings when it runs
/// the same program, and takes its program's defaults when not (a model of
/// one program means nothing to the other), with the fallback kept.
pub fn led_by(role: &Role, agent: &str, devws: &dyn Devws) -> Result<Role> {
    let kind = devws.agent_kind(agent)?;
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

/// The leader's role for the task: the role file's, led by the agent the user
/// picked when there is one.
fn leader_role(home: &Home, task: &Task, devws: &dyn Devws) -> Result<Role> {
    let config = task.config()?;
    let roles = Roles::load(
        home,
        config.repos.first().map(|repo| repo.worktree.as_path()),
    )?;
    let role = roles.get("leader").context("there is no leader role")?;
    match task.state()?.leader_pick {
        Some(agent) => led_by(role, &agent, devws),
        None => Ok(role.clone()),
    }
}

/// What the new leader does first after a handover.
fn take_over(before: &Role, why: &str) -> String {
    format!(
        "You take over as the leader from {}: {why}. Its conversation is gone. Call task_status and read_log to catch up, then tell the user in one line that you took over, and go on.",
        program(before)
    )
}

/// The program the leader starts on: its role's, or its fallback's when the
/// role's account has run out and the fallback's has not. With a reason for
/// the log when the account has run out.
pub fn choose(role: &Role, devws: &dyn Devws) -> Result<(Role, Option<String>)> {
    let Some(fallback) = role.on_fallback() else {
        return Ok((role.clone(), None));
    };
    let Some(out) = run_out(devws, &role.agent)? else {
        return Ok((role.clone(), None));
    };
    if let Some(other) = run_out(devws, &fallback.agent)? {
        return Ok((
            role.clone(),
            Some(format!(
                "{} has used {out} and {} {other}: the leader stays on {}",
                role.agent, fallback.agent, role.agent
            )),
        ));
    }
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
    let role = leader_role(home, task, devws)?;
    let state = task.state()?;
    let Some((now, other)) = now_and_other(&role, &state) else {
        return Ok(None);
    };
    let why = match &state.leader_switch {
        Some(asked) => asked.clone(),
        None => {
            let Some(out) = run_out(devws, &now.agent)? else {
                return Ok(None);
            };
            // Both out: it stays. The other not out also means the new
            // leader never qualifies to move straight back.
            if run_out(devws, &other.agent)?.is_some() {
                return Ok(None);
            }
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
    let args_file = write_leader_args(task, devws, &other, command, &take_over(&now, &why))?;
    devws.restart_agent(&pane, &other.agent, &args_file)?;
    let what = format!("the leader moved to {}: {why}", program(&other));
    task.transaction(|tx| {
        tx.state.leader_agent = Some(other.agent.clone());
        tx.state.leader_switch = None;
        tx.log(Kind::Leader, None, None, &what);
        Ok(())
    })?;
    Ok(Some(what))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_args::Kind as Program;
    use crate::devws::Limits;
    use crate::devws::fake::FakeDevws;
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
