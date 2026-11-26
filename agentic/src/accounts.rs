//! Which agent accounts a task may use: the user turns each one on or off on
//! the board, for the whole task (global) and for each worker group (seniors,
//! floor); new agents start only on accounts that are on for their group.
use crate::devws::Devws;
use crate::log::Kind;
use crate::roles::Group;
use crate::state::{LEADER, State};
use crate::task::Task;
use anyhow::{Result, bail, ensure};
use std::collections::BTreeSet;

/// The groups an account can be switched for, apart from the global switch.
/// The leader (CTO) follows the global switch only.
pub const WORKER_GROUPS: [Group; 2] = [Group::Senior, Group::Floor];

/// Whether new agents of this task may start on `account` (the global switch).
pub fn is_enabled(state: &State, account: &str) -> bool {
    !state.disabled_accounts.contains(account)
}

/// The group's own switch for `account`, whatever the global one says.
pub fn is_group_switch_on(state: &State, account: &str, group: Group) -> bool {
    !state
        .disabled_group_accounts
        .get(&group)
        .is_some_and(|accounts| accounts.contains(account))
}

/// Whether new agents of `group` may start on `account`: the global switch
/// and the group's are both on.
pub fn is_enabled_for(state: &State, account: &str, group: Group) -> bool {
    is_enabled(state, account) && is_group_switch_on(state, account, group)
}

/// Every account a new agent of `group` must not start on: the ones the
/// user switched off, and the ones a provider refused a while ago.
pub fn disabled_for(state: &State, group: Group) -> BTreeSet<String> {
    let mut disabled = state.disabled_accounts.clone();
    if let Some(accounts) = state.disabled_group_accounts.get(&group) {
        disabled.extend(accounts.iter().cloned());
    }
    disabled.extend(held(state, crate::log::now()).map(str::to_string));
    disabled
}

/// How long an account a provider refused is held back from new agents.
pub const HOLD_SECONDS: u64 = 30 * 60;

/// The accounts held back at `now`.
pub fn held(state: &State, now: u64) -> impl Iterator<Item = &str> {
    state
        .account_holds
        .iter()
        .filter(move |(_, hold)| hold.until > now)
        .map(|(account, _)| account.as_str())
}

#[cfg(test)]
fn registered(devws: &dyn Devws) -> Result<Vec<String>> {
    Ok(devws
        .accounts()?
        .into_iter()
        .map(|(name, _)| name)
        .collect())
}

fn ensure_registered(registered: &[String], account: &str) -> Result<()> {
    if !registered.iter().any(|name| name == account) {
        bail!("there is no account {account} in devws");
    }
    Ok(())
}

/// The user turns `account` on or off for this task. The last account that
/// is on stays on: a team with none could not start anyone.
#[cfg(test)]
pub fn set_enabled(task: &Task, devws: &dyn Devws, account: &str, on: bool) -> Result<()> {
    set_enabled_among(task, &registered(devws)?, account, on)
}

/// `set_enabled` among the `registered` accounts the caller has already:
/// the board, which a devws call per press made slow.
pub fn set_enabled_among(
    task: &Task,
    registered: &[String],
    account: &str,
    on: bool,
) -> Result<()> {
    ensure_registered(registered, account)?;
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

/// The user turns `account` on or off for one worker group of this task. The
/// group keeps at least one account that is on for it.
#[cfg(test)]
pub fn set_group_enabled(
    task: &Task,
    devws: &dyn Devws,
    account: &str,
    group: Group,
    on: bool,
) -> Result<()> {
    set_group_enabled_among(task, &registered(devws)?, account, group, on)
}

/// `set_group_enabled` among the `registered` accounts the caller has.
pub fn set_group_enabled_among(
    task: &Task,
    registered: &[String],
    account: &str,
    group: Group,
    on: bool,
) -> Result<()> {
    ensure!(
        WORKER_GROUPS.contains(&group),
        "the {} follows the global account switches only",
        group.label()
    );
    ensure_registered(registered, account)?;
    let name = group.name();
    task.transaction(|tx| {
        if on == is_group_switch_on(&tx.state, account, group) {
            return Ok(());
        }
        let accounts = tx.state.disabled_group_accounts.entry(group).or_default();
        if on {
            accounts.remove(account);
            if accounts.is_empty() {
                tx.state.disabled_group_accounts.remove(&group);
            }
        } else {
            accounts.insert(account.to_string());
            let others_on = registered
                .iter()
                .any(|other| other != account && is_enabled_for(&tx.state, other, group));
            ensure!(
                others_on,
                "{account} is the last account that is on for the {name} group: turn another one on first"
            );
        }
        let word = if on { "on" } else { "off" };
        tx.state.say(
            LEADER,
            format!(
                "The user turned {account} {word} for the {name} group of this task. Start new {name} agents only on accounts that are on for it (task_status accounts)."
            ),
        );
        tx.log(
            Kind::Review,
            None,
            None,
            &format!("the user turned {account} {word} for the {name} group"),
        );
        Ok(())
    })
}

/// The model the user picked for `group`'s agents on `account`, if any.
pub fn group_model<'a>(state: &'a State, account: &str, group: Group) -> Option<&'a str> {
    state
        .group_models
        .get(&group)
        .and_then(|models| models.get(account))
        .map(String::as_str)
}

/// The OpenCode settings with this task's group models on the opencode
/// account in place of the home-wide ones.
pub fn opencode_for_task(
    mut settings: crate::opencode::OpencodeSettings,
    state: &State,
) -> crate::opencode::OpencodeSettings {
    for group in WORKER_GROUPS {
        if let Some(model) = group_model(state, &settings.account, group) {
            let model = model.to_string();
            if let Ok(group_settings) = settings.group_mut(group) {
                group_settings.model = Some(model);
            }
        }
    }
    settings
}

/// The user picks the model `group`'s new agents run on `account` for this
/// task; None goes back to each role's own (or the program's default).
pub fn set_group_model(
    task: &Task,
    devws: &dyn Devws,
    account: &str,
    group: Group,
    model: Option<String>,
) -> Result<()> {
    ensure!(
        WORKER_GROUPS.contains(&group),
        "the {} keeps its own model",
        group.label()
    );
    let kind = devws.agent_kind(account)?;
    let model = model
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty());
    if let Some(model) = &model {
        ensure!(
            !kind.provider().provider_models() || model.contains('/'),
            "name the model as provider/model, as `opencode models` lists it: {model}"
        );
    }
    let name = group.name();
    task.transaction(|tx| {
        if group_model(&tx.state, account, group) == model.as_deref() {
            return Ok(());
        }
        let models = tx.state.group_models.entry(group).or_default();
        match &model {
            Some(model) => {
                models.insert(account.to_string(), model.clone());
            }
            None => {
                models.remove(account);
                if models.is_empty() {
                    tx.state.group_models.remove(&group);
                }
            }
        }
        let what = model.as_deref().unwrap_or("each role's own model");
        tx.state.say(
            LEADER,
            format!(
                "The user set the {name} group's model on {account} to {what} for this task. New {name} agents on {account} use it (task_status accounts)."
            ),
        );
        tx.log(
            Kind::Review,
            None,
            None,
            &format!("the user set the {name} model on {account} to {what}"),
        );
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devws::fake::FakeDevws;
    use crate::home::Home;
    use crate::providers::Kind as Program;
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

    #[test]
    fn an_account_is_turned_off_for_one_group_only() {
        let (_dir, _repo, task, devws) = task();
        set_group_enabled(&task, &devws, "claude", Group::Floor, false).unwrap();
        let state = task.state().unwrap();
        assert!(is_enabled(&state, "claude"), "the global switch stays on");
        assert!(!is_enabled_for(&state, "claude", Group::Floor));
        assert!(is_enabled_for(&state, "claude", Group::Senior));
        assert!(disabled_for(&state, Group::Floor).contains("claude"));
        assert!(!disabled_for(&state, Group::Senior).contains("claude"));
        assert!(state.queue.iter().any(|message| {
            message.to == LEADER
                && message
                    .text
                    .contains("turned claude off for the floor group")
        }));

        set_group_enabled(&task, &devws, "claude", Group::Floor, true).unwrap();
        let state = task.state().unwrap();
        assert!(is_enabled_for(&state, "claude", Group::Floor));
        assert!(state.disabled_group_accounts.is_empty());
    }

    #[test]
    fn the_global_switch_holds_a_group_switch_off() {
        let (_dir, _repo, task, devws) = task();
        set_enabled(&task, &devws, "codex", false).unwrap();
        let state = task.state().unwrap();
        assert!(is_group_switch_on(&state, "codex", Group::Senior));
        assert!(!is_enabled_for(&state, "codex", Group::Senior));
        assert!(disabled_for(&state, Group::Senior).contains("codex"));
    }

    #[test]
    fn the_last_account_on_for_a_group_stays_on() {
        let (_dir, _repo, task, devws) = task();
        set_enabled(&task, &devws, "claude", false).unwrap();
        set_group_enabled(&task, &devws, "claude_work", Group::Senior, false).unwrap();
        let error = set_group_enabled(&task, &devws, "codex", Group::Senior, false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("last account that is on for the senior group"),
            "{error}"
        );
        assert!(is_enabled_for(
            &task.state().unwrap(),
            "codex",
            Group::Senior
        ));
        set_group_enabled(&task, &devws, "codex", Group::Floor, false).unwrap();
    }

    #[test]
    fn the_leader_has_no_group_switch() {
        let (_dir, _repo, task, devws) = task();
        assert!(set_group_enabled(&task, &devws, "claude", Group::Cto, false).is_err());
    }

    #[test]
    fn a_group_model_is_set_for_one_account_and_cleared() {
        let (_dir, _repo, task, devws) = task();
        set_group_model(
            &task,
            &devws,
            "codex",
            Group::Floor,
            Some(" gpt-6-luna ".into()),
        )
        .unwrap();
        let state = task.state().unwrap();
        assert_eq!(
            group_model(&state, "codex", Group::Floor),
            Some("gpt-6-luna")
        );
        assert_eq!(group_model(&state, "codex", Group::Senior), None);
        assert!(state.queue.iter().any(|message| {
            message.to == LEADER
                && message
                    .text
                    .contains("floor group's model on codex to gpt-6-luna")
        }));

        set_group_model(&task, &devws, "codex", Group::Floor, None).unwrap();
        assert!(task.state().unwrap().group_models.is_empty());
        assert!(set_group_model(&task, &devws, "codex", Group::Cto, Some("x".into())).is_err());
    }

    #[test]
    fn an_opencode_group_model_names_its_provider_and_replaces_the_home_wide_one() {
        let (_dir, _repo, task, devws) = task();
        let devws = devws.with_kind("opencode", Program::Opencode);
        let error = set_group_model(
            &task,
            &devws,
            "opencode",
            Group::Floor,
            Some("flash".into()),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("provider/model"), "{error}");
        set_group_model(
            &task,
            &devws,
            "opencode",
            Group::Floor,
            Some("deepseek/deepseek-flash".into()),
        )
        .unwrap();
        let settings = crate::opencode::OpencodeSettings {
            enabled: true,
            floor: crate::opencode::GroupSettings {
                enabled: true,
                model: Some("opencode/glm-5.3-flash-free".into()),
            },
            ..Default::default()
        };
        let settings = opencode_for_task(settings, &task.state().unwrap());
        assert_eq!(
            settings.floor.model.as_deref(),
            Some("deepseek/deepseek-flash")
        );
        assert_eq!(settings.senior.model, None);
    }
}
