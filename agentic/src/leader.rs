//! The leader's program. The leader keeps the team going, so when its
//! account has used most of its week it moves to its role's fallback
//! (`[fallback]` in leader.md): at start, or, while it runs, by a handover
//! once it has finished its turn. Its conversation does not go with it: the
//! new leader catches up from task_status and the log. It does not move back
//! on its own; the next start chooses again.

use crate::devws::{Devws, PaneState};
use crate::home::Home;
use crate::log::Kind;
use crate::roles::{Role, Roles};
use crate::start::write_leader_args;
use crate::task::Task;
use anyhow::{Context, Result};

/// An account that has used this much of its week counts as run out.
pub const WEEK_LIMIT_PERCENT: u32 = 95;

/// What the new leader does first after a handover.
const TAKE_OVER: &str = "You take over as the leader: the one before you ran out of its account's week, and its conversation is gone. Call task_status and read_log to catch up, then tell the user in one line that you took over, and go on.";

/// How much of its week the account has used, when that is past the limit.
fn run_out(devws: &dyn Devws, agent: &str) -> Result<Option<u32>> {
    Ok(devws
        .limits(agent)?
        .week
        .filter(|used| *used >= WEEK_LIMIT_PERCENT))
}

fn program(role: &Role) -> String {
    match &role.model {
        Some(model) => format!("{} ({model})", role.agent),
        None => role.agent.clone(),
    }
}

/// The program the leader starts on: its role's, or its fallback's when the
/// role's account has run out and the fallback's has not. With a reason for
/// the log when the account has run out.
pub fn choose(role: &Role, devws: &dyn Devws) -> Result<(Role, Option<String>)> {
    let Some(fallback) = role.on_fallback() else {
        return Ok((role.clone(), None));
    };
    let Some(used) = run_out(devws, &role.agent)? else {
        return Ok((role.clone(), None));
    };
    if let Some(other) = run_out(devws, &fallback.agent)? {
        return Ok((
            role.clone(),
            Some(format!(
                "{} has used {used}% of its week and {} {other}% of its: the leader stays on {}",
                role.agent, fallback.agent, role.agent
            )),
        ));
    }
    let why = format!(
        "the leader runs on {}: {} has used {used}% of its week",
        program(&fallback),
        role.agent
    );
    Ok((fallback, Some(why)))
}

/// Hands the running leader over to its fallback when its account has run
/// out, the fallback's has not, and it is not in the middle of a turn. Says
/// what it did; nothing when it did nothing. `command` is this program, for
/// the leader's tools.
pub fn hand_over(
    home: &Home,
    task: &Task,
    devws: &dyn Devws,
    command: &str,
) -> Result<Option<String>> {
    let config = task.config()?;
    let roles = Roles::load(
        home,
        config.repos.first().map(|repo| repo.worktree.as_path()),
    )?;
    let role = roles.get("leader").context("there is no leader role")?;
    let Some(fallback) = role.on_fallback() else {
        return Ok(None);
    };
    // Moved already: never back on its own.
    if task.state()?.leader_agent.as_deref().unwrap_or(&role.agent) != role.agent {
        return Ok(None);
    }
    let Some(used) = run_out(devws, &role.agent)? else {
        return Ok(None);
    };
    if run_out(devws, &fallback.agent)?.is_some() {
        return Ok(None);
    }
    let Some(pane) = devws.pane_with_role(&config.workspace, "agent")? else {
        return Ok(None);
    };
    if matches!(
        devws.pane_state(&pane)?,
        PaneState::Working | PaneState::Gone
    ) {
        return Ok(None);
    }
    let args_file = write_leader_args(task, devws, &fallback, command, TAKE_OVER)?;
    devws.restart_agent(&pane, &fallback.agent, &args_file)?;
    let what = format!(
        "the leader moved to {}: {} has used {used}% of its week",
        program(&fallback),
        role.agent
    );
    task.transaction(|tx| {
        tx.state.leader_agent = Some(fallback.agent.clone());
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

    /// A task whose accounts have used that much of their week.
    fn team(dir: &TempDir, claude: u32, codex: u32) -> (Home, FakeDevws, Task, TempDir) {
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_kind("codex", Program::Codex)
            .with_limits("claude", week(claude))
            .with_limits("codex", week(codex));
        start_task(&home, &devws, repo.path(), "/bin/devagentics").unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        (home, devws, task, repo)
    }

    fn args(task: &Task) -> String {
        std::fs::read_to_string(task.dir().join("leader.args")).unwrap()
    }

    #[test]
    fn the_leader_starts_on_codex_sol_once_claude_has_used_95_percent_of_its_week() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team(&dir, 95, 14);
        assert_eq!(task.state().unwrap().leader_agent.as_deref(), Some("codex"));
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
        assert_eq!(
            task.state().unwrap().leader_agent.as_deref(),
            Some("claude")
        );

        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = team(&dir, 97, 96);
        assert_eq!(
            task.state().unwrap().leader_agent.as_deref(),
            Some("claude"),
            "both out: it stays"
        );
    }

    #[test]
    fn a_running_leader_moves_once_its_turn_is_over_and_only_once() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = team(&dir, 80, 14);
        let workspace = task.config().unwrap().workspace;
        devws.set_role(&workspace, "agent", "%1");
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
        assert_eq!(task.state().unwrap().leader_agent.as_deref(), Some("codex"));

        devws.set_state("%1", PaneState::Done);
        assert_eq!(
            hand_over(&home, &task, &devws, "d").unwrap(),
            None,
            "only once"
        );

        let roles = Roles::load(&home, None).unwrap();
        let board = crate::board::Board::build(&task, &devws, &roles, Vec::new()).unwrap();
        let leader = board.roles.iter().find(|row| row.name == "leader").unwrap();
        assert_eq!(leader.program, "codex · gpt-6.1-sol · medium · fallback");
    }
}
