//! The end of a task: every repo's branch pushed and a draft pull request for
//! it, each one linking the others. The user reviews them.

use crate::checks;
use crate::log::Kind;
use crate::state::StepStatus;
use crate::task::Task;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// `gh`, or `DEVAGENTICS_GH`.
pub fn gh_command() -> String {
    std::env::var("DEVAGENTICS_GH").unwrap_or_else(|_| "gh".to_string())
}

fn run(program: &str, worktree: &Path, arguments: &[&str]) -> Result<std::process::Output> {
    Command::new(program)
        .args(arguments)
        .current_dir(worktree)
        .output()
        .with_context(|| format!("running {program}"))
}

fn succeed(program: &str, worktree: &Path, arguments: &[&str]) -> Result<String> {
    let output = run(program, worktree, arguments)?;
    if !output.status.success() {
        bail!(
            "{program} {} failed in {}: {}",
            arguments.join(" "),
            worktree.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// A pull request for each repo of the task, as (repo, url). Only when every
/// step is accepted and every worktree is clean.
pub fn open_prs(task: &Task, gh: &str) -> Result<Vec<(String, String)>> {
    let config = task.config()?;
    let state = task.state()?;
    if state.steps.is_empty() {
        bail!("there are no steps: nothing was done to open pull requests for");
    }
    let open: Vec<_> = state
        .steps
        .iter()
        .filter(|step| step.status != StepStatus::Accepted)
        .map(|step| format!("{} ({:?})", step.id, step.status))
        .collect();
    if !open.is_empty() {
        bail!("not every step is accepted yet: {}", open.join(", "));
    }
    for repo in &config.repos {
        let work = checks::inspect(&repo.worktree, "HEAD", &[], &[])?;
        if !work.uncommitted.is_empty() {
            bail!(
                "{} has uncommitted changes: {}",
                repo.name,
                work.uncommitted.join(", ")
            );
        }
    }

    let mut requests = Vec::new();
    for repo in &config.repos {
        succeed(
            "git",
            &repo.worktree,
            &["push", "-u", "origin", &config.branch],
        )?;
        // One that exists already (a second run) is kept.
        let url = match succeed(
            gh,
            &repo.worktree,
            &["pr", "view", "--json", "url", "-q", ".url"],
        ) {
            Ok(url) if !url.is_empty() => url,
            _ => succeed(gh, &repo.worktree, &["pr", "create", "--draft", "--fill"])?
                .lines()
                .last()
                .unwrap_or_default()
                .to_string(),
        };
        requests.push((repo.clone(), url));
    }

    if requests.len() > 1 {
        for (repo, url) in &requests {
            let body = succeed(
                gh,
                &repo.worktree,
                &["pr", "view", url, "--json", "body", "-q", ".body"],
            )?;
            let links: Vec<_> = requests
                .iter()
                .filter(|(_, other)| other != url)
                .map(|(other, other_url)| format!("- {}: {other_url}", other.name))
                .collect();
            let linked = format!(
                "{body}\n\n---\nPart of the task {} with:\n{}",
                config.id,
                links.join("\n")
            );
            succeed(gh, &repo.worktree, &["pr", "edit", url, "--body", &linked])?;
        }
    }

    let opened: Vec<_> = requests
        .into_iter()
        .map(|(repo, url)| (repo.name, url))
        .collect();
    task.transaction(|tx| {
        for (repo, url) in &opened {
            tx.log(Kind::Pr, None, None, &format!("{repo}: {url}"));
        }
        Ok(())
    })?;
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::tests::{commit_file, repo, run as git};
    use crate::home::Home;
    use crate::state::Step;
    use crate::task::{Repo, TaskConfig};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    /// A repo on feat/x with its own bare "origin" and a commit to push.
    fn pushable_repo() -> (TempDir, TempDir) {
        let repo = repo();
        let origin = TempDir::new().unwrap();
        git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(
            repo.path(),
            &[
                "remote",
                "add",
                "origin",
                &origin.path().display().to_string(),
            ],
        );
        git(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        commit_file(repo.path(), "src/a.rs", "x\n", "a");
        (repo, origin)
    }

    /// A gh that records its calls, has no pull request until it creates one,
    /// and answers with a URL named after the folder it runs in.
    fn stub_gh(dir: &Path) -> String {
        let path = dir.join("gh");
        fs::write(
            &path,
            format!(
                r#"#!/bin/sh
name=$(basename "$PWD")
echo "$name: $*" | tr '\n' ' ' >> "{log}"; echo >> "{log}"
case "$1 $2" in
  "pr view")
    case "$*" in
      *"-q .url"*) [ -f "{dir}/$name.pr" ] && echo "https://github.com/acme/$name/pull/1" || exit 1 ;;
      *"-q .body"*) echo "body of $name" ;;
    esac ;;
  "pr create") touch "{dir}/$name.pr"; echo "Creating draft"; echo "https://github.com/acme/$name/pull/1" ;;
  "pr edit") ;;
esac
"#,
                log = dir.join("gh.log").display(),
                dir = dir.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    fn task_with(repos: &[(&str, &Path)], accepted: bool) -> (TempDir, Task) {
        let dir = TempDir::new().unwrap();
        let config = TaskConfig {
            id: "t".into(),
            branch: "feat/x".into(),
            workspace: "dev-a".into(),
            max_agents: 3,
            lanes: Vec::new(),
            repos: repos
                .iter()
                .map(|(name, path)| Repo {
                    name: name.to_string(),
                    worktree: path.to_path_buf(),
                    check: None,
                })
                .collect(),
        };
        let task = Task::create(&Home::at(dir.path().to_path_buf()), &config).unwrap();
        task.transaction(|tx| {
            tx.state.steps.push(Step {
                id: "s1".into(),
                title: "T".into(),
                status: if accepted {
                    StepStatus::Accepted
                } else {
                    StepStatus::Reported
                },
                attempts: 1,
            });
            Ok(())
        })
        .unwrap();
        (dir, task)
    }

    #[test]
    fn each_repos_branch_is_pushed_and_gets_a_draft_pr_linking_the_others() {
        let (api, api_origin) = pushable_repo();
        let (web, web_origin) = pushable_repo();
        let bin = TempDir::new().unwrap();
        let gh = stub_gh(bin.path());
        let (_dir, task) = task_with(&[("api", api.path()), ("web", web.path())], true);

        let opened = open_prs(&task, &gh).unwrap();
        assert_eq!(opened.len(), 2);
        assert_eq!(opened[0].0, "api");
        for origin in [&api_origin, &web_origin] {
            let branches = Command::new("git")
                .arg("-C")
                .arg(origin.path())
                .args(["branch", "--list", "feat/x"])
                .output()
                .unwrap();
            assert!(
                String::from_utf8_lossy(&branches.stdout).contains("feat/x"),
                "the branch was not pushed"
            );
        }

        let log = fs::read_to_string(bin.path().join("gh.log")).unwrap();
        assert_eq!(log.matches("pr create --draft --fill").count(), 2, "{log}");
        assert_eq!(log.matches("pr edit").count(), 2, "{log}");
        let edits: Vec<_> = log
            .lines()
            .filter(|line| line.contains("pr edit"))
            .collect();
        let api_name = api
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let web_name = web
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(
            edits
                .iter()
                .any(|line| line.starts_with(&format!("{api_name}:"))
                    && line.contains("web: https://github.com/acme/")
                    && line.contains(&format!("body of {api_name}"))),
            "{edits:?}"
        );
        assert!(
            edits
                .iter()
                .any(|line| line.starts_with(&format!("{web_name}:"))
                    && line.contains("api: https://github.com/acme/")),
            "{edits:?}"
        );
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("\"kind\":\"pr\""))
        );

        // Run again: the pull requests that exist are kept, not created twice.
        open_prs(&task, &gh).unwrap();
        let again = fs::read_to_string(bin.path().join("gh.log")).unwrap();
        assert_eq!(again.matches("pr create").count(), 2, "{again}");
    }

    #[test]
    fn a_single_repo_gets_its_pr_and_no_links() {
        let (repo, _origin) = pushable_repo();
        let bin = TempDir::new().unwrap();
        let gh = stub_gh(bin.path());
        let (_dir, task) = task_with(&[("app", repo.path())], true);
        let opened = open_prs(&task, &gh).unwrap();
        assert_eq!(opened.len(), 1);
        assert!(
            opened[0].1.starts_with("https://github.com/acme/"),
            "{}",
            opened[0].1
        );
        assert!(
            !fs::read_to_string(bin.path().join("gh.log"))
                .unwrap()
                .contains("pr edit")
        );
    }

    #[test]
    fn nothing_is_pushed_until_every_step_is_accepted_and_every_tree_is_clean() {
        let (repo, origin) = pushable_repo();
        let bin = TempDir::new().unwrap();
        let gh = stub_gh(bin.path());

        let (_dir, unfinished) = task_with(&[("app", repo.path())], false);
        let error = open_prs(&unfinished, &gh).unwrap_err().to_string();
        assert!(
            error.contains("not every step is accepted") && error.contains("s1"),
            "{error}"
        );

        let (_dir, empty) = task_with(&[("app", repo.path())], true);
        empty
            .transaction(|tx| {
                tx.state.steps.clear();
                Ok(())
            })
            .unwrap();
        assert!(
            open_prs(&empty, &gh)
                .unwrap_err()
                .to_string()
                .contains("no steps")
        );

        let (_dir, task) = task_with(&[("app", repo.path())], true);
        fs::write(repo.path().join("loose.txt"), "x").unwrap();
        assert!(
            open_prs(&task, &gh)
                .unwrap_err()
                .to_string()
                .contains("uncommitted changes: loose.txt")
        );

        let branches = Command::new("git")
            .arg("-C")
            .arg(origin.path())
            .args(["branch", "--list"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branches.stdout).trim().is_empty(),
            "something was pushed"
        );
        assert!(!bin.path().join("gh.log").exists(), "gh was called");
    }
}
