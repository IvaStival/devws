//! The one place devAgentics talks to devws: its commands (`devws open`,
//! `devws agent …`, `devws worktree ensure`) and, for its own pane markers,
//! tmux. Everything above this takes a `&dyn Devws`, so it can be tested with
//! a fake.

use crate::agent_args::Kind;
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// An agent's state, as `devws agent state` says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneState {
    Working,
    Waiting,
    Done,
    Idle,
    Gone,
}

impl PaneState {
    fn parse(text: &str) -> Result<Self> {
        match text.trim() {
            "working" => Ok(Self::Working),
            "waiting" => Ok(Self::Waiting),
            "done" => Ok(Self::Done),
            "idle" => Ok(Self::Idle),
            "gone" => Ok(Self::Gone),
            other => bail!("devws agent state said '{other}'"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    Delivered,
    Busy(PaneState),
    Gone,
}

/// How much of an account's plan limits is used, in per cent; `None` when
/// devws does not know.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub five_hour: Option<u32>,
    pub week: Option<u32>,
}

/// What an agent's conversation used: tokens, and the cost at API prices (a
/// plan is not billed so; it shows what the tokens would cost).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    /// Read from and written to the cache.
    pub cache: u64,
    pub cost: f64,
    /// A model had no price: the cost is at least this.
    pub unpriced: bool,
}

impl Usage {
    pub fn plus(self, other: Usage) -> Usage {
        Usage {
            input: self.input + other.input,
            output: self.output + other.output,
            cache: self.cache + other.cache,
            cost: self.cost + other.cost,
            unpriced: self.unpriced || other.unpriced,
        }
    }

    /// `devws agent usage`'s line: "<input> <output> <cache> <US$> <unpriced>".
    fn parse(text: &str) -> Option<Usage> {
        let mut fields = text.split_whitespace();
        Some(Usage {
            input: fields.next()?.parse().ok()?,
            output: fields.next()?.parse().ok()?,
            cache: fields.next()?.parse().ok()?,
            cost: fields.next()?.parse().ok()?,
            unpriced: fields.next()? == "1",
        })
    }
}

pub trait Devws: Send + Sync {
    fn agent_kind(&self, agent: &str) -> Result<Kind>;
    /// The folder of the branch's worktree of a repo (`owner/repo`, a URL or a
    /// folder), made when needed. Opens nothing.
    fn ensure_worktree(&self, repo: &str, branch: &str) -> Result<PathBuf>;
    /// A workspace of its own on a folder, the agent as its main one. Returns
    /// the session's name.
    fn open_workspace(
        &self,
        folder: &Path,
        agent: &str,
        task: &str,
        args_file: &Path,
    ) -> Result<String>;
    /// A panel in the team area of the leader's workspace. Returns its pane id.
    /// A new agent panel in a group of the team (`Seniors`, `Floor`): a
    /// workspace of its own beside the team's, four agents to a group, the
    /// next ones in `Floor 2`.
    fn spawn_panel(
        &self,
        session: &str,
        agent: &str,
        group: &str,
        folder: &Path,
        label: &str,
        args_file: &Path,
    ) -> Result<String>;
    fn pane_state(&self, pane: &str) -> Result<PaneState>;
    fn send(&self, pane: &str, text: &str) -> Result<Delivery>;
    /// Marks the agent `waiting` ("needs you") or `done`.
    fn mark(&self, pane: &str, state: &str) -> Result<()>;
    fn stop(&self, pane: &str) -> Result<()>;
    /// Shows the agent: its group's workspace, focused on it.
    fn go(&self, pane: &str) -> Result<()>;
    /// Brings the agent under the board for a look; again, back to its group.
    fn peek(&self, pane: &str) -> Result<()>;
    fn limits(&self, agent: &str) -> Result<Limits>;
    /// What the pane's agent has used; `None` while devws does not know its
    /// conversation yet, or when the pane is gone.
    fn usage(&self, pane: &str) -> Result<Option<Usage>>;
    /// The pane of a workspace with a devws role (`agent`, `team-log`…).
    fn pane_with_role(&self, session: &str, role: &str) -> Result<Option<String>>;
    /// devAgentics's own markers on a pane (`@devagentics_agent`).
    fn set_pane_option(&self, pane: &str, name: &str, value: &str) -> Result<()>;
}

/// Runs the `devws` command (`DEVAGENTICS_DEVWS` names another) and `tmux`
/// (`DEVAGENTICS_TMUX`).
pub struct CommandDevws {
    devws: String,
    tmux: String,
}

impl CommandDevws {
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("DEVAGENTICS_DEVWS").unwrap_or_else(|_| "devws".to_string()),
            std::env::var("DEVAGENTICS_TMUX").unwrap_or_else(|_| "tmux".to_string()),
        )
    }

    pub fn new(devws: String, tmux: String) -> Self {
        Self { devws, tmux }
    }

    fn run(&self, program: &str, arguments: &[&str], stdin: Option<&str>) -> Result<Output> {
        let mut command = Command::new(program);
        command
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = command
            .spawn()
            .with_context(|| format!("running {program}"))?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(text.as_bytes())?;
        }
        child
            .wait_with_output()
            .with_context(|| format!("running {program}"))
    }

    /// The command's output when it succeeded; its own message when it did not.
    fn succeed(&self, program: &str, arguments: &[&str], stdin: Option<&str>) -> Result<String> {
        let output = self.run(program, arguments, stdin)?;
        if !output.status.success() {
            bail!(
                "{program} {} failed: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn devws(&self, arguments: &[&str]) -> Result<String> {
        self.succeed(&self.devws, arguments, None)
    }
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("{} is not text", path.display()))
}

impl Devws for CommandDevws {
    fn agent_kind(&self, agent: &str) -> Result<Kind> {
        let name = self.devws(&["agent", "kind", agent])?;
        Kind::parse(&name)
            .with_context(|| format!("devAgentics does not run {name} agents yet ({agent})"))
    }

    fn ensure_worktree(&self, repo: &str, branch: &str) -> Result<PathBuf> {
        Ok(self.devws(&["worktree", "ensure", repo, branch])?.into())
    }

    fn open_workspace(
        &self,
        folder: &Path,
        agent: &str,
        task: &str,
        args_file: &Path,
    ) -> Result<String> {
        self.devws(&[
            "open",
            "--no-attach",
            "--task",
            task,
            "--agent-args",
            path_text(args_file)?,
            path_text(folder)?,
            agent,
        ])
    }

    fn spawn_panel(
        &self,
        session: &str,
        agent: &str,
        group: &str,
        folder: &Path,
        label: &str,
        args_file: &Path,
    ) -> Result<String> {
        self.devws(&[
            "agent",
            "spawn",
            session,
            agent,
            "--group",
            group,
            "--folder",
            path_text(folder)?,
            "--label",
            label,
            "--agent-args",
            path_text(args_file)?,
        ])
    }

    fn pane_state(&self, pane: &str) -> Result<PaneState> {
        PaneState::parse(&self.devws(&["agent", "state", pane])?)
    }

    fn send(&self, pane: &str, text: &str) -> Result<Delivery> {
        let output = self.run(&self.devws, &["agent", "send", pane, "-"], Some(text))?;
        match output.status.code() {
            Some(0) => Ok(Delivery::Delivered),
            // devws says "busy (working)": the state is asked again, so a
            // change since is not missed.
            Some(3) => Ok(Delivery::Busy(self.pane_state(pane)?)),
            Some(4) => Ok(Delivery::Gone),
            _ => bail!(
                "devws agent send failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }
    }

    fn mark(&self, pane: &str, state: &str) -> Result<()> {
        self.devws(&["agent", "mark", pane, state]).map(drop)
    }

    fn stop(&self, pane: &str) -> Result<()> {
        self.devws(&["agent", "stop", pane]).map(drop)
    }

    fn go(&self, pane: &str) -> Result<()> {
        self.devws(&["agent", "go", pane]).map(drop)
    }

    fn peek(&self, pane: &str) -> Result<()> {
        self.devws(&["agent", "peek", pane]).map(drop)
    }

    fn limits(&self, agent: &str) -> Result<Limits> {
        let text = self.devws(&["agent", "limits", agent])?;
        let mut fields = text
            .split_whitespace()
            .map(|field| field.parse::<u32>().ok());
        Ok(Limits {
            five_hour: fields.next().flatten(),
            week: fields.next().flatten(),
        })
    }

    fn usage(&self, pane: &str) -> Result<Option<Usage>> {
        // Informational: a pane that went or a log that cannot be read is no
        // reason to stop the board.
        Ok(self
            .devws(&["agent", "usage", pane])
            .ok()
            .and_then(|text| Usage::parse(&text)))
    }

    fn pane_with_role(&self, session: &str, role: &str) -> Result<Option<String>> {
        let target = format!("={session}");
        let listing = self.succeed(
            &self.tmux,
            &[
                "list-panes",
                "-s",
                "-t",
                &target,
                "-F",
                "#{pane_id} #{@devws_role}",
            ],
            None,
        )?;
        Ok(listing.lines().find_map(|line| {
            let (pane, pane_role) = line.split_once(' ')?;
            (pane_role == role).then(|| pane.to_string())
        }))
    }

    fn set_pane_option(&self, pane: &str, name: &str, value: &str) -> Result<()> {
        self.succeed(
            &self.tmux,
            &["set-option", "-p", "-t", pane, name, value],
            None,
        )
        .map(drop)
    }
}

#[cfg(test)]
pub mod fake {
    //! A devws that lives in memory, for testing what is built on the trait.
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Inner {
        calls: Vec<String>,
        states: HashMap<String, PaneState>,
        kinds: HashMap<String, Kind>,
        limits: HashMap<String, Limits>,
        usages: HashMap<String, Usage>,
        roles: HashMap<(String, String), String>,
        options: HashMap<(String, String), String>,
        sent: Vec<(String, String)>,
        next_pane: u32,
        worktrees: PathBuf,
    }

    pub struct FakeDevws {
        inner: Mutex<Inner>,
    }

    impl FakeDevws {
        /// Worktrees are made as folders under `worktrees`.
        pub fn new(worktrees: &Path) -> Self {
            let inner = Inner {
                worktrees: worktrees.to_path_buf(),
                next_pane: 100,
                ..Inner::default()
            };
            Self {
                inner: Mutex::new(inner),
            }
        }

        pub fn with_kind(self, agent: &str, kind: Kind) -> Self {
            self.inner
                .lock()
                .unwrap()
                .kinds
                .insert(agent.to_string(), kind);
            self
        }

        pub fn with_limits(self, agent: &str, limits: Limits) -> Self {
            self.inner
                .lock()
                .unwrap()
                .limits
                .insert(agent.to_string(), limits);
            self
        }

        pub fn set_usage(&self, pane: &str, usage: Usage) {
            self.inner
                .lock()
                .unwrap()
                .usages
                .insert(pane.to_string(), usage);
        }

        pub fn set_state(&self, pane: &str, state: PaneState) {
            self.inner
                .lock()
                .unwrap()
                .states
                .insert(pane.to_string(), state);
        }

        pub fn set_role(&self, session: &str, role: &str, pane: &str) {
            self.inner
                .lock()
                .unwrap()
                .roles
                .insert((session.to_string(), role.to_string()), pane.to_string());
        }

        pub fn calls(&self) -> Vec<String> {
            self.inner.lock().unwrap().calls.clone()
        }

        /// What was typed into each pane: (pane, text).
        pub fn sent(&self) -> Vec<(String, String)> {
            self.inner.lock().unwrap().sent.clone()
        }

        pub fn option(&self, pane: &str, name: &str) -> Option<String> {
            self.inner
                .lock()
                .unwrap()
                .options
                .get(&(pane.to_string(), name.to_string()))
                .cloned()
        }

        fn record(&self, call: String) -> std::sync::MutexGuard<'_, Inner> {
            let mut inner = self.inner.lock().unwrap();
            inner.calls.push(call);
            inner
        }

        fn new_pane(inner: &mut Inner) -> String {
            inner.next_pane += 1;
            let pane = format!("%{}", inner.next_pane);
            inner.states.insert(pane.clone(), PaneState::Idle);
            pane
        }
    }

    impl Devws for FakeDevws {
        fn agent_kind(&self, agent: &str) -> Result<Kind> {
            let inner = self.record(format!("kind {agent}"));
            inner
                .kinds
                .get(agent)
                .copied()
                .with_context(|| format!("unknown agent {agent}"))
        }

        fn ensure_worktree(&self, repo: &str, branch: &str) -> Result<PathBuf> {
            let inner = self.record(format!("worktree {repo} {branch}"));
            let name = repo.rsplit('/').next().unwrap_or(repo);
            let folder = inner
                .worktrees
                .join(format!("{name}-{}", branch.replace('/', "-")));
            std::fs::create_dir_all(&folder)?;
            if !folder.join(".git").exists() {
                for arguments in [
                    &["init", "-q", "-b", branch][..],
                    &[
                        "-c",
                        "user.name=t",
                        "-c",
                        "user.email=t@t",
                        "commit",
                        "-q",
                        "--allow-empty",
                        "-m",
                        "first",
                    ],
                ] {
                    let status = std::process::Command::new("git")
                        .arg("-C")
                        .arg(&folder)
                        .args(arguments)
                        .status()?;
                    anyhow::ensure!(status.success(), "git {arguments:?} failed");
                }
            }
            Ok(folder)
        }

        fn open_workspace(
            &self,
            folder: &Path,
            agent: &str,
            task: &str,
            args_file: &Path,
        ) -> Result<String> {
            let mut inner = self.record(format!(
                "open {} {agent} {task} {}",
                folder.display(),
                args_file.display()
            ));
            let session = format!("dev-{}", folder.file_name().unwrap().to_string_lossy());
            let pane = Self::new_pane(&mut inner);
            inner
                .roles
                .insert((session.clone(), "agent".to_string()), pane);
            Ok(session)
        }

        fn spawn_panel(
            &self,
            session: &str,
            agent: &str,
            group: &str,
            folder: &Path,
            label: &str,
            args_file: &Path,
        ) -> Result<String> {
            let mut inner = self.record(format!(
                "spawn {session} {agent} {group} {label} {} in {}",
                args_file.display(),
                folder.display()
            ));
            Ok(Self::new_pane(&mut inner))
        }

        fn pane_state(&self, pane: &str) -> Result<PaneState> {
            let inner = self.record(format!("state {pane}"));
            Ok(inner.states.get(pane).copied().unwrap_or(PaneState::Gone))
        }

        fn send(&self, pane: &str, text: &str) -> Result<Delivery> {
            let mut inner = self.record(format!("send {pane}"));
            match inner.states.get(pane).copied().unwrap_or(PaneState::Gone) {
                PaneState::Gone => Ok(Delivery::Gone),
                state @ (PaneState::Working | PaneState::Waiting) => Ok(Delivery::Busy(state)),
                PaneState::Done | PaneState::Idle => {
                    inner.sent.push((pane.to_string(), text.to_string()));
                    // The agent starts on it.
                    inner.states.insert(pane.to_string(), PaneState::Working);
                    Ok(Delivery::Delivered)
                }
            }
        }

        fn mark(&self, pane: &str, state: &str) -> Result<()> {
            let mut inner = self.record(format!("mark {pane} {state}"));
            inner.states.insert(
                pane.to_string(),
                if state == "waiting" {
                    PaneState::Waiting
                } else {
                    PaneState::Done
                },
            );
            Ok(())
        }

        fn stop(&self, pane: &str) -> Result<()> {
            let mut inner = self.record(format!("stop {pane}"));
            inner.states.insert(pane.to_string(), PaneState::Gone);
            Ok(())
        }

        fn go(&self, pane: &str) -> Result<()> {
            drop(self.record(format!("go {pane}")));
            Ok(())
        }

        fn peek(&self, pane: &str) -> Result<()> {
            drop(self.record(format!("peek {pane}")));
            Ok(())
        }

        fn limits(&self, agent: &str) -> Result<Limits> {
            let inner = self.record(format!("limits {agent}"));
            Ok(inner.limits.get(agent).copied().unwrap_or_default())
        }

        fn usage(&self, pane: &str) -> Result<Option<Usage>> {
            let inner = self.record(format!("usage {pane}"));
            Ok(inner.usages.get(pane).copied())
        }

        fn pane_with_role(&self, session: &str, role: &str) -> Result<Option<String>> {
            let inner = self.record(format!("role {session} {role}"));
            Ok(inner
                .roles
                .get(&(session.to_string(), role.to_string()))
                .cloned())
        }

        fn set_pane_option(&self, pane: &str, name: &str, value: &str) -> Result<()> {
            let mut inner = self.record(format!("option {pane} {name} {value}"));
            inner
                .options
                .insert((pane.to_string(), name.to_string()), value.to_string());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    /// A `devws` and a `tmux` that record how they were called and answer from
    /// the first word of their arguments.
    fn command_devws(dir: &TempDir) -> (CommandDevws, PathBuf) {
        let log = dir.path().join("calls");
        let devws = dir.path().join("devws");
        fs::write(
            &devws,
            format!(
                r#"#!/bin/sh
echo "devws $*" >> "{log}"
case "$1 $2" in
  "agent kind") case "$3" in claude|work) echo claude ;; codex) echo codex ;; opencode) echo opencode ;; *) echo "unknown agent" >&2; exit 1 ;; esac ;;
  "worktree ensure") echo "/work/$3-$4" ;;
  "open --no-attach") echo "dev-$8" ;;
  "agent spawn") echo "%7" ;;
  "agent state") case "$3" in %1) echo working ;; %2) echo gone ;; *) echo done ;; esac ;;
  "agent send") cat >> "{log}.sent"; case "$3" in %1) echo "busy (working)" >&2; exit 3 ;; %2) exit 4 ;; esac ;;
  "agent limits") case "$3" in claude) echo "42 63" ;; *) echo "- -" ;; esac ;;
  "agent mark"|"agent stop") ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#,
                log = log.display()
            ),
        )
        .unwrap();
        let tmux = dir.path().join("tmux");
        fs::write(
            &tmux,
            format!(
                r#"#!/bin/sh
echo "tmux $*" >> "{log}"
case "$1" in
  list-panes) printf '%%1 editor\n%%2 agent\n%%3 team-log\n' ;;
esac
"#,
                log = log.display()
            ),
        )
        .unwrap();
        for script in [&devws, &tmux] {
            fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        (
            CommandDevws::new(devws.display().to_string(), tmux.display().to_string()),
            log,
        )
    }

    fn calls(log: &Path) -> Vec<String> {
        fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn it_runs_the_devws_commands_with_the_arguments_devws_documents() {
        let dir = TempDir::new().unwrap();
        let (devws, log) = command_devws(&dir);

        assert_eq!(devws.agent_kind("work").unwrap(), Kind::Claude);
        assert!(devws.agent_kind("nobody").is_err());
        assert!(
            devws.agent_kind("opencode").is_err(),
            "an agent kind devAgentics does not run is refused"
        );
        assert_eq!(
            devws.ensure_worktree("acme/app", "feat/x").unwrap(),
            PathBuf::from("/work/acme/app-feat/x")
        );
        assert_eq!(
            devws
                .open_workspace(Path::new("/work/app"), "claude", "t1", Path::new("/t/args"))
                .unwrap(),
            "dev-claude"
        );
        assert_eq!(
            devws
                .spawn_panel(
                    "dev-app",
                    "codex",
                    "Floor",
                    Path::new("/w/app"),
                    "Backend",
                    Path::new("/t/args")
                )
                .unwrap(),
            "%7"
        );
        devws.mark("%7", "waiting").unwrap();
        devws.stop("%7").unwrap();

        let calls = calls(&log);
        assert!(calls.contains(&"devws worktree ensure acme/app feat/x".to_string()));
        assert!(calls.contains(
            &"devws open --no-attach --task t1 --agent-args /t/args /work/app claude".to_string()
        ));
        assert!(
            calls.contains(
                &"devws agent spawn dev-app codex --group Floor --folder /w/app --label Backend --agent-args /t/args"
                    .to_string()
            )
        );
        assert!(calls.contains(&"devws agent mark %7 waiting".to_string()));
        assert!(calls.contains(&"devws agent stop %7".to_string()));
    }

    #[test]
    fn it_reads_states_limits_and_panes() {
        let dir = TempDir::new().unwrap();
        let (devws, log) = command_devws(&dir);

        assert_eq!(devws.pane_state("%1").unwrap(), PaneState::Working);
        assert_eq!(devws.pane_state("%2").unwrap(), PaneState::Gone);
        assert_eq!(devws.pane_state("%3").unwrap(), PaneState::Done);
        assert_eq!(
            devws.limits("claude").unwrap(),
            Limits {
                five_hour: Some(42),
                week: Some(63)
            }
        );
        assert_eq!(
            devws.limits("codex").unwrap(),
            Limits {
                five_hour: None,
                week: None
            }
        );
        assert_eq!(
            devws.pane_with_role("dev-app", "agent").unwrap().as_deref(),
            Some("%2")
        );
        assert_eq!(
            devws
                .pane_with_role("dev-app", "team-log")
                .unwrap()
                .as_deref(),
            Some("%3")
        );
        assert_eq!(devws.pane_with_role("dev-app", "database").unwrap(), None);
        devws
            .set_pane_option("%7", "@devagentics_agent", "backend-1")
            .unwrap();

        let calls = calls(&log);
        assert!(
            calls.contains(
                &"tmux list-panes -s -t =dev-app -F #{pane_id} #{@devws_role}".to_string()
            )
        );
        assert!(
            calls.contains(&"tmux set-option -p -t %7 @devagentics_agent backend-1".to_string())
        );
    }

    #[test]
    fn it_tells_a_busy_agent_from_a_gone_one() {
        let dir = TempDir::new().unwrap();
        let (devws, log) = command_devws(&dir);

        assert_eq!(
            devws.send("%3", "line one\nline two").unwrap(),
            Delivery::Delivered
        );
        assert_eq!(
            devws.send("%1", "later").unwrap(),
            Delivery::Busy(PaneState::Working)
        );
        assert_eq!(devws.send("%2", "nobody").unwrap(), Delivery::Gone);
        // The text goes in on standard input, whole.
        assert_eq!(
            fs::read_to_string(format!("{}.sent", log.display())).unwrap(),
            "line one\nline twolaternobody"
        );
    }
}
