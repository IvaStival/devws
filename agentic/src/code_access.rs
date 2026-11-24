//! The leader's eyes on the code: read a file, search, the history and the
//! diffs of the task's branch, a lane or an order, without a shell and
//! without writing. Only committed objects are read (never a worktree), and
//! never what a snapshot leaves out: secrets, data, protected paths.

use crate::runner::{Configuration, leader_unreadable, safe_git, safe_git_output};
use crate::task::Task;
use anyhow::{Context, Result, bail, ensure};
use std::path::PathBuf;

/// The most a tool returns, under the MCP server's own limit.
const TOTAL: usize = 60_000;
const FILE_BYTES: u64 = 2 * 1024 * 1024;
const LINE: usize = 1000;
const DIFF_PER_FILE: usize = 20_000;
const MATCHES: usize = 200;
const MATCH_LINE: usize = 300;

/// What `at` names: a commit to read at, the base of its changes when it
/// is an order, and the runner configuration that decides what is readable.
struct Source {
    worktree: PathBuf,
    commit: String,
    base: Option<String>,
    label: String,
    config: Option<Configuration>,
}

fn commit_of(worktree: &std::path::Path, name: &str) -> Result<String> {
    safe_git(
        worktree,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    )
    .with_context(|| format!("{name} is not a commit here"))
}

fn is_sha(text: &str) -> bool {
    (7..=40).contains(&text.len()) && text.chars().all(|character| character.is_ascii_hexdigit())
}

/// The task's branch (`task`, the default), a lane (`lane:back`), an order
/// (`o123`: its base..head) or a commit. Nothing else, so nothing reaches git
/// as an option.
fn resolve(task: &Task, at: Option<&str>, repo: Option<&str>) -> Result<Source> {
    let config = task.config()?;
    let state = task.state()?;
    let pick = |name: Option<&str>| {
        match name {
            Some(name) => config.repos.iter().find(|found| found.name == name),
            None => config.repos.first(),
        }
        .cloned()
        .with_context(|| format!("the task has no repo {}", name.unwrap_or("")))
    };
    let at = at.unwrap_or("task");
    let lane_name = at.strip_prefix("lane:").or_else(|| {
        config
            .lanes
            .iter()
            .any(|lane| lane.name == at)
            .then_some(at)
    });
    if at == "task" {
        let repo = pick(repo)?;
        return Ok(Source {
            commit: commit_of(&repo.worktree, "HEAD")?,
            base: None,
            label: "the task branch".into(),
            config: state.runner.as_ref().map(|runner| runner.for_lane(None)),
            worktree: repo.worktree,
        });
    }
    if let Some(name) = lane_name {
        let lane = crate::lanes::get(task, name)?;
        let repo = pick(Some(&lane.repo))?;
        return Ok(Source {
            commit: commit_of(&repo.worktree, &lane.branch)?,
            base: None,
            label: format!("lane {name}"),
            config: state
                .runner
                .as_ref()
                .map(|runner| runner.for_lane(Some(name))),
            worktree: repo.worktree,
        });
    }
    if let Some(order) = state.orders.iter().find(|order| order.id == at) {
        let agent = state.agent(&order.agent);
        let repo = pick(agent.map(|agent| agent.repo.as_str()).or(repo))?;
        let lane = agent.and_then(|agent| agent.lane.clone());
        let head = match state.check_evidence.get(at) {
            Some(evidence) => evidence.head.clone(),
            None if order.status == crate::state::OrderStatus::Open && agent.is_some() => {
                crate::checks::head(&crate::lanes::workplace(task, agent.unwrap())?.worktree)?
            }
            None => state
                .reviews
                .iter()
                .rev()
                .find(|review| review.order == at)
                .map(|review| review.source.clone())
                .with_context(|| {
                    format!("order {at} has no recorded head: no check run or review yet")
                })?,
        };
        return Ok(Source {
            commit: commit_of(&repo.worktree, &head)?,
            base: Some(commit_of(&repo.worktree, &order.base)?),
            label: format!("order {at}"),
            config: state
                .runner
                .as_ref()
                .map(|runner| runner.for_lane(lane.as_deref())),
            worktree: repo.worktree,
        });
    }
    if is_sha(at) {
        let repo = pick(repo)?;
        return Ok(Source {
            commit: commit_of(&repo.worktree, at)?,
            base: None,
            label: format!("commit {at}"),
            config: state.runner.as_ref().map(|runner| runner.for_lane(None)),
            worktree: repo.worktree,
        });
    }
    bail!(
        "at names the task branch (task), a lane (lane:back), an order (o123) or a commit, not {at}"
    )
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(10)]
}

fn readable(source: &Source, path: &str) -> Result<()> {
    if let Some(why) = leader_unreadable(path, source.config.as_ref())? {
        bail!("{path} is not readable: {why}");
    }
    Ok(())
}

fn cut(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

/// A file at `at` with numbered lines, or a folder's entries.
pub fn read_file(
    task: &Task,
    path: &str,
    at: Option<&str>,
    repo: Option<&str>,
    start: Option<usize>,
    lines: Option<usize>,
) -> Result<String> {
    let source = resolve(task, at, repo)?;
    let path = path.trim().trim_end_matches('/');
    let root = path.is_empty() || path == ".";
    if !root {
        readable(&source, path)?;
    }
    let entry = if root {
        "040000 tree - \t.".to_string()
    } else {
        safe_git(&source.worktree, &["ls-tree", &source.commit, "--", path])?
    };
    let Some((meta, _)) = entry.split_once('\t') else {
        bail!("there is no {path} at {}", source.label);
    };
    let fields: Vec<&str> = meta.split_whitespace().collect();
    match fields.as_slice() {
        [_, "tree", ..] => list(&source, if root { "" } else { path }),
        ["120000", ..] => bail!("{path} is a symbolic link"),
        [_, "commit", ..] => bail!("{path} is a submodule"),
        [_, "blob", object] => blob(&source, path, object, start, lines),
        _ => bail!("cannot read {path}"),
    }
}

fn list(source: &Source, folder: &str) -> Result<String> {
    let target = if folder.is_empty() {
        source.commit.clone()
    } else {
        format!("{}:{folder}", source.commit)
    };
    let entries = safe_git(&source.worktree, &["ls-tree", &target])?;
    let mut shown = Vec::new();
    let mut hidden = 0;
    for line in entries.lines() {
        let Some((meta, name)) = line.split_once('\t') else {
            continue;
        };
        let path = if folder.is_empty() {
            name.to_string()
        } else {
            format!("{folder}/{name}")
        };
        let folder_entry = meta.contains(" tree ");
        // A folder is listed unless a built-in rule hides all of it; its
        // files are judged one by one when read.
        let hide = if folder_entry {
            leader_unreadable(&path, None)?.is_some()
        } else {
            leader_unreadable(&path, source.config.as_ref())?.is_some()
        };
        if hide {
            hidden += 1;
        } else {
            shown.push(if folder_entry {
                format!("{name}/")
            } else {
                name.to_string()
            });
        }
    }
    let mut text = format!(
        "{} @ {} ({}):\n{}",
        if folder.is_empty() { "." } else { folder },
        source.label,
        short(&source.commit),
        shown.join("\n")
    );
    if hidden > 0 {
        text.push_str(&format!(
            "\n({hidden} entries not readable: secrets, data or outside the runner's source)"
        ));
    }
    Ok(cut(&text, TOTAL))
}

fn blob(
    source: &Source,
    path: &str,
    object: &str,
    start: Option<usize>,
    lines: Option<usize>,
) -> Result<String> {
    let size: u64 = safe_git(&source.worktree, &["cat-file", "-s", object])?
        .parse()
        .context("blob size")?;
    ensure!(
        size <= FILE_BYTES,
        "{path} is {size} bytes: too large to read"
    );
    let output = safe_git_output(&source.worktree, &["cat-file", "blob", object])?;
    ensure!(output.status.success(), "cannot read {path}");
    ensure!(!output.stdout.contains(&0), "{path} is a binary file");
    let text = String::from_utf8_lossy(&output.stdout);
    let all: Vec<&str> = text.lines().collect();
    let first = start.unwrap_or(1).max(1);
    let count = lines.unwrap_or(300).clamp(1, 1500);
    let mut body = String::new();
    let mut last = first.saturating_sub(1);
    for (index, line) in all.iter().enumerate().skip(first - 1).take(count) {
        let numbered = format!("{:>5}  {}\n", index + 1, cut(line, LINE));
        if body.len() + numbered.len() > TOTAL {
            break;
        }
        body.push_str(&numbered);
        last = index + 1;
    }
    let mut text = format!(
        "{path} @ {} ({}), lines {first}–{last} of {}:\n{body}",
        source.label,
        short(&source.commit),
        all.len()
    );
    if last < all.len() {
        text.push_str(&format!("… more: read_file start={}", last + 1));
    }
    Ok(text)
}

/// Lines matching `pattern` (fixed text, or a regular expression), with
/// their file and line number; unreadable files are left out.
pub fn search_code(
    task: &Task,
    pattern: &str,
    at: Option<&str>,
    repo: Option<&str>,
    glob: Option<&str>,
    regex: bool,
) -> Result<String> {
    ensure!(
        !pattern.is_empty() && pattern.chars().count() <= 200,
        "the pattern is 1 to 200 characters"
    );
    let source = resolve(task, at, repo)?;
    let mut args = vec![
        "grep".to_string(),
        "-n".into(),
        "-I".into(),
        "--no-color".into(),
        if regex { "-E" } else { "-F" }.into(),
        "--max-count=20".into(),
        "-e".into(),
        pattern.into(),
        source.commit.clone(),
        "--".into(),
    ];
    if let Some(glob) = glob {
        ensure!(
            !glob.starts_with('/') && !glob.split('/').any(|part| part == ".."),
            "the glob is a path from the repo's root"
        );
        args.push(format!(":(glob){glob}"));
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = safe_git_output(&source.worktree, &args)?;
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Ok(format!("Nothing matches at {}.", source.label)),
        _ => bail!(
            "search failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
    let prefix = format!("{}:", source.commit);
    let mut found = Vec::new();
    let mut more = 0;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.strip_prefix(&prefix).unwrap_or(line);
        let Some((path, _)) = line.split_once(':') else {
            continue;
        };
        if leader_unreadable(path, source.config.as_ref())?.is_some() {
            continue;
        }
        if found.len() < MATCHES {
            found.push(cut(line, MATCH_LINE));
        } else {
            more += 1;
        }
    }
    if found.is_empty() {
        return Ok(format!("Nothing readable matches at {}.", source.label));
    }
    let mut text = format!(
        "Matches at {} ({}):\n{}",
        source.label,
        short(&source.commit),
        found.join("\n")
    );
    if more > 0 {
        text.push_str(&format!("\n… {more} more: narrow the pattern or the glob"));
    }
    Ok(cut(&text, TOTAL))
}

/// The commits at `at` (an order: its own), newest first.
pub fn git_log(
    task: &Task,
    at: Option<&str>,
    repo: Option<&str>,
    path: Option<&str>,
    limit: Option<usize>,
) -> Result<String> {
    let source = resolve(task, at, repo)?;
    let range = match &source.base {
        Some(base) => format!("{base}..{}", source.commit),
        None => source.commit.clone(),
    };
    let limit = limit.unwrap_or(20).clamp(1, 100).to_string();
    let mut args = vec![
        "log",
        "--no-color",
        "--date=short",
        "--format=%h %ad %an %s",
        "-n",
        &limit,
        &range,
    ];
    if let Some(path) = path {
        readable(&source, path)?;
        args.extend(["--", path]);
    }
    let log = safe_git(&source.worktree, &args)?;
    Ok(cut(
        &format!(
            "History at {}:\n{}",
            source.label,
            if log.is_empty() { "(no commits)" } else { &log }
        ),
        TOTAL,
    ))
}

/// What changed: an order's work (`at=o123`), a lane against the task's
/// branch (`at=lane:back`), or between two names (`from`, `to`). Files the
/// leader may not read are counted, never shown.
#[allow(clippy::too_many_arguments)]
pub fn git_diff(
    task: &Task,
    at: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    repo: Option<&str>,
    path: Option<&str>,
    stat: bool,
) -> Result<String> {
    let (source, range, label) = match (from, to) {
        (Some(from), Some(to)) => {
            let to = resolve(task, Some(to), repo)?;
            let from = resolve(task, Some(from), repo)?;
            let range = format!("{}..{}", from.commit, to.commit);
            let label = format!("{} → {}", from.label, to.label);
            (to, range, label)
        }
        (None, None) => {
            let source = resolve(task, at, repo)?;
            let range = match &source.base {
                Some(base) => format!("{base}..{}", source.commit),
                None if source.label.starts_with("lane ") => {
                    let task_head = commit_of(&source.worktree, "HEAD")?;
                    format!("{task_head}...{}", source.commit)
                }
                None => bail!("name what to compare: at=<order> or at=lane:<name>, or from and to"),
            };
            let label = source.label.clone();
            (source, range, label)
        }
        _ => bail!("give both from and to, or neither"),
    };
    if let Some(path) = path {
        readable(&source, path)?;
    }
    let mut args = vec!["diff", "--numstat", "-z", "--no-renames", &range];
    if let Some(path) = path {
        args.extend(["--", path]);
    }
    let output = safe_git_output(&source.worktree, &args)?;
    ensure!(
        output.status.success(),
        "diff failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let mut files = Vec::new();
    let mut hidden = 0;
    for record in String::from_utf8_lossy(&output.stdout).split('\0') {
        let mut fields = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(file)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if leader_unreadable(file, source.config.as_ref())?.is_some() {
            hidden += 1;
        } else {
            files.push((added.to_string(), removed.to_string(), file.to_string()));
        }
    }
    let mut text = format!("Changes in {label}: {} files", files.len());
    if hidden > 0 {
        text.push_str(&format!(
            " ({hidden} more not readable: secrets, data or outside the runner's source)"
        ));
    }
    text.push('\n');
    if stat {
        for (added, removed, file) in &files {
            text.push_str(&format!("+{added} -{removed} {file}\n"));
        }
        return Ok(text);
    }
    let mut left = Vec::new();
    for (added, removed, file) in &files {
        if text.len() >= TOTAL {
            left.push(format!("+{added} -{removed} {file}"));
            continue;
        }
        let output = safe_git_output(
            &source.worktree,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                &range,
                "--",
                file,
            ],
        )?;
        let diff = String::from_utf8_lossy(&output.stdout);
        let budget = DIFF_PER_FILE.min(TOTAL.saturating_sub(text.len()));
        text.push_str(&cut(&diff, budget));
        text.push('\n');
    }
    if !left.is_empty() {
        text.push_str(&format!(
            "… {} more files; ask for one with git_diff path=…:\n{}",
            left.len(),
            left.join("\n")
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::tests::commit_file;
    use tempfile::TempDir;

    fn files(repo: &std::path::Path) {
        commit_file(
            repo,
            "src/main.rs",
            "fn main() {\n    serve();\n}\n",
            "main",
        );
        commit_file(repo, ".env", "SECRET=1\n", "env");
        commit_file(repo, "keys/server.pem", "KEY\n", "key");
        commit_file(
            repo,
            "docs/guide.md",
            "# Guide\nserve() starts it\n",
            "docs",
        );
    }

    #[test]
    fn the_leader_reads_a_file_and_a_folder_but_never_a_secret() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&dir);
        files(repo.path());

        let text = read_file(&task, "src/main.rs", None, None, None, None).unwrap();
        assert!(text.starts_with("src/main.rs @ the task branch"), "{text}");
        assert!(text.contains("    2      serve();"), "{text}");
        let window = read_file(&task, "src/main.rs", None, None, Some(2), Some(1)).unwrap();
        assert!(
            window.contains("lines 2–2 of 3") && window.contains("more: read_file start=3"),
            "{window}"
        );

        let root = read_file(&task, ".", None, None, None, None).unwrap();
        assert!(root.contains("src/") && root.contains("docs/"), "{root}");
        assert!(!root.contains(".env"), "{root}");

        for secret in [".env", "keys/server.pem"] {
            let error = read_file(&task, secret, None, None, None, None)
                .unwrap_err()
                .to_string();
            assert!(error.contains("not readable"), "{error}");
        }
        assert!(read_file(&task, "../x", None, None, None, None).is_err());
        assert!(read_file(&task, "src/main.rs", Some("--output=x"), None, None, None).is_err());
        assert!(read_file(&task, "src/none.rs", None, None, None, None).is_err());
    }

    #[test]
    fn search_and_diff_leave_secrets_out() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&dir);
        let base = crate::checks::head(repo.path()).unwrap();
        files(repo.path());
        commit_file(repo.path(), ".env", "SECRET=serve\n", "env again");
        let head = crate::checks::head(repo.path()).unwrap();

        let found = search_code(&task, "serve", None, None, None, false).unwrap();
        assert!(
            found.contains("src/main.rs:2:") && found.contains("docs/guide.md:2:"),
            "{found}"
        );
        assert!(!found.contains(".env"), "{found}");
        let none = search_code(&task, "nowhere-to-be-found", None, None, None, false).unwrap();
        assert!(none.starts_with("Nothing matches"), "{none}");
        let only_docs = search_code(&task, "serve", None, None, Some("docs/**"), false).unwrap();
        assert!(!only_docs.contains("src/main.rs"), "{only_docs}");

        let diff = git_diff(&task, None, Some(&base), Some(&head), None, None, false).unwrap();
        assert!(diff.contains("+    serve();"), "{diff}");
        assert!(
            diff.contains("2 more not readable") && !diff.contains("SECRET"),
            "{diff}"
        );
        let stat = git_diff(&task, None, Some(&base), Some(&head), None, None, true).unwrap();
        assert!(
            stat.contains("src/main.rs") && !stat.contains("server.pem"),
            "{stat}"
        );
        assert!(
            git_diff(&task, None, None, None, None, None, false).is_err(),
            "nothing to compare"
        );

        let log = git_log(&task, None, None, Some("src/main.rs"), None).unwrap();
        assert!(log.contains(" main"), "{log}");
        assert!(git_log(&task, None, None, Some(".env"), None).is_err());

        // Nothing was written: the worktree and HEAD are as they were.
        assert_eq!(crate::checks::head(repo.path()).unwrap(), head);
        let status = crate::checks::git(repo.path(), &["status", "--porcelain"]).unwrap();
        assert!(status.trim().is_empty(), "{status}");
    }

    #[test]
    fn an_order_and_a_lane_are_read_where_their_work_is() {
        let dir = TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&dir);
        files(repo.path());
        let back = crate::lanes::add(&task, "back", &["backend/**".into()], None).unwrap();
        let base = crate::checks::head(&back.worktree).unwrap();
        commit_file(&back.worktree, "backend/api.rs", "fn api() {}\n", "api");
        let head = crate::checks::head(&back.worktree).unwrap();
        task.transaction(|tx| {
            tx.state.orders.push(crate::state::Order {
                read: Vec::new(),
                targets: Vec::new(),
                id: "o7".into(),
                step: "s1".into(),
                agent: "gone".into(),
                text: String::new(),
                allowed: Vec::new(),
                forbidden: Vec::new(),
                check: None,
                base: base.clone(),
                status: crate::state::OrderStatus::Closed,
                delivered: true,
                stalled: false,
                plan: None,
            });
            tx.state.check_evidence.insert(
                "o7".into(),
                crate::check_profiles::Evidence {
                    order: "o7".into(),
                    revision: 0,
                    base,
                    head,
                    configuration: String::new(),
                    run: PathBuf::new(),
                    phases: Vec::new(),
                },
            );
            Ok(())
        })
        .unwrap();

        let lane = read_file(&task, "backend/api.rs", Some("lane:back"), None, None, None).unwrap();
        assert!(lane.contains("fn api()"), "{lane}");
        assert!(
            read_file(&task, "backend/api.rs", None, None, None, None).is_err(),
            "not on the task branch yet"
        );
        let order = git_diff(&task, Some("o7"), None, None, None, None, false).unwrap();
        assert!(
            order.contains("+fn api() {}") && order.contains("1 files"),
            "{order}"
        );
        let lane_diff = git_diff(&task, Some("lane:back"), None, None, None, None, true).unwrap();
        assert!(lane_diff.contains("backend/api.rs"), "{lane_diff}");
        let history = git_log(&task, Some("o7"), None, None, None).unwrap();
        assert!(
            history.contains(" api") && !history.contains(" main"),
            "{history}"
        );
    }
}
