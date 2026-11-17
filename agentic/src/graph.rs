//! A code graph of a repo's tracked source at one commit, built by Graphify
//! (code only: no LLM backend, no query log, no network), for agents to look
//! up symbols, callers and paths instead of grepping. devAgentics owns the
//! graph: it is built from an exported copy into the task folder, never into
//! the project, and keyed by the source tree, so an agent never queries a
//! graph of other code than the commit it asked about.
use crate::task::Task;
use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a build or a query may take.
const BUILD_TIMEOUT: Duration = Duration::from_secs(300);
const QUERY_TIMEOUT: Duration = Duration::from_secs(60);
/// An answer longer than this is cut, saying so.
const ANSWER_LIMIT: usize = 16 * 1024;

/// The Graphify program devAgentics runs.
pub struct Graphify {
    pub program: PathBuf,
}

impl Graphify {
    /// `DEVAGENTICS_GRAPHIFY`, else the pinned install devws keeps at
    /// `~/.local/share/devagentics/graphify`.
    pub fn from_env() -> Result<Self> {
        if let Ok(program) = std::env::var("DEVAGENTICS_GRAPHIFY") {
            return Ok(Self {
                program: program.into(),
            });
        }
        let home = std::env::var("HOME").context("HOME is not set")?;
        Ok(Self {
            program: Path::new(&home).join(".local/share/devagentics/graphify/bin/graphify"),
        })
    }

    /// Fails unless the program runs and says its version.
    pub fn available(&self) -> Result<String> {
        let output = Command::new(&self.program)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .with_context(|| {
                format!(
                    "Graphify is not installed at {} (set DEVAGENTICS_GRAPHIFY to it)",
                    self.program.display()
                )
            })?;
        ensure!(
            output.status.success(),
            "{} --version failed",
            self.program.display()
        );
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// A built graph: its export holds `graphify-out/graph.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    pub repo: String,
    pub tree: String,
    pub source: PathBuf,
}

/// What an agent asks the graph.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// A symbol, its file and what it connects to; an ambiguous name lists
    /// the candidates.
    Explain { symbol: String },
    /// How one symbol reaches another.
    Path { from: String, to: String },
    /// A free question, answered by a bounded search.
    Query { question: String },
}

impl Request {
    /// A request from a tool's input: `action` explain (`symbol`), path
    /// (`from`, `to`) or query (`question`).
    pub fn from_input(input: &serde_json::Value) -> Result<Self> {
        let field = |name: &str| -> Result<String> {
            let value = input[name]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .with_context(|| format!("{name} is required"))?;
            ensure!(
                value.len() <= 200 && !value.starts_with('-'),
                "{name} must be a short name or question, not an option"
            );
            Ok(value.to_string())
        };
        match input["action"].as_str() {
            Some("explain") => Ok(Request::Explain {
                symbol: field("symbol")?,
            }),
            Some("path") => Ok(Request::Path {
                from: field("from")?,
                to: field("to")?,
            }),
            Some("query") => Ok(Request::Query {
                question: field("question")?,
            }),
            _ => bail!("action is explain (symbol), path (from, to) or query (question)"),
        }
    }

    fn arguments(&self) -> Vec<&str> {
        match self {
            Request::Explain { symbol } => vec!["explain", symbol],
            Request::Path { from, to } => vec!["path", from, to],
            Request::Query { question } => vec!["query", question],
        }
    }
}

/// The graph of `repo` (its worktree `worktree`) at `commit`: the one built
/// for its source tree, else a new one.
pub fn graph(
    graphify: &Graphify,
    task: &Task,
    repo: &str,
    worktree: &Path,
    commit: &str,
) -> Result<Graph> {
    let tree = crate::runner::safe_git(worktree, &["rev-parse", &format!("{commit}^{{tree}}")])?;
    ensure!(
        tree.chars().all(|character| character.is_ascii_hexdigit()),
        "invalid source tree"
    );
    let folder = task.dir().join("graphs").join(repo).join(&tree);
    let built = Graph {
        repo: repo.into(),
        tree: tree.clone(),
        source: folder.join("source"),
    };
    if built.source.join("graphify-out/graph.json").is_file() {
        return Ok(built);
    }
    let started = crate::log::now();
    let staging = folder.with_extension(format!(
        "building-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    let result = build(graphify, task, worktree, commit, &staging);
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    // Another build of the same tree may have finished first: either is it.
    if std::fs::rename(&staging, &folder).is_err() {
        std::fs::remove_dir_all(&staging)?;
        ensure!(
            built.source.join("graphify-out/graph.json").is_file(),
            "the graph of {tree} could not be stored"
        );
    }
    task.transaction(|tx| {
        tx.state
            .timing
            .record("graph_builds", started, crate::log::now().max(started))
    })?;
    Ok(built)
}

/// Exports the tracked source of `commit` (only what the runner exposes,
/// when the task has one) into `staging/source` and builds its graph there.
fn build(
    graphify: &Graphify,
    task: &Task,
    worktree: &Path,
    commit: &str,
    staging: &Path,
) -> Result<()> {
    let source = staging.join("source");
    std::fs::create_dir_all(&source)?;
    std::fs::create_dir_all(staging.join("home"))?;
    export(worktree, commit, &source)?;
    if let Some(runner) = task.state()?.runner {
        keep_runner_source(&source, &source, &runner)?;
    }
    let output = run(graphify, &source, staging, &["update", "."], BUILD_TIMEOUT)?;
    ensure!(
        output.success && source.join("graphify-out/graph.json").is_file(),
        "graphify could not build the graph: {}",
        tail(&output.text)
    );
    Ok(())
}

fn export(worktree: &Path, commit: &str, destination: &Path) -> Result<()> {
    let mut archive = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["archive", "--format=tar", commit])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("running git archive")?;
    let tar = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(destination)
        .stdin(archive.stdout.take().context("git archive output")?)
        .status()
        .context("running tar")?;
    ensure!(
        archive.wait()?.success() && tar.success(),
        "exporting {commit} failed"
    );
    Ok(())
}

/// Removes what the runner keeps from agents: a graph shows no more of the
/// project than its agents may read.
fn keep_runner_source(
    root: &Path,
    folder: &Path,
    runner: &crate::runner::Configuration,
) -> Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        if std::fs::symlink_metadata(&path)?.is_dir() {
            keep_runner_source(root, &path, runner)?;
            if std::fs::read_dir(&path)?.next().is_none() {
                std::fs::remove_dir(&path)?;
            }
            continue;
        }
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .context("non-UTF8 path")?
            .to_string();
        if !crate::runner::source_path(&relative, runner)? {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Answers an agent's graph `input` about `repo` at `commit`, when the task
/// has the code graph on. Each answer counts as a `graph_queries` interval.
pub fn answer(
    graphify: &Graphify,
    task: &Task,
    repo: &crate::task::Repo,
    commit: &str,
    input: &serde_json::Value,
) -> Result<String> {
    ensure!(
        task.state()?.graph_enabled,
        "the code graph is off for this task (the user turns it on: devagentics task graph {} on)",
        task.config()?.id
    );
    let request = Request::from_input(input)?;
    let started = crate::log::now();
    let graph = graph(graphify, task, &repo.name, &repo.worktree, commit)?;
    let text = ask(graphify, &graph, &request)?;
    task.transaction(|tx| {
        tx.state
            .timing
            .record("graph_queries", started, crate::log::now().max(started))
    })?;
    Ok(format!(
        "Code graph of {} at {} (tree {}):\n{text}",
        repo.name,
        &commit[..commit.len().min(12)],
        &graph.tree[..graph.tree.len().min(12)]
    ))
}

/// An answer from the graph. Ambiguous names and missing symbols are
/// answers too (Graphify says what it found); a failure to run is an error.
pub fn ask(graphify: &Graphify, graph: &Graph, request: &Request) -> Result<String> {
    let staging = graph.source.parent().context("graph folder")?;
    let output = run(
        graphify,
        &graph.source,
        staging,
        &request.arguments(),
        QUERY_TIMEOUT,
    )?;
    // 3: an ambiguous name, with its candidates; 4: nothing matched.
    ensure!(
        output.success || matches!(output.code, Some(3 | 4)),
        "graphify failed: {}",
        tail(&output.text)
    );
    Ok(bounded(&output.text))
}

struct Output {
    success: bool,
    code: Option<i32>,
    text: String,
}

/// Runs Graphify in `directory`, code only, with its own `HOME` under
/// `staging`, and without network where the system can take it away.
fn run(
    graphify: &Graphify,
    directory: &Path,
    staging: &Path,
    arguments: &[&str],
    timeout: Duration,
) -> Result<Output> {
    let mut command = if offline_available() {
        let mut command = Command::new("unshare");
        command.args(["-rn"]).arg(&graphify.program);
        command
    } else {
        Command::new(&graphify.program)
    };
    let mut child = command
        .args(arguments)
        .current_dir(directory)
        .env("HOME", staging.join("home"))
        .env("GRAPHIFY_BACKEND", "none")
        .env("GRAPHIFY_QUERY_LOG_DISABLE", "1")
        .env("GRAPHIFY_NO_TIPS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {}", graphify.program.display()))?;
    let stdout = child.stdout.take().context("graphify output")?;
    let stderr = child.stderr.take().context("graphify errors")?;
    let stdout = std::thread::spawn(move || read_all(stdout));
    let stderr = std::thread::spawn(move || read_all(stderr));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("graphify took longer than {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut text = stdout.join().unwrap_or_default();
    let errors = stderr.join().unwrap_or_default();
    if !errors.trim().is_empty() {
        text.push_str(&errors);
    }
    Ok(Output {
        success: status.success(),
        code: status.code(),
        text,
    })
}

fn read_all(mut stream: impl std::io::Read) -> String {
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Whether Graphify can run in a network namespace of its own (Linux with
/// unprivileged user namespaces): its code-only paths need no network, and
/// this makes sure of it.
fn offline_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("unshare")
            .args(["-rn", "true"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

fn bounded(text: &str) -> String {
    if text.len() <= ANSWER_LIMIT {
        return text.to_string();
    }
    let mut end = ANSWER_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[cut at {} KiB: ask about a narrower symbol or path]",
        &text[..end],
        ANSWER_LIMIT / 1024
    )
}

fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(5)..].join(" | ")
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::checks::tests::{commit_file, write_script};

    /// A stand-in for Graphify that records its calls: `update` writes a
    /// graph naming the files it saw; `explain` answers; `path` matches
    /// nothing (exit 4); `query fail` fails.
    pub fn stub(dir: &Path) -> Graphify {
        let program = dir.join("graphify");
        let log = dir.join("graphify.log");
        write_script(
            &program,
            &format!(
                r#"#!/bin/sh
echo "$PWD $* backend=$GRAPHIFY_BACKEND log=$GRAPHIFY_QUERY_LOG_DISABLE home=$HOME" >> "{log}"
case "$1" in
  update) mkdir -p graphify-out && find . -type f -not -path './graphify-out/*' | sort > graphify-out/graph.json ;;
  explain) echo "Node: $2" ;;
  path) echo "No match for $2"; exit 4 ;;
  query) [ "$2" = fail ] && {{ echo boom >&2; exit 1; }}; echo "answer to $2" ;;
esac
"#,
                log = log.display()
            ),
        );
        Graphify { program }
    }

    fn calls(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("graphify.log")).unwrap_or_default()
    }

    #[test]
    fn a_graph_is_built_once_per_source_tree_outside_the_project() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&directory);
        let graphify = stub(directory.path());
        commit_file(repo.path(), "src/app.ts", "export const app = 1\n", "app");
        let head = crate::checks::head(repo.path()).unwrap();

        let graph = graph(&graphify, &task, "app", repo.path(), &head).unwrap();
        assert!(graph.source.starts_with(task.dir()), "in the task folder");
        assert!(
            !repo.path().join("graphify-out").exists(),
            "never in the project"
        );
        let listed = std::fs::read_to_string(graph.source.join("graphify-out/graph.json")).unwrap();
        assert!(listed.contains("./src/app.ts"), "{listed}");
        let log = calls(directory.path());
        assert!(
            log.contains("update . backend=none log=1"),
            "code only, no query log: {log}"
        );
        assert!(!log.contains(&format!(
            "home={}",
            std::env::var("HOME").unwrap_or_default()
        )));

        let again = super::graph(&graphify, &task, "app", repo.path(), &head).unwrap();
        assert_eq!(again, graph);
        assert_eq!(
            calls(directory.path()).matches(" update ").count(),
            1,
            "reused"
        );

        commit_file(repo.path(), "src/more.ts", "x\n", "more");
        let newer = crate::checks::head(repo.path()).unwrap();
        let changed = super::graph(&graphify, &task, "app", repo.path(), &newer).unwrap();
        assert_ne!(changed.tree, graph.tree, "another tree, another graph");
        assert!(
            task.state()
                .unwrap()
                .timing
                .summary()
                .aggregate_seconds
                .contains_key("graph_builds")
        );
    }

    #[test]
    fn a_graph_shows_only_what_the_runner_lets_agents_read() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&directory);
        let graphify = stub(directory.path());
        commit_file(repo.path(), "src/app.ts", "app\n", "app");
        commit_file(repo.path(), "backend/secret.php", "x\n", "backend");
        commit_file(repo.path(), "src/.env", "KEY=1\n", "env");
        task.transaction(|tx| {
            tx.state.runner = Some(crate::runner::tests::configuration());
            Ok(())
        })
        .unwrap();
        let head = crate::checks::head(repo.path()).unwrap();
        let graph = graph(&graphify, &task, "app", repo.path(), &head).unwrap();
        let listed = std::fs::read_to_string(graph.source.join("graphify-out/graph.json")).unwrap();
        assert!(listed.contains("./src/app.ts"), "{listed}");
        assert!(
            !listed.contains("backend"),
            "outside the runner's source: {listed}"
        );
        assert!(!listed.contains(".env"), "protected: {listed}");
    }

    #[test]
    fn answers_include_ambiguity_and_misses_but_not_failures() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&directory);
        let graphify = stub(directory.path());
        let head = crate::checks::head(repo.path()).unwrap();
        let graph = graph(&graphify, &task, "app", repo.path(), &head).unwrap();
        let explained = ask(
            &graphify,
            &graph,
            &Request::Explain {
                symbol: "QueueDetails".into(),
            },
        )
        .unwrap();
        assert!(explained.contains("Node: QueueDetails"), "{explained}");
        let missed = ask(
            &graphify,
            &graph,
            &Request::Path {
                from: "A".into(),
                to: "B".into(),
            },
        )
        .unwrap();
        assert!(
            missed.contains("No match for A"),
            "a miss is an answer: {missed}"
        );
        let error = ask(
            &graphify,
            &graph,
            &Request::Query {
                question: "fail".into(),
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("boom"), "{error}");
    }

    #[test]
    fn a_missing_graphify_is_an_error_not_a_stale_graph() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&directory);
        let missing = Graphify {
            program: directory.path().join("nope"),
        };
        let head = crate::checks::head(repo.path()).unwrap();
        assert!(graph(&missing, &task, "app", repo.path(), &head).is_err());
        assert!(
            !task.dir().join("graphs").join("app").exists()
                || std::fs::read_dir(task.dir().join("graphs").join("app"))
                    .unwrap()
                    .next()
                    .is_none(),
            "nothing half-built is kept"
        );
    }

    #[test]
    #[ignore = "requires the pinned Graphify install"]
    fn the_pinned_graphify_builds_offline_and_explains_callers() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&directory);
        commit_file(
            repo.path(),
            "src/format.ts",
            "export function formatTime(at: number): string {\n  return String(at)\n}\n",
            "format",
        );
        commit_file(
            repo.path(),
            "src/view.ts",
            "import { formatTime } from './format'\nexport function view(at: number) {\n  return formatTime(at)\n}\n",
            "view",
        );
        let graphify = Graphify::from_env().unwrap();
        let head = crate::checks::head(repo.path()).unwrap();
        let graph = graph(&graphify, &task, "app", repo.path(), &head).unwrap();
        let answer = ask(
            &graphify,
            &graph,
            &Request::Explain {
                symbol: "formatTime".into(),
            },
        )
        .unwrap();
        assert!(answer.contains("view"), "its caller: {answer}");
        assert!(
            std::fs::read_dir(graph.source.parent().unwrap().join("home"))
                .unwrap()
                .next()
                .is_none(),
            "nothing written to its HOME"
        );
    }

    #[test]
    fn availability_needs_a_program_that_runs() {
        let directory = tempfile::TempDir::new().unwrap();
        assert!(
            Graphify {
                program: directory.path().join("nope")
            }
            .available()
            .unwrap_err()
            .to_string()
            .contains("not installed")
        );
        let program = directory.path().join("graphify");
        write_script(&program, "#!/bin/sh\necho graphify 0.9.111\n");
        assert_eq!(
            Graphify { program }.available().unwrap(),
            "graphify 0.9.111"
        );
    }

    #[test]
    fn requests_are_explain_path_or_query_and_never_options() {
        let request = |input| Request::from_input(&input);
        assert_eq!(
            request(serde_json::json!({"action": "explain", "symbol": " QueueDetails "})).unwrap(),
            Request::Explain {
                symbol: "QueueDetails".into()
            }
        );
        assert!(request(serde_json::json!({"action": "path", "from": "A"})).is_err());
        assert!(request(serde_json::json!({"action": "explain", "symbol": "--force"})).is_err());
        assert!(request(serde_json::json!({"action": "update"})).is_err());
        assert!(
            request(serde_json::json!({"action": "query", "question": "x".repeat(201)})).is_err()
        );
    }

    #[test]
    fn answers_need_the_graph_on_and_count_as_queries() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&directory);
        let graphify = stub(directory.path());
        let config = task.config().unwrap();
        let head = crate::checks::head(repo.path()).unwrap();
        let input = serde_json::json!({"action": "explain", "symbol": "app"});
        let error = answer(&graphify, &task, &config.repos[0], &head, &input)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("devagentics task graph feat-x on"),
            "{error}"
        );
        assert!(
            calls(directory.path()).is_empty(),
            "nothing built while off"
        );

        task.transaction(|tx| {
            tx.state.graph_enabled = true;
            Ok(())
        })
        .unwrap();
        let text = answer(&graphify, &task, &config.repos[0], &head, &input).unwrap();
        assert!(
            text.starts_with(&format!("Code graph of {}", config.repos[0].name)),
            "{text}"
        );
        assert!(text.contains("Node: app"), "{text}");
        assert_eq!(
            task.state()
                .unwrap()
                .timing
                .intervals
                .iter()
                .filter(|interval| interval.category == "graph_queries")
                .count(),
            1
        );
    }

    #[test]
    fn a_long_answer_is_cut_and_says_so() {
        let long = "é".repeat(ANSWER_LIMIT);
        let cut = bounded(&long);
        assert!(cut.len() < long.len() + 100 && cut.contains("[cut at 16 KiB"));
        assert_eq!(bounded("short"), "short");
    }
}
