//! Which agent accounts a task may use: the user turns each one on or off on
//! the board; new agents start only on accounts that are on.
use crate::devws::Devws;
use crate::log::Kind;
use crate::state::{LEADER, State};
use crate::task::Task;
use anyhow::{Result, bail, ensure};

/// Whether new agents of this task may start on `account`.
pub fn is_enabled(state: &State, account: &str) -> bool {
    !state.disabled_accounts.contains(account)
}

/// The user turns `account` on or off for this task. The last account that
/// is on stays on: a team with none could not start anyone.
pub fn set_enabled(task: &Task, devws: &dyn Devws, account: &str, on: bool) -> Result<()> {
    let registered: Vec<String> = devws
        .accounts()?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    if !registered.iter().any(|name| name == account) {
        bail!("there is no account {account} in devws");
    }
    task.transaction(|tx| {
        if on == is_enabled(&tx.state, account) {
            return Ok(());
        }
        if on {
            tx.state.disabled_accounts.remove(account);
        } else {
            let others_on = registered
                .iter()
                .any(|name| name != account && is_enabled(&tx.state, name));
            ensure!(
                others_on,
                "{account} is the last account that is on: turn another one on first"
            );
            tx.state.disabled_accounts.insert(account.to_string());
        }
        let word = if on { "on" } else { "off" };
        tx.state.say(
            LEADER,
            format!(
                "The user turned {account} {word} for this task. Start new agents only on accounts that are on (task_status accounts)."
            ),
        );
        tx.log(Kind::Review, None, None, &format!("the user turned {account} {word}"));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_args::Kind as Program;
    use crate::devws::fake::FakeDevws;
    use crate::home::Home;
    use tempfile::TempDir;

    fn task() -> (TempDir, TempDir, Task, FakeDevws) {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_kind("codex", Program::Codex)
            .with_kind("claude_work", Program::Claude);
        crate::start::start_task(&home, &devws, repo.path(), "d", None).unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        (dir, repo, task, devws)
    }

    #[test]
    fn an_account_is_turned_off_and_on_and_the_leader_hears_it() {
        let (_dir, _repo, task, devws) = task();
        set_enabled(&task, &devws, "claude", false).unwrap();
        let state = task.state().unwrap();
        assert!(!is_enabled(&state, "claude"));
        assert!(is_enabled(&state, "codex"));
        assert!(
            state
                .queue
                .iter()
                .any(|message| message.to == LEADER && message.text.contains("turned claude off"))
        );
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("the user turned claude off"))
        );

        set_enabled(&task, &devws, "claude", true).unwrap();
        assert!(is_enabled(&task.state().unwrap(), "claude"));
    }

    #[test]
    fn the_last_account_that_is_on_stays_on() {
        let (_dir, _repo, task, devws) = task();
        set_enabled(&task, &devws, "claude", false).unwrap();
        set_enabled(&task, &devws, "claude_work", false).unwrap();
        let error = set_enabled(&task, &devws, "codex", false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("last account that is on"), "{error}");
        assert!(is_enabled(&task.state().unwrap(), "codex"));
    }

    #[test]
    fn only_a_registered_account_can_be_switched() {
        let (_dir, _repo, task, devws) = task();
        let error = set_enabled(&task, &devws, "nobody", false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no account nobody"), "{error}");
    }
}
