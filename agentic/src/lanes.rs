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
    let _control = task.control_lock()?;
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
    open_worktree(&repo.worktree, &branch, &worktree)?;
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

/// A worktree on `branch` at `path`, from the task's branch when the branch
/// is new; one that is there already is kept as it is.
fn open_worktree(repo: &Path, branch: &str, path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let path = path.to_string_lossy().into_owned();
    let has_branch = !git(repo, &["branch", "--list", branch])?.trim().is_empty();
    if has_branch {
        git(repo, &["worktree", "add", &path, branch])?;
    } else {
        git(repo, &["worktree", "add", "-b", branch, &path, "HEAD"])?;
    }
    Ok(())
}

pub fn get(task: &Task, name: &str) -> Result<Lane> {
    Ok(find(&task.config()?.lanes, name)?.clone())
}

/// Seat `number` of a lane: one more writer's worktree, on a branch of its
/// own, with the lane's name, repo and files, so its checks, image and plan
/// items are the lane's. Seats 0 and 1 are the lane itself. A lane's name
/// has no dot, so `front.2` is never another lane's.
pub fn seat(task: &Task, lane: &Lane, number: u32) -> Lane {
    if number <= 1 {
        return lane.clone();
    }
    Lane {
        branch: format!("{}.{number}", lane.branch),
        worktree: task
            .dir()
            .join("lanes")
            .join(format!("{}.{number}", lane.name)),
        ..lane.clone()
    }
}

/// The seat's worktree, made from the task's branch the first time.
pub fn open_seat(task: &Task, lane: &Lane, number: u32) -> Result<Lane> {
    let seat = seat(task, lane, number);
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == lane.repo)
        .with_context(|| format!("the task has no repo {}", lane.repo))?;
    open_worktree(&repo.worktree, &seat.branch, &seat.worktree)?;
    Ok(seat)
}

/// The lane an agent writes in, as its own seat sees it.
pub fn of_agent(task: &Task, agent: &Agent) -> Result<Option<Lane>> {
    let Some(name) = &agent.lane else {
        return Ok(None);
    };
    Ok(Some(seat(task, &get(task, name)?, agent.seat)))
}

/// Where an agent works: its seat's worktree in its lane, or its repo's. The
/// repo's check goes with it.
pub fn workplace(task: &Task, agent: &Agent) -> Result<Repo> {
    let config = task.config()?;
    let mut repo = config
        .repos
        .iter()
        .find(|repo| repo.name == agent.repo)
        .with_context(|| format!("the task has no repo {}", agent.repo))?
        .clone();
    if let Some(lane) = of_agent(task, agent)? {
        repo.worktree = lane.worktree;
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
    crate::workflow::block_unverified_runner(task)?;
    let branch = task.config()?.branch;
    if git(
        &lane.worktree,
        &["merge-base", "--is-ancestor", &branch, "HEAD"],
    )
    .is_ok()
    {
        return Ok(());
    }
    if task.state()?.workflow == crate::execution::Workflow::Reviewed {
        return catch_up_reviewed(&lane.worktree, &branch, &lane.name);
    }
    merge(
        &lane.worktree,
        &branch,
        &format!("catching lane {} up", lane.name),
    )
}

/// A reviewed lane takes no code git combined unreviewed: the task's branch
/// by fast-forward, or by a merge when the files the branch changed since
/// they parted (the team's history, other lanes' accepted work) are not
/// files the lane changed. Each merged file is then one reviewed side, and
/// the lane's own work is reviewed as its candidate.
fn catch_up_reviewed(worktree: &Path, branch: &str, lane: &str) -> Result<()> {
    anyhow::ensure!(
        checks::inspect(worktree, "HEAD", &[], &[])?
            .uncommitted
            .is_empty(),
        "lane has uncommitted changes"
    );
    if git(worktree, &["merge", "--ff-only", "--no-edit", branch]).is_ok() {
        return Ok(());
    }
    let changed = |range: String| -> Result<std::collections::BTreeSet<String>> {
        Ok(git(worktree, &["diff", "--name-only", &range])?
            .lines()
            .map(str::to_string)
            .collect())
    };
    let on_branch = changed(format!("HEAD...{branch}"))?;
    let in_lane = changed(format!("{branch}...HEAD"))?;
    let both: Vec<&str> = on_branch
        .intersection(&in_lane)
        .map(String::as_str)
        .collect();
    if !both.is_empty() {
        bail!(
            "divergent lane requires a separately reviewed integration: the task's branch and lane {lane} both changed {}; no automatic conflict-resolution code was merged",
            both.join(", ")
        );
    }
    merge(
        worktree,
        branch,
        &format!("catching lane {lane} up with the task's branch"),
    )
}

/// An accepted step: the lane's work goes into the task's branch.
pub fn merge_into_task(task: &Task, lane: &Lane, step: &str) -> Result<()> {
    crate::workflow::block_unverified_runner(task)?;
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == lane.repo)
        .with_context(|| format!("the task has no repo {}", lane.repo))?;
    if task.state()?.workflow == crate::execution::Workflow::Reviewed {
        let state = task.state()?;
        let order = state
            .orders
            .iter()
            .rev()
            .find(|order| {
                order.step == step
                    && state
                        .agent(&order.agent)
                        .is_some_and(|agent| agent.lane.as_deref() == Some(&lane.name))
                    && state.steps.iter().any(|step| {
                        step.id == order.step && step.status == crate::state::StepStatus::Reported
                    })
            })
            .with_context(|| {
                format!(
                    "no reported implementation for step {step} in lane {}",
                    lane.name
                )
            })?;
        let review = state
            .reviews
            .iter()
            .rev()
            .find(|review| review.order == order.id)
            .context("independent review required")?;
        anyhow::ensure!(
            review.verdict == Some(crate::reviews::Verdict::Approved),
            "independent review not approved"
        );
        return crate::integration::merge(task, lane, &order.step, review);
    }
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

        merge_into_task(&task, &back, "legacy-step").unwrap();
        merge_into_task(&task, &front, "legacy-step").unwrap();
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
                spec: None,
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
        let second = team.spawn_agent_in("executor", None, Some("back")).unwrap();
        assert_eq!(
            second.seat, 2,
            "a second writer in a lane takes its next seat"
        );
        let error = team
            .spawn_agent_in("executor", None, Some("back"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("3 writers are running"),
            "max_agents still caps every writer: {error}"
        );
        team.stop_agent(&second.id).unwrap();
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
                    read_lane: None,
                    diagnosis: None,
                    read: Vec::new(),
                    targets: Vec::new(),
                    profiles: Vec::new(),
                    agent: agent.into(),
                    step: step.into(),
                    title: step.into(),
                    text: "do it".into(),
                    allowed: vec![],
                    forbidden: vec![],
                    check: Some("true".into()),
                    plan: Some("p1".into()),
                    spec: None,
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
        let orders = task
            .transaction(|tx| {
                for order in &mut tx.state.orders {
                    order.delivered = true;
                }
                Ok(tx.state.orders.clone())
            })
            .unwrap();
        report(
            &task,
            &one.pane,
            &orders
                .iter()
                .find(|order| order.agent == one.id)
                .unwrap()
                .id,
            Outcome::Done,
            "api",
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        report(
            &task,
            &two.pane,
            &orders
                .iter()
                .find(|order| order.agent == two.id)
                .unwrap()
                .id,
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

    /// Two writers in one lane: each in a seat of its own, orders only on
    /// files apart, and both steps merge into the task's branch.
    #[test]
    fn two_seats_of_a_lane_write_apart_and_both_merge() {
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
                spec: None,
                review_since: None,
            });
            tx.state.plans.push(ExecutionPlan {
                step: "p1".into(),
                author: "planner-1".into(),
            });
            Ok(())
        })
        .unwrap();
        let front = add(&task, "front", &["frontend/**".into()], None).unwrap();
        let one = team
            .spawn_agent_in("executor", None, Some("front"))
            .unwrap();
        let two = team
            .spawn_agent_in("executor", None, Some("front"))
            .unwrap();
        assert_eq!((one.seat, two.seat), (1, 2));
        let second = seat(&task, &front, 2);
        assert_eq!(
            second.name, "front",
            "a seat is its lane for checks and items"
        );
        assert_eq!(second.branch, "feat/x--front.2");
        assert!(
            second.worktree.join(".git").exists(),
            "{:?}",
            second.worktree
        );
        assert_ne!(second.worktree, front.worktree);
        assert!(
            devws
                .calls()
                .iter()
                .any(|call| call.ends_with(&format!(" in {}", second.worktree.display()))),
            "the second writer opens in its seat: {:?}",
            devws.calls()
        );

        let order = |agent: &str, step: &str, allowed: &[&str]| {
            send_order(
                &task,
                OrderRequest {
                    read_lane: None,
                    diagnosis: None,
                    read: Vec::new(),
                    targets: Vec::new(),
                    profiles: Vec::new(),
                    agent: agent.into(),
                    step: step.into(),
                    title: step.into(),
                    text: "do it".into(),
                    allowed: allowed.iter().map(|glob| glob.to_string()).collect(),
                    forbidden: vec![],
                    check: Some("true".into()),
                    plan: Some("p1".into()),
                    spec: None,
                },
            )
        };
        order(&one.id, "s1", &["frontend/team/**"]).unwrap();
        let error = order(&two.id, "s2", &["frontend/**"])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("s2 can't run next to")
                && error.contains("(s1, executor-1)")
                && error.contains("frontend/** and frontend/team/**"),
            "{error}"
        );
        // A step's unmerged work is in its seat's worktree: another seat
        // starts from the task's branch, without it.
        let error = order(&two.id, "s1", &["frontend/team/**"])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("s1's work is in seat 1 of the lane front (executor-1"),
            "{error}"
        );
        order(&two.id, "s2", &["frontend/history/**"]).unwrap();
        let second_order = task
            .state()
            .unwrap()
            .open_order_of(&two.id)
            .unwrap()
            .id
            .clone();
        let error = crate::orders::amend(
            &task,
            &second_order,
            vec!["frontend/team/shared.ts".into()],
            "it needs the rail's type",
            "leader",
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("can't widen to frontend/team/shared.ts next to"),
            "an amendment stays apart from the other seat: {error}"
        );

        commit_file(&front.worktree, "frontend/team/rail.ts", "rail\n", "rail");
        commit_file(
            &second.worktree,
            "frontend/history/empty.ts",
            "empty\n",
            "empty",
        );
        let orders = task
            .transaction(|tx| {
                for order in &mut tx.state.orders {
                    order.delivered = true;
                }
                Ok(tx.state.orders.clone())
            })
            .unwrap();
        for (agent, summary) in [(&one, "rail"), (&two, "empty")] {
            let id = &orders
                .iter()
                .find(|order| order.agent == agent.id)
                .unwrap()
                .id;
            report(
                &task,
                &agent.pane,
                id,
                Outcome::Done,
                summary,
                std::time::Duration::from_secs(5),
            )
            .unwrap();
        }
        accept_step(&task, "s1").unwrap();
        accept_step(&task, "s2").unwrap();
        assert!(
            repo.path().join("frontend/team/rail.ts").exists()
                && repo.path().join("frontend/history/empty.ts").exists(),
            "both seats merged into the task's branch"
        );

        order(&one.id, "s3", &["frontend/history/**"]).unwrap();
        assert!(
            front.worktree.join("frontend/history/empty.ts").exists(),
            "work the other seat merged no longer blocks, and is caught up"
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

    /// A reviewed lane holding its candidate takes what the task's branch got
    /// since (history, other lanes' accepted work), but never a file both
    /// changed: git would combine them into code nobody reviewed.
    /// What a review binds and merges for a seat's writer is its own seat's
    /// work: its worktree, and the merge candidate made from it.
    #[test]
    fn a_seats_writer_is_reviewed_and_merged_from_its_own_seat() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = started(&dir);
        let front = add(&task, "front", &["frontend/**".into()], None).unwrap();
        let second = open_seat(&task, &front, 2).unwrap();
        commit_file(
            &second.worktree,
            "frontend/history.ts",
            "seat\n",
            "seat work",
        );
        commit_file(&front.worktree, "frontend/team.ts", "lane\n", "lane work");
        let writer = Agent {
            id: "executor-2".into(),
            role: "executor".into(),
            agent: "claude".into(),
            repo: front.repo.clone(),
            session: String::new(),
            pane: String::new(),
            writes: true,
            group: None,
            lane: Some("front".into()),
            seat: 2,
            live: true,
            unavailable: false,
            ready: true,
            lost: false,
            usage: None,
        };
        assert_eq!(of_agent(&task, &writer).unwrap(), Some(second.clone()));
        assert_eq!(workplace(&task, &writer).unwrap().worktree, second.worktree);
        let candidate = crate::integration::candidate(&task, &second).unwrap();
        assert_eq!(candidate.source, checks::head(&second.worktree).unwrap());
        assert_ne!(candidate.source, checks::head(&front.worktree).unwrap());
        assert_eq!(
            open_seat(&task, &front, 2).unwrap(),
            second,
            "an open seat is kept as it is"
        );
        assert!(second.worktree.join("frontend/history.ts").exists());
    }

    #[test]
    fn a_reviewed_lane_takes_only_the_teams_history_by_merge() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = started(&dir);
        let branch = task.config().unwrap().branch;
        let front = add(&task, "front", &["frontend/**".into()], None).unwrap();
        commit_file(&front.worktree, "frontend/page.ts", "page\n", "candidate");
        commit_file(
            repo.path(),
            ".devagentics/history/feat-x/history.json",
            "{}\n",
            "record a planning step",
        );
        catch_up_reviewed(&front.worktree, &branch, "front").unwrap();
        assert!(
            front
                .worktree
                .join(".devagentics/history/feat-x/history.json")
                .exists()
        );
        assert!(
            front.worktree.join("frontend/page.ts").exists(),
            "the candidate stays"
        );

        // Another lane's accepted work, in files the lane did not change:
        // each merged file is one reviewed side, nothing git combined.
        commit_file(repo.path(), "backend/api.rs", "api\n", "accepted back step");
        catch_up_reviewed(&front.worktree, &branch, "front").unwrap();
        assert!(front.worktree.join("backend/api.rs").exists());
        assert!(front.worktree.join("frontend/page.ts").exists());

        // The branch changed a file the lane changed too: git would combine
        // them into code nobody reviewed.
        commit_file(&front.worktree, "frontend/menu.ts", "lane\n", "candidate 2");
        commit_file(repo.path(), "frontend/menu.ts", "branch\n", "other work");
        let error = catch_up_reviewed(&front.worktree, &branch, "front")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("separately reviewed integration") && error.contains("frontend/menu.ts"),
            "{error}"
        );
        assert_eq!(
            std::fs::read_to_string(front.worktree.join("frontend/menu.ts")).unwrap(),
            "lane\n"
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
        let error = merge_into_task(&task, &back, "legacy-step")
            .unwrap_err()
            .to_string();
        assert!(!error.contains("clash"), "{error}");
        assert!(
            error.contains("index.lock"),
            "git's error is shown: {error}"
        );
        std::fs::remove_file(&lock).unwrap();
        merge_into_task(&task, &back, "legacy-step").unwrap();
    }

    #[test]
    fn a_clash_is_undone_and_names_the_files() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = started(&dir);
        let back = add(&task, "back", &["shared/**".into()], None).unwrap();
        let front = add(&task, "front", &["shared/**".into()], None).unwrap();
        commit_file(&back.worktree, "shared/x", "back\n", "b");
        commit_file(&front.worktree, "shared/x", "front\n", "f");
        merge_into_task(&task, &back, "legacy-step").unwrap();
        let error = merge_into_task(&task, &front, "legacy-step")
            .unwrap_err()
            .to_string();
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
