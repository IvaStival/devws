//! `devagentics task start <folder>`: what `devws agentic` calls to make the
//! task and get the leader's start-up arguments.

use crate::agent_args::{self, Launch, McpServer};
use crate::devws::Devws;
use crate::home::Home;
use crate::log::Kind;
use crate::roles::{Role, Roles};
use crate::task::{Repo, Task, TaskConfig, slug};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// What `devws agentic` needs: the task, the leader's agent and its arguments.
#[derive(Debug, PartialEq)]
pub struct Started {
    pub id: String,
    pub leader_agent: String,
    pub args_file: PathBuf,
}

fn git(folder: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(arguments)
        .output()
        .context("running git")?;
    if !output.status.success() {
        bail!(
            "{} is not a git worktree on a branch: {}",
            folder.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The repo a worktree belongs to: the folder of its checkout, or, for a devws
/// clone (`<owner>/<repo>/.bare`), the repo's own folder.
pub fn repo_name(worktree: &Path) -> Result<String> {
    let common = PathBuf::from(git(
        worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    let root = match common.file_name().and_then(|name| name.to_str()) {
        Some(".bare" | ".git") => common.parent().map(Path::to_path_buf),
        _ => Some(common.clone()),
    };
    root.as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .with_context(|| format!("no repo name for {}", worktree.display()))
}

/// devws names the session after the folder, with `.` and `:` written as `_`.
pub fn session_name(folder: &Path) -> String {
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("dev-{}", name.replace(['.', ':'], "_"))
}

/// What an agent is told when it starts: its role, where it is, and what to do
/// first.
pub fn opening(
    role: &Role,
    config: &TaskConfig,
    agent_id: &str,
    repo: &Repo,
    leader: bool,
) -> String {
    let first = if leader {
        "Call task_status, then tell the user in one line that you are ready for their goal, and wait.".to_string()
    } else {
        "Reply with exactly: ready — then wait for your first order.".to_string()
    };
    format!(
        "{}\n\n---\nTask {} on branch {}. You are {agent_id}; your worktree is {}.\n{first}",
        role.instructions,
        config.id,
        config.branch,
        repo.worktree.display()
    )
}

/// Starts the task for a worktree, or finds the one already started for it.
/// The leader's start-up arguments are written next to the task. Opens nothing:
/// devws opens the workspace.
pub fn start_task(home: &Home, devws: &dyn Devws, folder: &Path, command: &str) -> Result<Started> {
    let folder = folder
        .canonicalize()
        .with_context(|| format!("{} is not a folder", folder.display()))?;
    let branch = git(&folder, &["symbolic-ref", "--short", "HEAD"])?;
    let repo = Repo {
        name: repo_name(&folder)?,
        worktree: folder.clone(),
        check: None,
    };

    let base = slug(&branch);
    let mut id = base.clone();
    let mut copy = 1;
    let task = loop {
        match Task::open(home, &id) {
            Ok(existing)
                if existing
                    .config()?
                    .repos
                    .first()
                    .is_some_and(|first| first.worktree == folder) =>
            {
                break existing;
            }
            Ok(_) => {
                copy += 1;
                id = format!("{base}-{copy}");
            }
            Err(_) => {
                let config = TaskConfig {
                    id: id.clone(),
                    branch: branch.clone(),
                    workspace: session_name(&folder),
                    max_agents: 3,
                    repos: vec![repo.clone()],
                    lanes: Vec::new(),
                };
                let task = Task::create(home, &config)?;
                task.transaction(|tx| {
                    tx.log(
                        Kind::Start,
                        None,
                        None,
                        &format!("task started on {branch} in {}", folder.display()),
                    );
                    Ok(())
                })?;
                break task;
            }
        }
    };

    let config = task.config()?;
    let roles = Roles::load(home, Some(&folder))?;
    let leader = roles.get("leader").context("there is no leader role")?;
    let kind = devws.agent_kind(&leader.agent)?;
    let mcp = McpServer {
        command: command.to_string(),
        task_id: id.clone(),
        config_file: task.dir().join("mcp.json"),
    };
    agent_args::write_mcp_config(&mcp)?;
    let first_message = opening(leader, &config, "the leader", &config.repos[0], true);
    let arguments = agent_args::arguments(&Launch {
        role: leader,
        kind,
        first_message: &first_message,
        mcp: Some(&mcp),
        writable: Vec::new(),
    });
    let args_file = task.dir().join("leader.args");
    agent_args::write_args_file(&args_file, &arguments)?;

    Ok(Started {
        id,
        leader_agent: leader.agent.clone(),
        args_file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_args::Kind as Program;
    use crate::checks::tests::repo;
    use crate::devws::fake::FakeDevws;
    use std::fs;
    use tempfile::TempDir;

    fn branch_repo(branch: &str) -> TempDir {
        let repo = repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", branch]);
        repo
    }

    #[test]
    fn a_session_is_named_as_devws_names_it() {
        assert_eq!(session_name(Path::new("/w/app-feat-x")), "dev-app-feat-x");
        assert_eq!(session_name(Path::new("/w/my.app")), "dev-my_app");
    }

    #[test]
    fn starting_a_task_makes_it_and_the_leaders_arguments() {
        let home_dir = TempDir::new().unwrap();
        let home = Home::at(home_dir.path().to_path_buf());
        let repo = branch_repo("feat/login");
        let devws = FakeDevws::new(home_dir.path()).with_kind("claude", Program::Claude);

        let started = start_task(&home, &devws, repo.path(), "/bin/devagentics").unwrap();
        assert_eq!(started.id, "feat-login");
        assert_eq!(started.leader_agent, "claude");

        let task = Task::open(&home, "feat-login").unwrap();
        let config = task.config().unwrap();
        assert_eq!(config.branch, "feat/login");
        assert_eq!(
            config.workspace,
            session_name(&repo.path().canonicalize().unwrap())
        );
        assert_eq!(
            config.repos[0].worktree,
            repo.path().canonicalize().unwrap()
        );
        assert!(task.log_lines().unwrap()[0].contains("task started"));

        let args = fs::read(&started.args_file).unwrap();
        let args: Vec<_> = args
            .split(|byte| *byte == 0)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .collect();
        assert!(
            args[0].contains("You are the leader") || args[0].contains("leader of an agent team"),
            "{}",
            args[0]
        );
        assert!(args[0].contains("Task feat-login on branch feat/login"));
        assert!(args.contains(&"--mcp-config".to_string()));
        let mcp: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(task.dir().join("mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            mcp["mcpServers"]["devagentics"]["args"],
            serde_json::json!(["mcp", "feat-login"])
        );
    }

    #[test]
    fn starting_again_for_the_same_folder_finds_the_same_task_and_another_folder_gets_its_own() {
        let home_dir = TempDir::new().unwrap();
        let home = Home::at(home_dir.path().to_path_buf());
        let devws = FakeDevws::new(home_dir.path()).with_kind("claude", Program::Claude);
        let one = branch_repo("feat/login");
        let two = branch_repo("feat/login");

        let first = start_task(&home, &devws, one.path(), "d").unwrap();
        let again = start_task(&home, &devws, one.path(), "d").unwrap();
        let other = start_task(&home, &devws, two.path(), "d").unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(other.id, "feat-login-2");
        assert_eq!(home.task_ids().unwrap(), ["feat-login", "feat-login-2"]);
    }

    #[test]
    fn a_folder_that_is_not_on_a_branch_is_refused() {
        let home_dir = TempDir::new().unwrap();
        let home = Home::at(home_dir.path().to_path_buf());
        let devws = FakeDevws::new(home_dir.path()).with_kind("claude", Program::Claude);
        let plain = TempDir::new().unwrap();
        assert!(start_task(&home, &devws, plain.path(), "d").is_err());
        let detached = repo();
        crate::checks::tests::run(detached.path(), &["checkout", "-q", "--detach"]);
        assert!(start_task(&home, &devws, detached.path(), "d").is_err());
        assert!(home.task_ids().unwrap().is_empty());
    }

    #[test]
    fn a_leader_whose_agent_devws_does_not_know_stops_the_start() {
        let home_dir = TempDir::new().unwrap();
        let home = Home::at(home_dir.path().to_path_buf());
        let repo = branch_repo("feat/x");
        let devws = FakeDevws::new(home_dir.path());
        assert!(start_task(&home, &devws, repo.path(), "d").is_err());
    }
}
