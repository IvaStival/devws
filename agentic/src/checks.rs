//! What devAgentics checks itself when an agent says it is done, instead of
//! taking its word: the tree is clean, only the allowed files changed, and the
//! repo's check command passes.

use anyhow::{Context, Result, bail};
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) fn git(worktree: &Path, arguments: &[&str]) -> Result<String> {
    // A status devAgentics runs while it watches takes no index lock: one
    // left by a git stopped half way would fail the next merge.
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(worktree)
        .args(arguments)
        .output()
        .context("running git")?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The commit the worktree is on.
pub fn head(worktree: &Path) -> Result<String> {
    Ok(git(worktree, &["rev-parse", "HEAD"])?.trim().to_string())
}

/// The folders an agent that commits in the worktree writes, outside it: for
/// a linked worktree (a lane) its own `.git/worktrees/<name>`, and the
/// clone's `objects`, `refs` and `logs`. Not `.git` itself: Codex keeps a
/// `.git` folder read-only even when it is added as writable, but not the
/// folders inside it. A main checkout's `.git` is inside the worktree.
pub fn git_data(worktree: &Path) -> Result<Vec<std::path::PathBuf>> {
    let absolute = |which: &str| -> Result<std::path::PathBuf> {
        Ok(
            git(worktree, &["rev-parse", "--path-format=absolute", which])?
                .trim()
                .into(),
        )
    };
    let common = absolute("--git-common-dir")?;
    let own = absolute("--git-dir")?;
    let mut folders: Vec<_> = ["objects", "refs", "logs"]
        .iter()
        .map(|folder| common.join(folder))
        .collect();
    if own != common {
        folders.insert(0, own);
    }
    Ok(folders)
}

/// Globs relative to the repo's root. `*` stays inside a folder, `**` crosses
/// them, and a trailing `/` means everything under it.
pub(crate) fn glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let pattern = if pattern.ends_with('/') {
            format!("{pattern}**")
        } else {
            pattern.clone()
        };
        let glob: Glob = GlobBuilder::new(&pattern)
            .literal_separator(true)
            .build()
            .with_context(|| format!("'{pattern}' is not a glob"))?;
        builder.add(glob);
    }
    builder.build().context("building the globs")
}

/// What an agent's work looks like.
#[derive(Debug, Default, PartialEq)]
pub struct Work {
    /// Uncommitted or untracked paths.
    pub uncommitted: Vec<String>,
    /// Files changed since the base commit.
    pub changed: Vec<String>,
    pub added: u32,
    pub removed: u32,
    /// Changed files that no `allowed` glob covers.
    pub outside: Vec<String>,
    /// Changed files a `forbidden` glob covers.
    pub forbidden: Vec<String>,
}

impl Work {
    pub fn follows_the_rules(&self) -> bool {
        self.outside.is_empty() && self.forbidden.is_empty()
    }
}

pub fn inspect(
    worktree: &Path,
    base: &str,
    allowed: &[String],
    forbidden: &[String],
) -> Result<Work> {
    let allowed = glob_set(allowed)?;
    let has_allowed = !allowed.is_empty();
    let forbidden_set = glob_set(forbidden)?;
    let range = format!("{base}..HEAD");

    let uncommitted = git(
        worktree,
        &["status", "--porcelain", "--untracked-files=all"],
    )?
    .lines()
    .map(|line| line.get(3..).unwrap_or(line).to_string())
    .collect();
    let changed: Vec<String> = git(worktree, &["diff", "--name-only", &range])?
        .lines()
        .map(str::to_string)
        .collect();

    let (mut added, mut removed) = (0, 0);
    for line in git(worktree, &["diff", "--numstat", &range])?.lines() {
        let mut fields = line.split('\t');
        added += fields
            .next()
            .and_then(|count| count.parse::<u32>().ok())
            .unwrap_or(0);
        removed += fields
            .next()
            .and_then(|count| count.parse::<u32>().ok())
            .unwrap_or(0);
    }

    let outside = changed
        .iter()
        .filter(|path| has_allowed && !allowed.is_match(path))
        .cloned()
        .collect();
    let forbidden = changed
        .iter()
        .filter(|path| forbidden_set.is_match(path))
        .cloned()
        .collect();
    Ok(Work {
        uncommitted,
        changed,
        added,
        removed,
        outside,
        forbidden,
    })
}

/// How the repo's check ended.
#[derive(Debug, PartialEq)]
pub struct CheckRun {
    pub command: String,
    pub passed: bool,
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    pub log: PathBuf,
    pub seconds: u64,
}

/// Runs the check in the worktree with `sh -c`, its output in `log`. A check
/// still running after `timeout` is killed, and has not passed.
pub fn run_check(
    command: &str,
    worktree: &Path,
    log: &Path,
    timeout: Duration,
) -> Result<CheckRun> {
    let output = File::create(log).with_context(|| format!("creating {}", log.display()))?;
    let mut child = Command::new("sh")
        .process_group(0)
        .arg("-c")
        .arg(command)
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output)
        .spawn()
        .with_context(|| format!("running '{command}'"))?;

    let started = Instant::now();
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (Some(status), false);
        }
        if started.elapsed() >= timeout {
            // The shell may have descendants; kill the complete isolated group.
            let killed = unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
            if killed != 0 {
                child.kill().context("killing timed-out check")?;
            }
            child.wait().context("reaping timed-out check")?;
            break (None, true);
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Ok(CheckRun {
        command: command.to_string(),
        passed: status.is_some_and(|status| status.success()),
        timed_out,
        exit_code: status.and_then(|status| status.code()),
        log: log.to_path_buf(),
        seconds: started.elapsed().as_secs(),
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// A git repo with one commit; the returned directory removes it.
    pub fn repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        run(dir.path(), &["init", "-q", "-b", "main"]);
        commit_file(dir.path(), "README.md", "hello\n", "first");
        dir
    }

    pub fn run(dir: &Path, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(arguments)
            .status()
            .unwrap();
        assert!(status.success(), "git {arguments:?} failed");
    }

    pub fn commit_file(dir: &Path, path: &str, text: &str, message: &str) {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        run(dir, &["add", "-A"]);
        run(dir, &["commit", "-q", "-m", message]);
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn globs_are_relative_to_the_root_and_a_slash_means_everything_under() {
        let set = glob_set(&strings(&["src/**", "docs/", "*.md"])).unwrap();
        assert!(set.is_match("src/a/b.rs"));
        assert!(set.is_match("docs/guide/x.md"));
        assert!(set.is_match("README.md"));
        assert!(!set.is_match("tests/a.rs"));
        assert!(!set.is_match("sub/NOTES.md"), "* stays inside a folder");
        assert!(glob_set(&strings(&["["])).is_err());
    }

    #[test]
    fn work_is_what_changed_since_the_base_and_whether_it_is_committed() {
        let repo = repo();
        let base = head(repo.path()).unwrap();
        commit_file(repo.path(), "src/a.rs", "one\ntwo\n", "add");
        commit_file(repo.path(), "tests/a.rs", "t\n", "test");
        fs::write(repo.path().join("loose.txt"), "x").unwrap();

        let work = inspect(
            repo.path(),
            &base,
            &strings(&["src/**"]),
            &strings(&["tests/"]),
        )
        .unwrap();
        assert_eq!(work.changed, ["src/a.rs", "tests/a.rs"]);
        assert_eq!((work.added, work.removed), (3, 0));
        assert_eq!(work.uncommitted, ["loose.txt"]);
        assert_eq!(work.outside, ["tests/a.rs"]);
        assert_eq!(work.forbidden, ["tests/a.rs"]);
        assert!(!work.follows_the_rules());

        let clean = inspect(repo.path(), &base, &[], &[]).unwrap();
        assert!(clean.follows_the_rules(), "no allowed globs means any file");
    }

    #[test]
    fn a_check_passes_fails_or_runs_out_of_time_and_keeps_its_output() {
        let repo = repo();
        let logs = TempDir::new().unwrap();
        let log = logs.path().join("check.log");

        let passed = run_check("echo fine", repo.path(), &log, Duration::from_secs(5)).unwrap();
        assert!(passed.passed && !passed.timed_out && passed.exit_code == Some(0));
        assert_eq!(fs::read_to_string(&log).unwrap(), "fine\n");

        let failed = run_check(
            "echo broken >&2; exit 3",
            repo.path(),
            &log,
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(!failed.passed && failed.exit_code == Some(3));
        assert_eq!(fs::read_to_string(&log).unwrap(), "broken\n");

        let slow = run_check("sleep 5", repo.path(), &log, Duration::from_millis(200)).unwrap();
        assert!(!slow.passed && slow.timed_out && slow.exit_code.is_none());

        let in_the_repo = run_check(
            "test -f README.md",
            repo.path(),
            &log,
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(in_the_repo.passed, "a check runs in the worktree");
    }
}
