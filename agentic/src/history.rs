//! The team's history, kept in the repo so the work can go on elsewhere: what
//! was accepted, the approved plan and the seniors' execution plans, in
//! `.devagentics/history/<task>/` on the task's branch. Every accepted step
//! commits it (`save`); a task started on that branch in another home takes
//! it back (`load`), so DONE, the release versions and the plan carry on.

use crate::log::Kind;
use crate::specs;
use crate::state::{Step, StepStatus};
use crate::task::Task;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Where histories live in a repo; the work check leaves this folder out.
pub const FOLDER: &str = ".devagentics/history";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct History {
    task: String,
    branch: String,
    approved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    application: Option<crate::execution::Application>,
    #[serde(default)]
    reviews: Vec<crate::reviews::Review>,
    #[serde(default)]
    integrations: std::collections::BTreeMap<String, crate::integration::Journal>,
    #[serde(default)]
    next_id: u64,
    /// Accepted steps, in the order they were first ordered.
    steps: Vec<Done>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Done {
    id: String,
    title: String,
    attempts: u32,
    /// Its spec's release version (`1.83.0`): a loaded step is filed under it,
    /// so its spec keeps its number.
    version: String,
}

/// A task's history folder in `worktree`.
fn folder(worktree: &Path, task_id: &str) -> PathBuf {
    worktree.join(FOLDER).join(task_id)
}

fn git(worktree: &Path, arguments: &[&str]) -> Result<String> {
    crate::runner::safe_git(worktree, arguments)
}

/// Whether `worktree`'s HEAD is `commit` followed only by history commits:
/// the team records an accepted step or plan on the branch while reviews of
/// other work are open, and that must not make their reviewed code stale.
pub fn only_history_since(worktree: &Path, commit: &str) -> Result<bool> {
    if git(worktree, &["rev-parse", "HEAD"])?.trim() == commit {
        return Ok(true);
    }
    if git(worktree, &["merge-base", "--is-ancestor", commit, "HEAD"]).is_err() {
        return Ok(false);
    }
    let changed = git(
        worktree,
        &["diff", "--name-only", &format!("{commit}..HEAD")],
    )?;
    let history = format!("{FOLDER}/");
    Ok(changed.lines().all(|path| path.starts_with(&history)))
}

/// Copies every `*.md` of `from` into `to`; nothing when `from` is missing.
fn copy_plans(from: &Path, to: &Path) -> Result<()> {
    let Ok(entries) = fs::read_dir(from) else {
        return Ok(());
    };
    fs::create_dir_all(to)?;
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "md") {
            fs::copy(
                &path,
                to.join(path.file_name().context("a plan without a name")?),
            )?;
        }
    }
    Ok(())
}

/// Writes the history of what `task` accepted into its first repo's worktree
/// and commits only that folder: after `step` was accepted, or when asked
/// (`devagentics history save`, no step). Returns whether there was anything
/// new to commit.
pub fn save(task: &Task, step: Option<&str>) -> Result<bool> {
    let config = task.config()?;
    let state = task.state()?;
    let worktree = &config
        .repos
        .first()
        .context("the task has no repo")?
        .worktree;
    let versions = specs::versions(&state.steps);
    let version_of = |step: &Step| {
        if state.workflow == crate::execution::Workflow::Reviewed {
            return state
                .application
                .items
                .get(&step.id)
                .map(|item| item.scope.clone())
                .unwrap_or_else(|| "planning".into());
        }
        let spec = specs::spec_of(step);
        versions
            .iter()
            .find(|(name, _)| *name == spec)
            .map_or(spec.clone(), |(_, version)| version.clone())
    };
    let history = History {
        task: config.id.clone(),
        branch: config.branch.clone(),
        approved: state.approved,
        application: if state.workflow == crate::execution::Workflow::Reviewed {
            Some(state.application.clone())
        } else {
            None
        },
        reviews: if state.workflow == crate::execution::Workflow::Reviewed {
            state
                .reviews
                .iter()
                .filter(|review| {
                    state
                        .integrations
                        .values()
                        .any(|journal| journal.accepted && journal.review == review.id)
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        },
        integrations: if state.workflow == crate::execution::Workflow::Reviewed {
            state
                .integrations
                .iter()
                .filter(|(_, journal)| journal.accepted)
                .map(|(id, journal)| (id.clone(), journal.clone()))
                .collect()
        } else {
            Default::default()
        },
        next_id: if state.workflow == crate::execution::Workflow::Reviewed {
            state.next_id
        } else {
            0
        },
        steps: state
            .steps
            .iter()
            .filter(|step| step.status == StepStatus::Accepted)
            .map(|step| Done {
                id: step.id.clone(),
                title: step.title.clone(),
                attempts: step.attempts,
                version: version_of(step),
            })
            .collect(),
    };
    let target = folder(worktree, &config.id);
    fs::create_dir_all(&target)?;
    fs::write(
        target.join("history.json"),
        serde_json::to_string_pretty(&history)? + "\n",
    )?;
    let plan = task.dir().join("plan.md");
    if plan.is_file() {
        fs::copy(&plan, target.join("plan.md"))?;
    }
    copy_plans(&task.dir().join("plans"), &target.join("plans"))?;

    let relative = format!("{FOLDER}/{}", config.id);
    // Forced: a repo that ignores `.devagentics/` still keeps its history.
    git(worktree, &["add", "--force", "--", &relative])?;
    if git(
        worktree,
        &["diff", "--cached", "--name-only", "--", &relative],
    )?
    .is_empty()
    {
        return Ok(false);
    }
    let subject = match step.and_then(|step| state.steps.iter().find(|found| found.id == step)) {
        Some(found) => {
            let number = if state.workflow == crate::execution::Workflow::Reviewed {
                found.id.clone()
            } else {
                specs::step_versions(&state.steps)
                    .get(&found.id)
                    .cloned()
                    .unwrap_or_else(|| found.id.clone())
            };
            format!("chore(team): record {number} {}", found.title)
        }
        None => "chore(team): record the team history".to_string(),
    };
    // The pathspec commits these files alone: an agent's work in progress in
    // the same worktree stays as it is.
    git(
        worktree,
        &["commit", "--quiet", "-m", &subject, "--", &relative],
    )?;
    Ok(true)
}

/// Takes back the history `worktree` has for `task`, a task just made: its
/// accepted steps, whether the plan was approved, the plan and the execution
/// plans. Returns how many steps were loaded; None without a history.
pub fn load(task: &Task, worktree: &Path) -> Result<Option<usize>> {
    let config = task.config()?;
    let source = folder(worktree, &config.id);
    let file = source.join("history.json");
    if !file.is_file() {
        return Ok(None);
    }
    let history: History = serde_json::from_str(&fs::read_to_string(&file)?)
        .with_context(|| format!("{} is not a team history", file.display()))?;
    let plan = source.join("plan.md");
    if plan.is_file() {
        fs::copy(&plan, task.dir().join("plan.md"))?;
    }
    copy_plans(&source.join("plans"), &task.dir().join("plans"))?;
    let count = history.steps.len();
    task.transaction(|tx| {
        tx.state.approved = history.approved;
        if let Some(application) = &history.application {
            application.validate()?;
            tx.state.application = application.clone();
            tx.state.reviews = history.reviews.clone();
            tx.state.integrations = history.integrations.clone();
            tx.state.next_id = tx.state.next_id.max(history.next_id);
        }
        for done in &history.steps {
            if tx.state.steps.iter().any(|step| step.id == done.id) {
                continue;
            }
            tx.state.steps.push(Step {
                id: done.id.clone(),
                title: done.title.clone(),
                status: StepStatus::Accepted,
                attempts: done.attempts,
                spec: Some(done.version.clone()),
                review_since: None,
            });
        }
        tx.log(
            Kind::Start,
            None,
            None,
            &format!(
                "history loaded from {}: {count} accepted steps",
                source.display()
            ),
        );
        Ok(())
    })?;
    Ok(Some(count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_args::Kind as Program;
    use crate::board::tests::started;
    use crate::checks::tests::{commit_file, run};
    use crate::devws::fake::FakeDevws;
    use crate::home::Home;
    use tempfile::TempDir;

    fn git_output(dir: &Path, arguments: &[&str]) -> String {
        git(dir, arguments).unwrap()
    }

    fn accepted(id: &str, attempts: u32, spec: Option<&str>) -> Step {
        Step {
            id: id.into(),
            title: format!("title {id}"),
            status: StepStatus::Accepted,
            attempts,
            spec: spec.map(str::to_string),
            review_since: None,
        }
    }

    /// A started task on a repo that commits as itself, with two specs done,
    /// one at work, an approved plan and an execution plan.
    fn with_work(dir: &TempDir) -> (Task, TempDir) {
        let (_home, _devws, task, repo) = started(dir);
        run(repo.path(), &["config", "user.name", "t"]);
        run(repo.path(), &["config", "user.email", "t@t"]);
        task.transaction(|tx| {
            tx.state.approved = true;
            tx.state.steps.extend([
                accepted("H2f-execution", 1, None),
                accepted("H2f.1", 3, None),
                Step {
                    status: StepStatus::Ordered,
                    ..accepted("H2g.1", 1, None)
                },
                accepted("H2h.1", 1, None),
            ]);
            Ok(())
        })
        .unwrap();
        fs::write(task.dir().join("plan.md"), "# The plan\n").unwrap();
        fs::create_dir_all(task.dir().join("plans")).unwrap();
        fs::write(task.dir().join("plans/H2f-execution.md"), "# H2f\n").unwrap();
        (task, repo)
    }

    #[test]
    fn an_accepted_step_commits_the_history_and_nothing_else() {
        let dir = TempDir::new().unwrap();
        let (task, repo) = with_work(&dir);
        // An agent's work in progress in the same worktree.
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();

        assert!(save(&task, Some("H2f.1")).unwrap());
        let committed = git_output(repo.path(), &["show", "--name-only", "--format=%s", "HEAD"]);
        let mut lines = committed.lines();
        assert_eq!(lines.next(), Some("chore(team): record 1.1.2 title H2f.1"));
        let files: Vec<_> = lines.filter(|line| !line.is_empty()).collect();
        assert_eq!(
            files,
            [
                ".devagentics/history/feat-x/history.json",
                ".devagentics/history/feat-x/plan.md",
                ".devagentics/history/feat-x/plans/H2f-execution.md",
            ]
        );
        assert_eq!(
            git_output(repo.path(), &["status", "--porcelain"]),
            "M README.md",
            "the agent's change is left as it was"
        );
        let history: History = serde_json::from_str(
            &fs::read_to_string(folder(repo.path(), "feat-x").join("history.json")).unwrap(),
        )
        .unwrap();
        let versions: Vec<_> = history
            .steps
            .iter()
            .map(|done| (done.id.as_str(), done.version.as_str()))
            .collect();
        assert_eq!(
            versions,
            [
                ("H2f-execution", "1.1.0"),
                ("H2f.1", "1.1.0"),
                ("H2h.1", "1.3.0")
            ],
            "accepted steps only, H2g (1.2.0) still at work"
        );
        assert!(
            !save(&task, Some("H2f.1")).unwrap(),
            "nothing new, no commit"
        );
        fs::write(task.dir().join("plans/H2h.1.md"), "# H2h\n").unwrap();
        assert!(save(&task, None).unwrap(), "asked for: the new plan");
        assert_eq!(
            git_output(repo.path(), &["log", "-1", "--format=%s"]),
            "chore(team): record the team history"
        );
    }

    #[test]
    fn a_task_started_elsewhere_carries_on_from_the_history() {
        let dir = TempDir::new().unwrap();
        let (task, repo) = with_work(&dir);
        save(&task, Some("H2h.1")).unwrap();

        // Another server: a home of its own, the same branch.
        let other = TempDir::new().unwrap();
        let home = Home::at(other.path().join("home"));
        let devws = FakeDevws::new(&other.path().join("w")).with_kind("claude", Program::Claude);
        let started =
            crate::start::start_task(&home, &devws, repo.path(), "devagentics", None).unwrap();
        let loaded = Task::open(&home, &started.id).unwrap();
        let state = loaded.state().unwrap();
        assert!(state.approved, "the plan stays approved");
        let steps: Vec<_> = state
            .steps
            .iter()
            .map(|step| (step.id.as_str(), step.status, step.attempts))
            .collect();
        assert_eq!(
            steps,
            [
                ("H2f-execution", StepStatus::Accepted, 1),
                ("H2f.1", StepStatus::Accepted, 3),
                ("H2h.1", StepStatus::Accepted, 1),
            ]
        );
        let versions = specs::versions(&state.steps);
        assert_eq!(versions[0].1, "1.1.0");
        assert_eq!(versions[1].1, "1.3.0", "numbers kept, H2g's gap included");
        assert_eq!(specs::next_version(&state.steps), "1.4.0");
        assert_eq!(
            fs::read_to_string(loaded.dir().join("plan.md")).unwrap(),
            "# The plan\n"
        );
        assert!(loaded.dir().join("plans/H2f-execution.md").is_file());
        assert!(
            loaded
                .log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("history loaded")),
            "the log says so"
        );
        let args = fs::read_to_string(&started.args_file).unwrap();
        assert!(args.contains("continues from the team history"), "{args}");
    }

    #[test]
    fn only_history_commits_keep_a_reviewed_target_current() {
        let repo = crate::checks::tests::repo();
        commit_file(repo.path(), "src/app.rs", "fn app() {}\n", "app");
        let reviewed = crate::checks::head(repo.path()).unwrap();
        assert!(only_history_since(repo.path(), &reviewed).unwrap());
        commit_file(
            repo.path(),
            ".devagentics/history/t/history.json",
            "{}\n",
            "history",
        );
        assert!(only_history_since(repo.path(), &reviewed).unwrap());
        commit_file(repo.path(), "src/other.rs", "fn other() {}\n", "code");
        assert!(
            !only_history_since(repo.path(), &reviewed).unwrap(),
            "new code"
        );
    }

    #[test]
    fn the_history_is_no_agents_work() {
        let dir = TempDir::new().unwrap();
        let (task, repo) = with_work(&dir);
        let base = crate::checks::head(repo.path()).unwrap();
        save(&task, Some("H2f.1")).unwrap();
        commit_file(repo.path(), "src/login.rs", "fn login() {}\n", "login");
        let work = crate::checks::inspect(repo.path(), &base, &["src/**".into()], &[]).unwrap();
        assert!(work.follows_the_rules(), "{work:?}");
        assert_eq!(work.changed, ["src/login.rs"]);
        let senior = crate::checks::inspect(repo.path(), &base, &[], &["**".into()]).unwrap();
        assert_eq!(senior.forbidden, ["src/login.rs"], "only the agent's file");
    }
}
