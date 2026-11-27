use crate::home::Home;
use crate::log::{Entry, Kind};
use crate::state::State;
use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

const DEFAULT_MAX_AGENTS: usize = 6;

fn default_max_agents() -> usize {
    DEFAULT_MAX_AGENTS
}

/// Orders a step may have. When they are used, devAgentics waits and gives it
/// one more (`watch::RETRY_AFTER_SECS`); a step that still fails needs a person.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

fn default_max_attempts() -> u32 {
    DEFAULT_MAX_ATTEMPTS
}

/// Writers one lane may have at once, each in a seat of its own
/// (`lanes::seat`); `max_agents` still caps them all.
pub const DEFAULT_MAX_SEATS: usize = 2;

fn default_max_seats() -> usize {
    DEFAULT_MAX_SEATS
}

/// Minutes an agent may have nothing open before the dispatcher closes it
/// (`idle.rs`); 0 keeps them.
pub const DEFAULT_IDLE_CLOSE_MINUTES: u64 = 30;

fn default_idle_close_minutes() -> u64 {
    DEFAULT_IDLE_CLOSE_MINUTES
}

/// A task's settings, in `task.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskConfig {
    pub id: String,
    pub branch: String,
    /// The devws session of the leader's workspace.
    pub workspace: String,
    #[serde(default = "default_max_agents")]
    pub max_agents: usize,
    /// Orders (and reviews asking for changes) a step gets; the user allows
    /// more for one step (`devagentics task attempts`).
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Writers one lane may have at once (`lanes::seat`).
    #[serde(default = "default_max_seats")]
    pub max_seats: usize,
    /// Minutes an agent may have nothing open before it is closed (`idle.rs`);
    /// 0 keeps every agent until the leader stops it.
    #[serde(default = "default_idle_close_minutes")]
    pub idle_close_minutes: u64,
    #[serde(default)]
    pub check_profiles: std::collections::BTreeMap<String, crate::check_profiles::RepositoryChecks>,
    #[serde(default)]
    pub repos: Vec<Repo>,
    /// Areas of a repo the factory floor works on side by side, each in a
    /// worktree of its own.
    #[serde(default)]
    pub lanes: Vec<Lane>,
    /// `off` gives every item at least standard rigor: today's full review,
    /// whatever its facts say. The rollback for risk routing.
    #[serde(default)]
    pub routing: crate::execution::RoutingMode,
    /// What the team may spend; nothing is limited by default.
    #[serde(default, skip_serializing_if = "Budget::is_unlimited")]
    pub budget: Budget,
}

/// `[budget]` in task.toml. At 80% of a limit the leader is told; at the
/// limit the task pauses, nothing is lost, and the user decides: raise it in
/// task.toml and resume, or stop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Tokens every model of the team processed (input, output and cache),
    /// as the panes report them: an estimate, providers count differently.
    #[serde(default)]
    pub max_tokens: Option<u64>,
    /// Hours since the task started.
    #[serde(default)]
    pub max_hours: Option<f64>,
}

impl Budget {
    pub fn is_unlimited(&self) -> bool {
        self.max_tokens.is_none() && self.max_hours.is_none()
    }
}

/// An area of a repo (back, front, docs…): its files, and the worktree on a
/// branch of its own off the task's, where one agent writes them while
/// agents of other lanes write theirs. Accepted work merges into the task's
/// branch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    pub name: String,
    pub repo: String,
    /// The files that are the lane's, from the repo's root.
    pub globs: Vec<String>,
    pub branch: String,
    pub worktree: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Repo {
    pub name: String,
    pub worktree: PathBuf,
    /// The command that decides a step is done in this repo.
    #[serde(default)]
    pub check: Option<String>,
}

/// "feat/login" becomes "feat-login": the branch as a task id.
pub fn slug(branch: &str) -> String {
    let mut slug = String::new();
    for character in branch.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "task".to_string()
    } else {
        slug.to_string()
    }
}

/// What a transaction changes: the state, and the log entries to append.
pub struct Tx {
    pub state: State,
    entries: Vec<Entry>,
}

impl Tx {
    pub fn log(&mut self, kind: Kind, agent: Option<&str>, step: Option<&str>, text: &str) {
        self.entries.push(Entry::new(kind, agent, step, text));
    }
}

/// A task's folder, `~/.devagentics/tasks/<id>/`.
#[derive(Clone, Debug)]
pub struct Task {
    dir: PathBuf,
}

impl Task {
    pub fn create(home: &Home, config: &TaskConfig) -> Result<Self> {
        let dir = home.task_dir(&config.id);
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let task = Self { dir };
        task.write_config(config)?;
        task.transaction(|_| Ok(()))?;
        Ok(task)
    }

    pub fn open(home: &Home, id: &str) -> Result<Self> {
        let dir = home.task_dir(id);
        if !dir.join("task.toml").is_file() {
            anyhow::bail!("no task named {id}");
        }
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    pub fn config(&self) -> Result<TaskConfig> {
        let path = self.dir.join("task.toml");
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn write_config(&self, config: &TaskConfig) -> Result<()> {
        let text = toml::to_string_pretty(config)?;
        write_atomically(&self.dir.join("task.toml"), text.as_bytes())
    }

    /// Reads the state under an exclusive lock, lets `change` alter it, and
    /// writes it back (atomically) with the log entries it made. When `change`
    /// fails nothing is written. The MCP server, the dispatcher and
    /// `devagentics report` all change the same task: this is how they meet.
    pub fn transaction<T>(&self, change: impl FnOnce(&mut Tx) -> Result<T>) -> Result<T> {
        let lock = File::create(self.dir.join("state.lock")).context("opening the task's lock")?;
        lock.lock_exclusive().context("locking the task")?;

        let state_path = self.dir.join("state.json");
        let (state, read) = match fs::read_to_string(&state_path) {
            Ok(text) => (
                serde_json::from_str(&text)
                    .with_context(|| format!("parsing {}", state_path.display()))?,
                Some(text),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (State::default(), None),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", state_path.display()));
            }
        };
        let mut tx = Tx {
            state,
            entries: Vec::new(),
        };

        let value = change(&mut tx)?;

        // The same state is not written again: a check that changed nothing
        // made every waiting reader (a worker's panel) read it all again.
        let text = serde_json::to_string_pretty(&tx.state)?;
        if read.as_deref() != Some(text.as_str()) {
            write_atomically(&state_path, text.as_bytes())?;
        }
        if !tx.entries.is_empty() {
            let mut log = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join("log.jsonl"))
                .context("opening the log")?;
            for entry in &tx.entries {
                writeln!(log, "{}", serde_json::to_string(entry)?)?;
            }
        }
        Ok(value)
    }

    /// The state as it is now.
    /// Serializes terminal effects with pause and queued-order amendments.
    pub fn control_lock(&self) -> Result<File> {
        let lock = File::create(self.dir.join("control.lock"))?;
        lock.lock_exclusive()?;
        Ok(lock)
    }

    pub fn state(&self) -> Result<State> {
        // Status polling must not rewrite the entire task or contend as a writer.
        let lock_path = self.dir.join("state.lock");
        let lock = match File::open(&lock_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)?,
            existing => existing?,
        };
        fs2::FileExt::lock_shared(&lock)?;
        match fs::read_to_string(self.dir.join("state.json")) {
            Ok(text) => serde_json::from_str(&text).context("reading task state"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(error) => Err(error).context("reading task state"),
        }
    }

    /// When the state was last written and its size: a reader that waits
    /// (a worker's panel) parses the state again only when this changes, as
    /// a big task's state takes a while to read.
    pub fn state_stamp(&self) -> Result<Option<(std::time::SystemTime, u64)>> {
        match fs::metadata(self.dir.join("state.json")) {
            Ok(metadata) => Ok(Some((metadata.modified()?, metadata.len()))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).context("reading the task state's stamp"),
        }
    }

    pub fn log_lines(&self) -> Result<Vec<String>> {
        match fs::read_to_string(self.dir.join("log.jsonl")) {
            Ok(text) => Ok(text.lines().map(str::to_string).collect()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error).context("reading the log"),
        }
    }
}

/// The file is written beside its place and moved there, so a reader never
/// sees half of it.
fn write_atomically(path: &PathBuf, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Kind;
    use tempfile::TempDir;

    fn config(id: &str) -> TaskConfig {
        TaskConfig {
            id: id.to_string(),
            branch: "feat/login".to_string(),
            workspace: "dev-app".to_string(),
            max_agents: 3,
            max_attempts: crate::task::DEFAULT_MAX_ATTEMPTS,
            max_seats: crate::task::DEFAULT_MAX_SEATS,
            idle_close_minutes: crate::task::DEFAULT_IDLE_CLOSE_MINUTES,
            check_profiles: Default::default(),
            lanes: Vec::new(),
            routing: Default::default(),
            budget: Default::default(),
            repos: vec![Repo {
                name: "app".to_string(),
                worktree: "/work/app-feat-login".into(),
                check: Some("make test".to_string()),
            }],
        }
    }

    #[test]
    fn status_reads_leave_persisted_state_untouched() {
        let directory = TempDir::new().unwrap();
        let home = Home::at(directory.path().into());
        let task = Task::create(&home, &config("readonly-state")).unwrap();
        let path = task.dir().join("state.json");
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        let contents = fs::read(&path).unwrap();
        for _ in 0..10 {
            assert!(!task.state().unwrap().approved);
        }
        assert_eq!(fs::read(&path).unwrap(), contents);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    }

    // A devagentics still running after an upgrade (a leader's MCP server)
    // saves the state too: the fields only the new build knows must survive it.
    #[test]
    fn a_transaction_keeps_the_fields_a_newer_build_wrote() {
        let directory = TempDir::new().unwrap();
        let home = Home::at(directory.path().into());
        let task = Task::create(&home, &config("newer-fields")).unwrap();
        let path = task.dir().join("state.json");
        let mut written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        written["from_a_newer_build"] = serde_json::json!({"container": "c", "until": 9});
        fs::write(&path, written.to_string()).unwrap();

        task.transaction(|tx| {
            tx.state.paused = true;
            Ok(())
        })
        .unwrap();

        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["paused"], true);
        assert_eq!(
            saved["from_a_newer_build"],
            serde_json::json!({"container": "c", "until": 9})
        );
    }

    // Something that looks without changing anything (a panel's check, a
    // pass with nothing to do) used to write a big state back several times a
    // second, and every waiting panel then read it all again.
    #[test]
    fn a_transaction_that_changes_nothing_writes_nothing() {
        let directory = TempDir::new().unwrap();
        let home = Home::at(directory.path().into());
        let task = Task::create(&home, &config("unchanged")).unwrap();
        task.transaction(|tx| {
            tx.state.paused = true;
            Ok(())
        })
        .unwrap();
        let before = task.state_stamp().unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        task.transaction(|tx| {
            tx.state.paused = true;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            task.state_stamp().unwrap().unwrap(),
            before,
            "the same state is not written again"
        );
        task.transaction(|tx| {
            tx.log(Kind::Review, None, None, "a log line alone");
            Ok(())
        })
        .unwrap();
        assert_eq!(task.state_stamp().unwrap().unwrap(), before);
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("a log line alone"))
        );
        task.transaction(|tx| {
            tx.state.paused = false;
            Ok(())
        })
        .unwrap();
        assert_ne!(
            task.state_stamp().unwrap().unwrap(),
            before,
            "a change is written"
        );
    }

    #[test]
    fn the_state_stamp_moves_with_a_write_and_not_with_a_read() {
        let directory = TempDir::new().unwrap();
        let home = Home::at(directory.path().into());
        let task = Task::create(&home, &config("stamp")).unwrap();
        let before = task.state_stamp().unwrap().unwrap();
        task.state().unwrap();
        assert_eq!(
            task.state_stamp().unwrap().unwrap(),
            before,
            "a read changes nothing"
        );
        task.transaction(|tx| {
            tx.state.paused = true;
            Ok(())
        })
        .unwrap();
        assert_ne!(
            task.state_stamp().unwrap().unwrap(),
            before,
            "a write moves it"
        );
    }

    #[test]
    fn a_task_toml_from_before_the_attempt_limit_gets_the_default() {
        let config: TaskConfig =
            toml::from_str("id = \"t\"\nbranch = \"feat/x\"\nworkspace = \"dev\"\n").unwrap();
        assert_eq!(config.max_attempts, DEFAULT_MAX_ATTEMPTS);
        assert_eq!(config.max_agents, DEFAULT_MAX_AGENTS);
    }

    #[test]
    fn a_branch_becomes_a_slug() {
        assert_eq!(slug("feat/login"), "feat-login");
        assert_eq!(slug("Fix: Crash on  Save!"), "fix-crash-on-save");
        assert_eq!(slug("///"), "task");
    }

    #[test]
    fn a_created_task_reads_back_with_an_empty_state() {
        let home = TempDir::new().unwrap();
        let home = Home::at(home.path().to_path_buf());
        let task = Task::create(&home, &config("feat-login")).unwrap();

        assert_eq!(task.config().unwrap(), config("feat-login"));
        let state = task.transaction(|tx| Ok(tx.state.clone())).unwrap();
        assert_eq!(state, State::default());
        assert!(Task::open(&home, "feat-login").is_ok());
        assert!(Task::open(&home, "missing").is_err());
    }

    #[test]
    fn a_failed_transaction_changes_nothing() {
        let home = TempDir::new().unwrap();
        let home = Home::at(home.path().to_path_buf());
        let task = Task::create(&home, &config("t")).unwrap();

        let result: anyhow::Result<()> = task.transaction(|tx| {
            tx.state.approved = true;
            tx.log(Kind::Start, None, None, "never written");
            anyhow::bail!("refused")
        });
        assert!(result.is_err());

        assert!(!task.transaction(|tx| Ok(tx.state.approved)).unwrap());
        assert!(task.log_lines().unwrap().is_empty());
    }

    #[test]
    fn concurrent_transactions_lose_nothing() {
        let home = TempDir::new().unwrap();
        let home = Home::at(home.path().to_path_buf());
        let task = Task::create(&home, &config("t")).unwrap();

        let workers: Vec<_> = (0..4)
            .map(|_| {
                let task = task.clone();
                std::thread::spawn(move || {
                    for _ in 0..25 {
                        task.transaction(|tx| {
                            tx.state.next_id += 1;
                            tx.log(Kind::Start, None, None, "tick");
                            Ok(())
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }

        assert_eq!(task.transaction(|tx| Ok(tx.state.next_id)).unwrap(), 100);
        assert_eq!(task.log_lines().unwrap().len(), 100);
    }

    #[test]
    fn log_entries_carry_their_kind_time_agent_and_step() {
        let home = TempDir::new().unwrap();
        let home = Home::at(home.path().to_path_buf());
        let task = Task::create(&home, &config("t")).unwrap();

        task.transaction(|tx| {
            tx.log(Kind::Start, Some("backend-1"), Some("step-2"), "hello");
            Ok(())
        })
        .unwrap();

        let lines = task.log_lines().unwrap();
        assert_eq!(lines.len(), 1);
        let entry: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(entry["kind"], "start");
        assert_eq!(entry["agent"], "backend-1");
        assert_eq!(entry["step"], "step-2");
        assert_eq!(entry["text"], "hello");
        assert!(entry["at"].as_u64().unwrap() > 1_700_000_000);
    }

    #[test]
    fn tasks_are_listed_by_id() {
        let home = TempDir::new().unwrap();
        let home = Home::at(home.path().to_path_buf());
        assert!(home.task_ids().unwrap().is_empty());
        Task::create(&home, &config("b-task")).unwrap();
        Task::create(&home, &config("a-task")).unwrap();
        assert_eq!(home.task_ids().unwrap(), vec!["a-task", "b-task"]);
    }
}
