//! The team: which agents there are, where they run, and the rules that keep
//! them in check (one writer per worktree, a limit on how many, no writers
//! before the user approves the plan, no agents on an account near its limit).

use crate::agent_args::{self, Launch};
use crate::devws::{Devws, PaneState};
use crate::home::Home;
use crate::lanes;
use crate::log::Kind;
use crate::orders;
use crate::roles::{Role, Roles};
use crate::start::{AGENT_FIRST, opening, repo_name};
use crate::state::{Agent, OrderStatus, StepStatus};
use crate::task::{Repo, Task, TaskConfig};
use anyhow::{Context, Result, bail};
use std::time::Duration;

/// An account this far into its 5-hour limit gets no new agents.
const LIMIT_PERCENT: u32 = 85;

pub struct Team<'a> {
    pub task: &'a Task,
    pub home: &'a Home,
    pub devws: &'a dyn Devws,
}

fn label(role: &Role) -> String {
    let mut characters = role.name.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().chain(characters).collect())
        .unwrap_or_default()
}

fn find_repo<'c>(config: &'c TaskConfig, name: Option<&str>) -> Result<&'c Repo> {
    match name {
        Some(name) => config
            .repos
            .iter()
            .find(|repo| repo.name == name)
            .with_context(|| {
                let known: Vec<_> = config.repos.iter().map(|repo| repo.name.as_str()).collect();
                format!(
                    "the task has no repo {name} (it has: {}); add_repo adds one",
                    known.join(", ")
                )
            }),
        None => config.repos.first().context("the task has no repos"),
    }
}

impl Team<'_> {
    /// Another repo for the task: a worktree on the task's branch.
    pub fn add_repo(&self, spec: &str, check: Option<&str>) -> Result<Repo> {
        let mut config = self.task.config()?;
        let worktree = self.devws.ensure_worktree(spec, &config.branch)?;
        let name = repo_name(&worktree)?;
        if config.repos.iter().any(|repo| repo.name == name) {
            bail!("the task already has the repo {name}");
        }
        let repo = Repo {
            name: name.clone(),
            worktree,
            check: check.map(str::to_string),
        };
        config.repos.push(repo.clone());
        self.task.write_config(&config)?;
        self.task.transaction(|tx| {
            tx.log(
                Kind::Repo,
                None,
                None,
                &format!("added {name} at {}", repo.worktree.display()),
            );
            Ok(())
        })?;
        Ok(repo)
    }

    pub fn set_check(&self, repo_name: &str, command: &str) -> Result<()> {
        let mut config = self.task.config()?;
        config
            .repos
            .iter_mut()
            .find(|repo| repo.name == repo_name)
            .with_context(|| format!("the task has no repo {repo_name}"))?
            .check = Some(command.to_string());
        self.task.write_config(&config)
    }

    /// Starts an agent with a role in a repo's worktree (the task's first repo
    /// by default).
    #[cfg(test)]
    pub fn spawn_agent(&self, role_name: &str, repo_name: Option<&str>) -> Result<Agent> {
        self.spawn_agent_in(role_name, repo_name, None)
    }

    /// Starts an agent; one that writes, in a lane when one is named: the
    /// lane's worktree, its repo. A repo with lanes takes its writers only in
    /// them, so they never share a worktree.
    pub fn spawn_agent_in(
        &self,
        role_name: &str,
        repo_name: Option<&str>,
        lane_name: Option<&str>,
    ) -> Result<Agent> {
        let config = self.task.config()?;
        let lane = lane_name
            .map(|name| lanes::get(self.task, name))
            .transpose()?;
        let repo = find_repo(
            &config,
            lane.as_ref().map(|lane| lane.repo.as_str()).or(repo_name),
        )?
        .clone();
        let roles = Roles::load(self.home, Some(&repo.worktree))?;
        let role = roles.get(role_name).with_context(|| {
            format!(
                "there is no role {role_name} (roles: {})",
                roles
                    .all()
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if role.name == "leader" {
            bail!("the leader is you: it is already running");
        }
        if lane.is_some() && !role.writes {
            bail!(
                "{} does not write the repo: lanes are for the factory floor",
                role.name
            );
        }
        if role.writes && lane.is_none() {
            let of_repo: Vec<_> = config
                .lanes
                .iter()
                .filter(|lane| lane.repo == repo.name)
                .map(|lane| lane.name.as_str())
                .collect();
            if !of_repo.is_empty() {
                bail!(
                    "{} has lanes ({}): start the agent in one (lane)",
                    repo.name,
                    of_repo.join(", ")
                );
            }
        }
        self.devws.agent_kind(&role.agent)?;
        if let Some(used) = self
            .devws
            .limits(&role.agent)?
            .five_hour
            .filter(|used| *used >= LIMIT_PERCENT)
        {
            bail!(
                "{} has used {used}% of its 5-hour limit: no new agents on it until it resets",
                role.agent
            );
        }

        // The agent is reserved before it starts, so two spawns cannot both
        // pass the rules.
        let task_id = config.id.clone();
        let id = self.task.transaction(|tx| {
            if tx.state.paused {
                bail!("the task is paused: `devagentics task resume {task_id}`");
            }
            if role.writes && !tx.state.approved {
                bail!("the plan is not approved: the user approves it (the leader calls approve_plan once they say yes, or `devagentics task approve {task_id}`); until then only roles that do not write may start");
            }
            let counted = tx.state.agents.iter().filter(|agent| agent.live).count();
            if counted >= config.max_agents {
                bail!("{counted} agents are running, the most this task allows: stop one first");
            }
            let lane_name = lane.as_ref().map(|lane| lane.name.clone());
            if role.writes
                && let Some(writer) = tx.state.agents.iter().find(|agent| agent.live && agent.writes && agent.repo == repo.name && agent.lane == lane_name)
            {
                let place = lane_name.as_ref().map_or_else(|| repo.name.clone(), |lane| format!("the lane {lane}"));
                bail!("{} already writes in {place}: one writer per worktree; give it the order, or use another lane", writer.id);
            }
            let number = tx.state.agents.iter().filter(|agent| agent.role == role.name).count() + 1;
            let id = format!("{}-{number}", role.name);
            tx.state.agents.push(Agent {
                id: id.clone(),
                role: role.name.clone(),
                agent: role.agent.clone(),
                repo: repo.name.clone(),
                session: String::new(),
                pane: String::new(),
                writes: role.writes,
                group: Some(role.group),
                lane: lane_name,
                live: true,
                ready: false,
                lost: false,
                usage: None,
            });
            Ok(id)
        })?;

        // Where it works: its lane's worktree, else its repo's.
        let mut workplace = repo.clone();
        if let Some(lane) = &lane {
            workplace.worktree = lane.worktree.clone();
        }
        match self.launch(&config, role, &workplace, &id) {
            Ok((session, pane)) => self.task.transaction(|tx| {
                let agent = tx
                    .state
                    .agents
                    .iter_mut()
                    .find(|agent| agent.id == id)
                    .expect("the agent was reserved");
                agent.session = session;
                agent.pane = pane;
                let started = agent.clone();
                tx.log(
                    Kind::Spawn,
                    Some(&id),
                    None,
                    &match &lane {
                        Some(lane) => format!("{} in {}, lane {}", role.name, repo.name, lane.name),
                        None => format!("{} in {}", role.name, repo.name),
                    },
                );
                Ok(started)
            }),
            Err(error) => {
                self.task.transaction(|tx| {
                    tx.state.agents.retain(|agent| agent.id != id);
                    Ok(())
                })?;
                Err(error)
            }
        }
    }

    /// Runs the agent: a panel in the leader's workspace for the task's first
    /// repo, a workspace of its own for another. Returns its session and pane.
    fn launch(
        &self,
        config: &TaskConfig,
        role: &Role,
        repo: &Repo,
        id: &str,
    ) -> Result<(String, String)> {
        let kind = self.devws.agent_kind(&role.agent)?;
        let first_message = opening(role, config, id, repo, AGENT_FIRST);
        let arguments = agent_args::arguments(&Launch {
            role,
            kind,
            first_message: &first_message,
            mcp: None,
            writable: [self.home.root().to_path_buf()]
                .into_iter()
                .chain(crate::checks::git_data(&repo.worktree).ok())
                .collect(),
        });
        let directory = self.task.dir().join("agents");
        std::fs::create_dir_all(&directory)?;
        let args_file = directory.join(format!("{id}.args"));
        agent_args::write_args_file(&args_file, &arguments)?;

        let (session, pane) = if config
            .repos
            .first()
            .is_some_and(|first| first.name == repo.name)
        {
            (
                config.workspace.clone(),
                self.devws.spawn_panel(
                    &config.workspace,
                    &role.agent,
                    role.group.workspace(),
                    &repo.worktree,
                    &label(role),
                    &args_file,
                )?,
            )
        } else {
            let session =
                self.devws
                    .open_workspace(&repo.worktree, &role.agent, &config.id, &args_file)?;
            let pane = self.agent_pane_of(&session)?;
            (session, pane)
        };
        self.devws
            .set_pane_option(&pane, "@devagentics_agent", id)?;
        self.devws.set_pane_option(&pane, "@devws_team", "1")?;
        Ok((session, pane))
    }

    /// The agent pane of a workspace that was only just opened: its role is
    /// set a moment after the workspace is up.
    fn agent_pane_of(&self, session: &str) -> Result<String> {
        for _ in 0..50 {
            if let Some(pane) = self.devws.pane_with_role(session, "agent")? {
                return Ok(pane);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!("the workspace {session} has no agent pane")
    }

    /// Ends an agent: its panel closes, its worktree is free for another
    /// writer, and an order it still had is blocked.
    pub fn stop_agent(&self, id: &str) -> Result<()> {
        let agent = self
            .task
            .state()?
            .agent(id)
            .filter(|agent| agent.live)
            .cloned()
            .with_context(|| format!("there is no live agent {id}"))?;
        let usage = self.devws.usage(&agent.pane)?;
        if self.devws.pane_state(&agent.pane)? != PaneState::Gone {
            self.devws.stop(&agent.pane)?;
        }
        self.task.transaction(|tx| {
            if let Some(stopped) = tx
                .state
                .agents
                .iter_mut()
                .find(|candidate| candidate.id == id)
            {
                stopped.live = false;
                stopped.usage = usage;
            }
            let open: Vec<_> = tx
                .state
                .orders
                .iter_mut()
                .filter(|order| order.agent == id && order.status == OrderStatus::Open)
                .collect();
            let steps: Vec<_> = open.iter().map(|order| order.step.clone()).collect();
            for order in tx
                .state
                .orders
                .iter_mut()
                .filter(|order| order.agent == id && order.status == OrderStatus::Open)
            {
                order.status = OrderStatus::Closed;
            }
            for step in steps {
                if let Some(step) = tx.state.step_mut(&step) {
                    step.status = StepStatus::Blocked;
                }
            }
            tx.state.queue.retain(|message| message.to != id);
            tx.log(Kind::Stop, Some(id), None, "stopped");
            Ok(())
        })
    }

    /// The user approves the plan: agents that write may start.
    pub fn approve(&self) -> Result<()> {
        let plan = std::fs::read_to_string(self.task.dir().join("plan.md")).unwrap_or_default();
        if plan.trim().is_empty() {
            bail!(
                "there is no plan to approve: a read-only agent writes it with `devagentics plan write`"
            );
        }
        self.task.transaction(|tx| {
            tx.state.approved = true;
            tx.log(Kind::Approve, None, None, "plan approved");
            Ok(())
        })
    }

    pub fn write_plan(&self, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            bail!("the plan is empty");
        }
        let path = self.task.dir().join("plan.md");
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, &path)?;
        self.task.transaction(|tx| {
            // A new plan has to be approved again.
            tx.state.approved = false;
            tx.log(
                Kind::Plan,
                None,
                None,
                &format!("plan written ({} lines)", text.lines().count()),
            );
            Ok(())
        })
    }

    pub fn pause(&self) -> Result<()> {
        self.task.transaction(|tx| {
            tx.state.paused = true;
            tx.log(Kind::Pause, None, None, "paused");
            Ok(())
        })
    }

    /// Takes the task off pause, and starts again the agents whose panels
    /// went (a restart) as new conversations, each told its open order again.
    /// Returns their ids.
    pub fn resume(&self) -> Result<Vec<String>> {
        let config = self.task.config()?;
        self.task.transaction(|tx| {
            tx.state.paused = false;
            tx.log(Kind::Pause, None, None, "resumed");
            Ok(())
        })?;

        let mut restarted = Vec::new();
        for agent in self.task.state()?.agents.iter().filter(|agent| agent.live) {
            if !agent.lost && self.devws.pane_state(&agent.pane)? != PaneState::Gone {
                continue;
            }
            let repo = lanes::workplace(self.task, agent)?;
            let roles = Roles::load(self.home, Some(&repo.worktree))?;
            let role = roles
                .get(&agent.role)
                .with_context(|| format!("the role {} of {} is gone", agent.role, agent.id))?;
            let (session, pane) = self.launch(&config, role, &repo, &agent.id)?;
            self.task.transaction(|tx| {
                let order = tx.state.open_order_of(&agent.id).cloned();
                let title = order
                    .as_ref()
                    .and_then(|order| tx.state.steps.iter().find(|step| step.id == order.step))
                    .map(|step| step.title.clone());
                tx.state.queue.retain(|message| message.to != agent.id);
                if let (Some(order), Some(title)) = (order, title) {
                    tx.state.say_about(
                        &agent.id,
                        orders::order_message(&order, &title),
                        Some(order.id.clone()),
                    );
                    if let Some(open) = tx
                        .state
                        .orders
                        .iter_mut()
                        .find(|candidate| candidate.id == order.id)
                    {
                        open.delivered = false;
                        open.stalled = false;
                    }
                }
                let restarted = tx
                    .state
                    .agents
                    .iter_mut()
                    .find(|candidate| candidate.id == agent.id)
                    .expect("the agent is in the state");
                restarted.session = session;
                restarted.pane = pane;
                restarted.ready = false;
                restarted.lost = false;
                tx.log(Kind::Spawn, Some(&agent.id), None, "started again");
                Ok(())
            })?;
            restarted.push(agent.id.clone());
        }
        Ok(restarted)
    }

    /// Stops every agent and the task with them; the worktrees stay.
    pub fn cancel(&self) -> Result<()> {
        self.pause()?;
        for agent in self.task.state()?.agents.iter().filter(|agent| agent.live) {
            self.stop_agent(&agent.id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_args::Kind as Program;
    use crate::devws::Limits;
    use crate::devws::fake::FakeDevws;
    use crate::start::start_task;
    use crate::state::Message;
    use tempfile::TempDir;

    struct Fixture {
        home_dir: TempDir,
        home: Home,
        task: Task,
        devws: FakeDevws,
        repo: TempDir,
    }

    fn fixture() -> Fixture {
        let home_dir = TempDir::new().unwrap();
        let home = Home::at(home_dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&home_dir.path().join("worktrees"))
            .with_kind("claude", Program::Claude)
            .with_kind("codex", Program::Codex);
        start_task(&home, &devws, repo.path(), "/bin/devagentics").unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        Fixture {
            home_dir,
            home,
            task,
            devws,
            repo,
        }
    }

    impl Fixture {
        fn team(&self) -> Team<'_> {
            Team {
                task: &self.task,
                home: &self.home,
                devws: &self.devws,
            }
        }

        fn approved(self) -> Self {
            self.team().write_plan("1. do it\n").unwrap();
            self.team().approve().unwrap();
            self
        }
    }

    #[test]
    fn a_planner_may_start_before_approval_but_a_writer_may_not() {
        let f = fixture();
        let planner = f.team().spawn_agent("planner", None).unwrap();
        assert_eq!(planner.id, "planner-1");

        let error = f
            .team()
            .spawn_agent("executor", None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not approved") && error.contains("devagentics task approve feat-x"),
            "{error}"
        );
        assert!(
            f.team()
                .approve()
                .unwrap_err()
                .to_string()
                .contains("no plan"),
            "nothing to approve yet"
        );

        f.team().write_plan("1. step one\n").unwrap();
        f.team().approve().unwrap();
        assert!(f.team().spawn_agent("executor", None).is_ok());
        f.team().write_plan("1. step one, changed\n").unwrap();
        assert!(
            f.team().spawn_agent("executor", None).is_err(),
            "a changed plan has to be approved again"
        );
    }

    #[test]
    fn a_spawned_agent_is_a_panel_of_the_leaders_workspace_marked_as_the_teams() {
        let f = fixture().approved();
        let agent = f.team().spawn_agent("executor", None).unwrap();

        let workspace = f.task.config().unwrap().workspace;
        assert_eq!(
            (agent.session.as_str(), agent.live, agent.ready),
            (workspace.as_str(), true, false)
        );
        assert!(
            f.devws
                .calls()
                .iter()
                .any(|call| call.starts_with(&format!("spawn {workspace} claude Floor Executor "))),
            "{:?}",
            f.devws.calls()
        );
        assert_eq!(
            f.devws.option(&agent.pane, "@devagentics_agent").as_deref(),
            Some("executor-1")
        );
        assert_eq!(
            f.devws.option(&agent.pane, "@devws_team").as_deref(),
            Some("1")
        );

        let args = std::fs::read(f.task.dir().join("agents/executor-1.args")).unwrap();
        let first =
            String::from_utf8_lossy(args.split(|byte| *byte == 0).next().unwrap()).into_owned();
        assert!(
            first.contains("You are executor-1") && first.contains("Reply with exactly: ready"),
            "{first}"
        );
        assert!(
            f.task
                .log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("\"kind\":\"spawn\""))
        );
    }

    #[test]
    fn one_writer_per_worktree_and_a_limit_on_how_many_agents() {
        let f = fixture().approved();
        f.team().spawn_agent("executor", None).unwrap();
        let error = f
            .team()
            .spawn_agent("executor", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("executor-1 already writes"), "{error}");

        // A reader counts as an agent too.
        std::fs::create_dir_all(f.home.roles_dir()).unwrap();
        std::fs::write(
            f.home.roles_dir().join("reviewer.md"),
            "+++\nagent = \"claude\"\n+++\nReview.\n",
        )
        .unwrap();
        f.team().spawn_agent("reviewer", None).unwrap();
        f.team().spawn_agent("reviewer", None).unwrap();
        let error = f
            .team()
            .spawn_agent("reviewer", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("3 agents are running"), "{error}");

        // Stopping an agent frees its place and, for a writer, its worktree.
        f.team().stop_agent("executor-1").unwrap();
        assert!(f.team().spawn_agent("executor", None).is_ok());
        assert!(
            f.team().spawn_agent("reviewer", None).is_err(),
            "the place was taken again"
        );
    }

    #[test]
    fn an_account_near_its_limit_gets_no_new_agents_and_an_unknown_one_does() {
        let f = fixture().approved();
        let devws = FakeDevws::new(&f.home_dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_limits(
                "claude",
                Limits {
                    five_hour: Some(85),
                    week: Some(10),
                },
            );
        let team = Team {
            task: &f.task,
            home: &f.home,
            devws: &devws,
        };
        let error = team.spawn_agent("executor", None).unwrap_err().to_string();
        assert!(
            error.contains("85%") || error.contains("used 85"),
            "{error}"
        );
        assert!(
            f.task.state().unwrap().agents.is_empty(),
            "a refused spawn leaves no agent behind"
        );

        let below = FakeDevws::new(&f.home_dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_limits(
                "claude",
                Limits {
                    five_hour: Some(84),
                    week: None,
                },
            );
        assert!(
            Team {
                task: &f.task,
                home: &f.home,
                devws: &below
            }
            .spawn_agent("executor", None)
            .is_ok()
        );
        let unknown =
            FakeDevws::new(&f.home_dir.path().join("w")).with_kind("claude", Program::Claude);
        assert!(
            Team {
                task: &f.task,
                home: &f.home,
                devws: &unknown
            }
            .spawn_agent("planner", None)
            .is_ok(),
            "limits devws does not know do not stop an agent"
        );
    }

    #[test]
    fn a_paused_task_starts_nothing_and_unknown_roles_and_the_leader_are_refused() {
        let f = fixture();
        f.team().pause().unwrap();
        assert!(
            f.team()
                .spawn_agent("planner", None)
                .unwrap_err()
                .to_string()
                .contains("paused")
        );
        f.team().resume().unwrap();
        assert!(f.team().spawn_agent("planner", None).is_ok());
        assert!(
            f.team()
                .spawn_agent("nobody", None)
                .unwrap_err()
                .to_string()
                .contains("no role nobody")
        );
        assert!(
            f.team()
                .spawn_agent("leader", None)
                .unwrap_err()
                .to_string()
                .contains("already running")
        );
        assert!(
            f.team()
                .spawn_agent("planner", Some("elsewhere"))
                .unwrap_err()
                .to_string()
                .contains("add_repo")
        );
    }

    #[test]
    fn another_repo_gets_its_own_workspace_with_the_agent_as_its_main_one() {
        let f = fixture().approved();
        let repo = f.team().add_repo("acme/web", Some("npm test")).unwrap();
        assert_eq!(f.task.config().unwrap().repos.len(), 2);
        assert_eq!(repo.check.as_deref(), Some("npm test"));
        assert!(
            f.team()
                .add_repo("acme/web", None)
                .unwrap_err()
                .to_string()
                .contains("already has")
        );

        let agent = f.team().spawn_agent("executor", Some(&repo.name)).unwrap();
        let calls = f.devws.calls();
        assert!(
            calls
                .iter()
                .any(|call| call.starts_with("worktree acme/web feat/x")),
            "{calls:?}"
        );
        assert!(
            calls.iter().any(|call| call
                .starts_with(&format!("open {} claude feat-x ", repo.worktree.display()))),
            "{calls:?}"
        );
        assert_eq!(
            agent.session,
            format!(
                "dev-{}",
                repo.worktree.file_name().unwrap().to_string_lossy()
            )
        );
        assert!(!agent.pane.is_empty());

        // The first repo's writer does not block the other repo's.
        assert!(f.team().spawn_agent("executor", None).is_ok());
    }

    #[test]
    fn a_failed_start_gives_the_agents_place_back() {
        let f = fixture();
        std::fs::write(f.repo.path().join(".keep"), "").unwrap();
        let broken = FakeDevws::new(&f.home_dir.path().join("w"));
        let team = Team {
            task: &f.task,
            home: &f.home,
            devws: &broken,
        };
        assert!(
            team.spawn_agent("planner", None).is_err(),
            "devws does not know the agent"
        );
        assert!(f.task.state().unwrap().agents.is_empty());
    }

    #[test]
    fn stopping_closes_the_panel_frees_the_worktree_and_blocks_the_order() {
        let f = fixture().approved();
        let agent = f.team().spawn_agent("executor", None).unwrap();
        f.task
            .transaction(|tx| {
                tx.state.agents[0].ready = true;
                tx.state.steps.push(crate::state::Step {
                    id: "s1".into(),
                    title: "T".into(),
                    status: StepStatus::Ordered,
                    attempts: 1,
                });
                tx.state.orders.push(crate::state::Order {
                    id: "o1".into(),
                    step: "s1".into(),
                    agent: "executor-1".into(),
                    text: "x".into(),
                    allowed: vec![],
                    forbidden: vec![],
                    check: None,
                    base: "abc".into(),
                    status: OrderStatus::Open,
                    delivered: false,
                    stalled: false,
                    plan: None,
                });
                tx.state.queue.push(Message {
                    id: 1,
                    to: "executor-1".into(),
                    text: "queued".into(),
                    order: None,
                });
                Ok(())
            })
            .unwrap();

        f.team().stop_agent("executor-1").unwrap();
        assert!(f.devws.calls().contains(&format!("stop {}", agent.pane)));
        let state = f.task.state().unwrap();
        assert!(!state.agents[0].live);
        assert_eq!(state.steps[0].status, StepStatus::Blocked);
        assert_eq!(state.orders[0].status, OrderStatus::Closed);
        assert!(
            state.queue.is_empty(),
            "nothing is left to be typed into a stopped agent"
        );
        assert!(
            f.team().stop_agent("executor-1").is_err(),
            "it is already stopped"
        );
        assert!(f.team().spawn_agent("executor", None).is_ok());
    }

    #[test]
    fn resuming_starts_the_agents_whose_panels_are_gone_and_tells_them_their_order_again() {
        let f = fixture().approved();
        let agent = f.team().spawn_agent("executor", None).unwrap();
        f.task
            .transaction(|tx| {
                tx.state.agents[0].ready = true;
                tx.state.steps.push(crate::state::Step {
                    id: "s1".into(),
                    title: "Add it".into(),
                    status: StepStatus::Ordered,
                    attempts: 1,
                });
                tx.state.orders.push(crate::state::Order {
                    id: "o1".into(),
                    step: "s1".into(),
                    agent: "executor-1".into(),
                    text: "Do the thing".into(),
                    allowed: vec![],
                    forbidden: vec![],
                    check: None,
                    base: "abc".into(),
                    status: OrderStatus::Open,
                    delivered: false,
                    stalled: false,
                    plan: None,
                });
                tx.state.say("executor-1", "stale copy".into());
                Ok(())
            })
            .unwrap();
        f.devws.set_state(&agent.pane, PaneState::Gone);
        f.team().pause().unwrap();

        assert_eq!(f.team().resume().unwrap(), ["executor-1"]);
        let state = f.task.state().unwrap();
        assert!(!state.paused);
        assert_ne!(state.agents[0].pane, agent.pane, "it has a new panel");
        assert!(
            !state.agents[0].ready,
            "and a new conversation to answer first"
        );
        assert_eq!(state.queue.len(), 1);
        assert!(
            state.queue[0].text.contains("ORDER o1")
                && state.queue[0].text.contains("Do the thing"),
            "{}",
            state.queue[0].text
        );

        // Nothing to restart now.
        assert!(f.team().resume().unwrap().is_empty());
    }

    #[test]
    fn cancelling_stops_every_agent_and_pauses_the_task() {
        let f = fixture().approved();
        f.team().spawn_agent("executor", None).unwrap();
        f.team().spawn_agent("planner", None).unwrap();
        f.team().cancel().unwrap();
        let state = f.task.state().unwrap();
        assert!(state.paused && state.agents.iter().all(|agent| !agent.live));
        assert!(f.repo.path().exists(), "the worktrees stay");
    }

    #[test]
    fn the_checks_of_a_repo_can_be_set_by_the_leader() {
        let f = fixture();
        let name = f.task.config().unwrap().repos[0].name.clone();
        f.team().set_check(&name, "make test").unwrap();
        assert_eq!(
            f.task.config().unwrap().repos[0].check.as_deref(),
            Some("make test")
        );
        assert!(f.team().set_check("nope", "x").is_err());
    }
}
