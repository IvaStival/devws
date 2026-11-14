//! The team's history, kept in the repo so the work can go on elsewhere: what
//! was accepted, the approved plan and the seniors' execution plans, in
//! `.devagentics/history/<task>/` on the task's branch. Every accepted step
//! commits it (`save`); a task started on that branch in another home takes
//! it back (`load`), so DONE, the release versions and the plan carry on.

use crate::log::Kind;
use crate::specs;
use crate::state::{Step, StepStatus};
use crate::task::Task;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where histories live in a repo; the work check leaves this folder out.
pub const FOLDER: &str = ".devagentics/history";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct History {
    task: String,
    branch: String,
    approved: bool,
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
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(arguments)
        .output()
        .context("running git")?;
    if !output.status.success() {
        bail!(
            "git {}: {}",
            arguments.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
            let number = specs::step_versions(&state.steps)
                .get(&found.id)
                .cloned()
                .unwrap_or_else(|| found.id.clone());
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
