//! Lanes: areas of a repo (back, front, docs…) the factory floor works on at
//! the same time. Each lane has a worktree on a branch of its own off the
//! task's; an order catches its lane up with the task's branch first, and an
//! accepted step merges the lane into it. Areas that do not overlap never
//! clash.

use crate::checks::{self, git};
use crate::log::Kind;
use crate::state::Agent;
use crate::task::{Lane, Repo, Task};
use anyhow::{Context, Result, bail};
use std::path::Path;

fn find<'c>(lanes: &'c [Lane], name: &str) -> Result<&'c Lane> {
    lanes
        .iter()
        .find(|lane| lane.name == name)
        .with_context(|| {
            let known: Vec<_> = lanes.iter().map(|lane| lane.name.as_str()).collect();
            format!(
                "the task has no lane {name} (it has: {}); add_lane adds one",
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            )
        })
}

/// A new lane of a repo (the task's first by default): its worktree in the
/// task's folder (`lanes/<name>`), on `<task branch>--<name>`, from where the
/// task's branch is now.
pub fn add(task: &Task, name: &str, globs: &[String], repo_name: Option<&str>) -> Result<Lane> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("a lane's name uses lowercase letters, digits and -: {name}");
    }
    if globs.is_empty() {
        bail!("a lane needs its files: globs like backend/**");
    }
    let mut config = task.config()?;
    if config.lanes.iter().any(|lane| lane.name == name) {
        bail!("the task already has the lane {name}");
    }
    let repo = match repo_name {
        Some(repo_name) => config
            .repos
            .iter()
            .find(|repo| repo.name == repo_name)
            .with_context(|| format!("the task has no repo {repo_name}"))?,
        None => config.repos.first().context("the task has no repos")?,
    }
    .clone();
    let branch = format!("{}--{name}", config.branch);
    let worktree = task.dir().join("lanes").join(name);
    if !worktree.exists() {
        let path = worktree.to_string_lossy().into_owned();
        let has_branch = !git(&repo.worktree, &["branch", "--list", &branch])?
            .trim()
            .is_empty();
        if has_branch {
            git(&repo.worktree, &["worktree", "add", &path, &branch])?;
        } else {
            git(
                &repo.worktree,
                &["worktree", "add", "-b", &branch, &path, "HEAD"],
            )?;
        }
    }
    let lane = Lane {
        name: name.to_string(),
        repo: repo.name.clone(),
        globs: globs.to_vec(),
        branch,
        worktree,
    };
    config.lanes.push(lane.clone());
    task.write_config(&config)?;
    task.transaction(|tx| {
        tx.log(
            Kind::Repo,
            None,
            None,
            &format!(
                "lane {name} of {} ({}) at {}",
                lane.repo,
                lane.globs.join(", "),
                lane.worktree.display()
            ),
        );
        Ok(())
    })?;
    Ok(lane)
}

pub fn get(task: &Task, name: &str) -> Result<Lane> {
    Ok(find(&task.config()?.lanes, name)?.clone())
}

/// Where an agent works: its lane's worktree, or its repo's. The repo's check
/// goes with it.
pub fn workplace(task: &Task, agent: &Agent) -> Result<Repo> {
    let config = task.config()?;
    let mut repo = config
        .repos
        .iter()
        .find(|repo| repo.name == agent.repo)
        .with_context(|| format!("the task has no repo {}", agent.repo))?
        .clone();
    if let Some(name) = &agent.lane {
        repo.worktree = find(&config.lanes, name)?.worktree.clone();
    }
    Ok(repo)
}

/// The files a merge left in conflict, after which the merge is undone.
fn abort_merge(worktree: &Path) -> String {
    let files = git(worktree, &["diff", "--name-only", "--diff-filter=U"])
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(", ");
    let _ = git(worktree, &["merge", "--abort"]);
    files
}

fn merge(worktree: &Path, branch: &str, what: &str) -> Result<()> {
    let work = checks::inspect(worktree, "HEAD", &[], &[])?;
    if !work.uncommitted.is_empty() {
        bail!(
            "{what}: {} has uncommitted changes ({})",
            worktree.display(),
            work.uncommitted.join(", ")
        );
    }
    let Err(error) = git(worktree, &["merge", "--no-edit", branch]) else {
        return Ok(());
    };
    let files = abort_merge(worktree);
    // No file in conflict: the merge failed for another reason (a repo
    // locked by another git command…), which git says.
    if files.is_empty() {
        bail!("{what}: {error:#}; nothing merged");
    }
    bail!(
        "{what}: the lanes clash in {files}: the plan gave the same files to two lanes; undone, nothing merged"
    )
}

/// Before an order: the lane takes what the other lanes merged into the
/// task's branch.
/// A lane that already has the task's branch is left as it is: a step being
/// redone keeps its uncommitted edits.
pub fn catch_up(task: &Task, lane: &Lane) -> Result<()> {
    let branch = task.config()?.branch;
    if git(
        &lane.worktree,
        &["merge-base", "--is-ancestor", &branch, "HEAD"],
    )
    .is_ok()
    {
        return Ok(());
    }
    merge(
        &lane.worktree,
        &branch,
        &format!("catching lane {} up", lane.name),
    )
}

/// An accepted step: the lane's work goes into the task's branch.
pub fn merge_into_task(task: &Task, lane: &Lane) -> Result<()> {
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == lane.repo)
        .with_context(|| format!("the task has no repo {}", lane.repo))?;
    merge(
        &repo.worktree,
        &lane.branch,
        &format!("merging lane {}", lane.name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::tests::started;
    use crate::checks::tests::commit_file;
    use tempfile::TempDir;

    #[test]
    fn a_lane_is_a_worktree_on_its_own_branch_off_the_tasks() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = started(&dir);
        let lane = add(&task, "back", &["backend/**".into()], None).unwrap();
        assert_eq!(lane.branch, "feat/x--back");
        assert!(lane.worktree.join(".git").exists(), "{:?}", lane.worktree);
        assert_eq!(
            checks::head(&lane.worktree).unwrap(),
            checks::head(repo.path()).unwrap()
        );
        assert_eq!(task.config().unwrap().lanes, std::slice::from_ref(&lane));
        assert!(add(&task, "back", &["x".into()], None).is_err(), "twice");
        assert!(add(&task, "Back End", &["x".into()], None).is_err());
        assert!(add(&task, "docs", &[], None).is_err(), "no files");
        assert!(
            get(&task, "front")
                .unwrap_err()
                .to_string()
                .contains("add_lane")
        );
    }

    /// A lane's agent may commit: its git folders are writable, `.git`
    /// itself is not named (Codex keeps it read-only even when added).
    #[test]
    fn a_lanes_git_folders_are_named_one_by_one() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = started(&dir);
        let back = add(&task, "back", &["backend/**".into()], None).unwrap();
        let common = repo.path().canonicalize().unwrap().join(".git");
        let folders = checks::git_data(&back.worktree).unwrap();
        assert_eq!(
            folders,
            [
                common.join("worktrees/back"),
                common.join("objects"),
                common.join("refs"),
                common.join("logs")
            ],
        );
    }

    #[test]
    fn lanes_work_side_by_side_and_merge_into_the_tasks_branch() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = started(&dir);
        let back = add(&task, "back", &["backend/**".into()], None).unwrap();
        let front = add(&task, "front", &["frontend/**".into()], None).unwrap();
        commit_file(&back.worktree, "backend/api.rs", "api\n", "api");
        commit_file(&front.worktree, "frontend/page.ts", "page\n", "page");

        merge_into_task(&task, &back).unwrap();
        merge_into_task(&task, &front).unwrap();
        assert!(repo.path().join("backend/api.rs").exists());
        assert!(repo.path().join("frontend/page.ts").exists());

        catch_up(&task, &back).unwrap();
        assert!(
            back.worktree.join("frontend/page.ts").exists(),
            "the back lane has the front's work"
        );
    }

    /// Two hammers, in the back and front lanes, ordered at once; each step
    /// accepted merges its lane, and the next order takes the other's work.
    #[test]
    fn two_lanes_work_at_once_from_spawn_to_merge() {
        use crate::orders::{OrderRequest, Outcome, accept_step, report, send_order};
        use crate::state::{ExecutionPlan, Step, StepStatus};
        use crate::team::Team;
        let dir = TempDir::new().unwrap();
        let (home, devws, task, repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        team.write_plan("1. p1\n").unwrap();
        team.approve().unwrap();
        task.transaction(|tx| {
            tx.state.steps.push(Step {
                id: "p1".into(),
                title: "plan".into(),
                status: StepStatus::Accepted,
                attempts: 1,
                review_since: None,
            });
            tx.state.plans.push(ExecutionPlan {
                step: "p1".into(),
                author: "planner-1".into(),
            });
            Ok(())
        })
        .unwrap();
        let back = add(&task, "back", &["backend/**".into()], None).unwrap();
        let front = add(&task, "front", &["frontend/**".into()], None).unwrap();

        let error = team.spawn_agent("executor", None).unwrap_err().to_string();
        assert!(error.contains("has lanes (back, front)"), "{error}");
        let one = team.spawn_agent_in("executor", None, Some("back")).unwrap();
        let two = team
            .spawn_agent_in("executor", None, Some("front"))
            .unwrap();
        let error = team
            .spawn_agent_in("executor", None, Some("back"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("already writes in the lane back"), "{error}");
        assert!(
            devws
                .calls()
                .iter()
                .any(|call| call.ends_with(&format!(" in {}", back.worktree.display()))),
            "the back agent opens in its lane: {:?}",
            devws.calls()
        );

        let order = |agent: &str, step: &str| {
            send_order(
                &task,
                OrderRequest {
                    agent: agent.into(),
                    step: step.into(),
                    title: step.into(),
                    text: "do it".into(),
                    allowed: vec![],
                    forbidden: vec![],
                    check: Some("true".into()),
                    plan: Some("p1".into()),
                },
            )
            .unwrap()
        };
        order(&one.id, "s1");
        order(&two.id, "s2");
        let allowed = |agent: &str| {
            task.state()
                .unwrap()
                .open_order_of(agent)
                .unwrap()
                .allowed
                .clone()
        };
        assert_eq!(
            allowed(&one.id),
            ["backend/**"],
            "the lane's files by default"
        );
        commit_file(&back.worktree, "backend/api.rs", "api\n", "api");
        commit_file(&front.worktree, "frontend/page.ts", "page\n", "page");
        report(
            &task,
            &one.pane,
            Outcome::Done,
            "api",
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        report(
            &task,
            &two.pane,
            Outcome::Done,
            "page",
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        accept_step(&task, "s1").unwrap();
        accept_step(&task, "s2").unwrap();
        assert!(
            repo.path().join("backend/api.rs").exists()
                && repo.path().join("frontend/page.ts").exists()
        );

        order(&one.id, "s3");
        assert!(
            back.worktree.join("frontend/page.ts").exists(),
            "the next order took the front's accepted work"
        );
    }

    /// A lane with a step's edits still uncommitted (a retry of the same
    /// step) takes its next order when it has nothing to catch up with; when
    /// the task's branch moved on, it is refused, and its edits stay.
    #[test]
    fn a_lane_up_to_date_keeps_its_uncommitted_edits() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = started(&dir);
        let back = add(&task, "back", &["backend/**".into()], None).unwrap();
        commit_file(&back.worktree, "backend/api.rs", "api\n", "api");
        std::fs::write(back.worktree.join("backend/api.rs"), "api fixed\n").unwrap();
        catch_up(&task, &back).unwrap();
        assert_eq!(
            std::fs::read_to_string(back.worktree.join("backend/api.rs")).unwrap(),
            "api fixed\n",
            "the edit stays"
        );

        commit_file(repo.path(), "docs/x.md", "x\n", "docs");
        let error = catch_up(&task, &back).unwrap_err().to_string();
        assert!(
            error.contains("uncommitted changes (backend/api.rs)"),
            "{error}"
        );
        assert_eq!(
            std::fs::read_to_string(back.worktree.join("backend/api.rs")).unwrap(),
            "api fixed\n"
        );
    }

    /// A merge that fails without a conflict (a repo locked by another git)
    /// says git's own error, not an empty clash.
    #[test]
    fn a_merge_that_fails_otherwise_says_why() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = started(&dir);
        let back = add(&task, "back", &["backend/**".into()], None).unwrap();
        commit_file(&back.worktree, "backend/api.rs", "api\n", "api");
        let lock = repo.path().join(".git/index.lock");
        std::fs::write(&lock, "").unwrap();
        let error = merge_into_task(&task, &back).unwrap_err().to_string();
        assert!(!error.contains("clash"), "{error}");
        assert!(
            error.contains("index.lock"),
            "git's error is shown: {error}"
        );
        std::fs::remove_file(&lock).unwrap();
        merge_into_task(&task, &back).unwrap();
    }

    #[test]
    fn a_clash_is_undone_and_names_the_files() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = started(&dir);
        let back = add(&task, "back", &["shared/**".into()], None).unwrap();
        let front = add(&task, "front", &["shared/**".into()], None).unwrap();
        commit_file(&back.worktree, "shared/x", "back\n", "b");
        commit_file(&front.worktree, "shared/x", "front\n", "f");
        merge_into_task(&task, &back).unwrap();
        let error = merge_into_task(&task, &front).unwrap_err().to_string();
        assert!(error.contains("clash in shared/x"), "{error}");
        assert_eq!(
            std::fs::read_to_string(repo.path().join("shared/x")).unwrap(),
            "back\n",
            "the task's branch is as it was"
        );
        assert!(
            checks::inspect(repo.path(), "HEAD", &[], &[])
                .unwrap()
                .uncommitted
                .is_empty(),
            "no half merge left"
        );
    }
}
