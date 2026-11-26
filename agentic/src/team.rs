//! The team: which agents there are, where they run, and the rules that keep
//! them in check (one writer per worktree, a limit on how many, no writers
//! before the user approves the plan, no agents on an account near its limit).

use crate::agent_args::{self, Launch};
use crate::devws::{Devws, PaneState};
use crate::home::Home;
use crate::lanes;
use crate::log::Kind;
use crate::opencode::OpencodeSettings;
use crate::orders;
use crate::providers::Kind as Program;
use crate::roles::{Role, Roles};
use crate::start::{AGENT_FIRST, opening, repo_name};
use crate::state::{Agent, OrderStatus, StepStatus, UsageApproval};
use crate::task::{Repo, Task, TaskConfig};
use crate::worker::{FailureScope, LaunchState, StartupFailure, program_name};
use anyhow::{Context, Result, bail};
use std::time::Duration;

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
        let _control = self.task.control_lock()?;
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
        let _control = self.task.control_lock()?;
        let mut config = self.task.config()?;
        if !command.trim().is_empty()
            && config
                .check_profiles
                .get(repo_name)
                .is_some_and(|profiles| !profiles.selected.is_empty())
        {
            bail!("clear named profile selection before setting a legacy check");
        }
        config
            .repos
            .iter_mut()
            .find(|repo| repo.name == repo_name)
            .with_context(|| format!("the task has no repo {repo_name}"))?
            .check = if command.trim().is_empty() {
            None
        } else {
            Some(command.to_string())
        };
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
    #[cfg(test)]
    pub fn spawn_agent_in(
        &self,
        role_name: &str,
        repo_name: Option<&str>,
        lane_name: Option<&str>,
    ) -> Result<Agent> {
        self.spawn_agent_on(role_name, repo_name, lane_name, None)
    }

    /// Starts an agent as `spawn_agent_in` does; on `account` only when one
    /// is named: the user asked for it, so nothing else stands in when it
    /// cannot start.
    pub fn spawn_agent_on(
        &self,
        role_name: &str,
        repo_name: Option<&str>,
        lane_name: Option<&str>,
        account: Option<&str>,
    ) -> Result<Agent> {
        let _control = self.task.control_lock()?;
        crate::workflow::block_unverified_runner(self.task)?;
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
        let kind = self
            .devws
            .agent_kind(&role.agent)
            .or_else(|error| Program::parse(&role.agent).ok_or(error))?;
        let mut launch = self.launch_state(role, kind)?;
        if let Some(account) = account {
            self.pin_account(&mut launch, account)?;
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
            let counted = tx.state.live_agents(role.writes);
            if counted >= config.max_agents {
                bail!("{counted} {} are running, the most this task allows: stop one first", kind_of_agents(role.writes));
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
            tx.state.worker_launches.insert(id.clone(), launch.clone());
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
                unavailable: false,
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
        match self.launch_worker(&config, &workplace, &id, &mut launch) {
            Ok((session, pane)) => self.task.transaction(|tx| {
                let agent = tx
                    .state
                    .agents
                    .iter_mut()
                    .find(|agent| agent.id == id)
                    .expect("the agent was reserved");
                agent.agent = launch
                    .selected
                    .as_ref()
                    .expect("launched candidate")
                    .agent
                    .clone();
                agent.session = session;
                agent.pane = pane;
                let started = agent.clone();
                tx.log(
                    Kind::Spawn,
                    Some(&id),
                    None,
                    &match &lane {
                        Some(lane) => format!(
                            "{} in {}, lane {} on {}",
                            role.name,
                            repo.name,
                            lane.name,
                            program_name(launch.selected.as_ref().unwrap())
                        ),
                        None => format!(
                            "{} in {} on {}",
                            role.name,
                            repo.name,
                            program_name(launch.selected.as_ref().unwrap())
                        ),
                    },
                );
                if !launch.failures.is_empty() {
                    tx.log(
                        Kind::Spawn,
                        Some(&id),
                        None,
                        &format!("fallback reasons: {}", launch.failures.join("; ")),
                    );
                }
                Ok(started)
            }),
            Err(error) => {
                self.task.transaction(|tx| {
                    if error.is::<crate::worker::UsageLimitRequired>() {
                        if let Some(agent) = tx.state.agents.iter_mut().find(|agent| agent.id == id)
                        {
                            agent.live = false;
                            agent.unavailable = true;
                            agent.lost = true;
                        }
                    } else {
                        tx.state.agents.retain(|agent| agent.id != id);
                    }
                    Ok(())
                })?;
                Err(error)
            }
        }
    }

    /// Starts a lost or unavailable worker again, on its first account that
    /// may take it, and tells it its open order (or review) again.
    fn relaunch(&self, config: &TaskConfig, agent: &Agent) -> Result<()> {
        let state = self.task.state()?;
        let startup = (|| -> Result<_> {
            if !agent.live {
                if state.live_agents(agent.writes) >= config.max_agents {
                    bail!(
                        "cannot resume {}: the places for {} are full",
                        agent.id,
                        kind_of_agents(agent.writes)
                    );
                }
                if agent.writes
                    && state.agents.iter().any(|candidate| {
                        candidate.live
                            && candidate.writes
                            && candidate.repo == agent.repo
                            && candidate.lane == agent.lane
                    })
                {
                    bail!(
                        "cannot resume {}: another writer occupies its worktree",
                        agent.id
                    );
                }
                if agent.writes && !state.approved {
                    bail!("cannot resume writer: the plan is not approved");
                }
            }
            let repo = lanes::workplace(self.task, agent)?;
            let roles = Roles::load(self.home, Some(&repo.worktree))?;
            let role = roles
                .get(&agent.role)
                .with_context(|| format!("the role {} of {} is gone", agent.role, agent.id))?;
            // Automatic retries use a snapshot. Explicit resume may pick up corrected
            // provider settings while retaining this worker's instructions and scope.
            let resumed_role =
                if let Some(previous) = self.task.state()?.worker_launches.get(&agent.id) {
                    Role {
                        fallback: role.fallback.clone(),
                        alternatives: role.alternatives.clone(),
                        ..previous.role.with_program(&role.program())
                    }
                } else {
                    role.clone()
                };
            if resumed_role.writes != agent.writes || resumed_role.group != agent.group() {
                bail!(
                    "cannot resume {}: its role's write scope or group changed",
                    agent.id
                );
            }
            let kind = self
                .devws
                .agent_kind(&resumed_role.agent)
                .or_else(|error| Program::parse(&resumed_role.agent).ok_or(error))?;
            let mut launch = self.launch_state(&resumed_role, kind)?;
            // A failed cleanup is not permission to duplicate the still-live process.
            if agent.unavailable && !self.pane_gone(&agent.pane)? {
                self.devws
                    .stop(&agent.pane)
                    .context("close the unavailable worker before resuming")?;
            }
            let (session, pane) = self.launch_worker(config, &repo, &agent.id, &mut launch)?;
            Ok((session, pane, launch))
        })();
        let (session, pane, launch) = startup?;
        self.task.transaction(|tx| {
            let order = tx.state.open_order_of(&agent.id).cloned();
            let title = order
                .as_ref()
                .and_then(|order| tx.state.steps.iter().find(|step| step.id == order.step))
                .map(|step| step.title.clone());
            tx.state.cancel_probe(&agent.id);
            tx.state.queue.retain(|message| message.to != agent.id);
            if let (Some(order), Some(title)) = (order, title) {
                tx.state.say_about(
                    &agent.id,
                    orders::order_message(
                        &order,
                        &title,
                        tx.state
                            .order_profiles
                            .get(&order.id)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]),
                    ),
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
            if let Some(message) = crate::reviews::resumption(&tx.state, &agent.id) {
                tx.state.say(&agent.id, message);
            }
            let restarted = tx
                .state
                .agents
                .iter_mut()
                .find(|candidate| candidate.id == agent.id)
                .expect("the agent is in the state");
            restarted.agent = launch.selected.as_ref().unwrap().agent.clone();
            restarted.session = session;
            restarted.pane = pane;
            restarted.live = true;
            restarted.unavailable = false;
            restarted.ready = false;
            restarted.lost = false;
            tx.state.restart_on_next_account.remove(&agent.id);
            tx.log(Kind::Spawn, Some(&agent.id), None, "started again");
            Ok(())
        })?;
        Ok(())
    }

    /// The dispatcher restarts a worker its account refused before it did
    /// anything (worker_tools `hold_and_restart`): that worker alone, on its
    /// next account, without pausing the team. Never while the task is
    /// paused: the user's resume brings it back then. The caller holds the
    /// control lock (the dispatcher's pass does): taking it again here would
    /// wait forever, since a second lock on the file blocks its own process.
    pub fn restart_on_next_account(&self, agent_id: &str) -> Result<bool> {
        let state = self.task.state()?;
        if state.paused || !state.restart_on_next_account.contains(agent_id) {
            return Ok(false);
        }
        let agent = state.agent(agent_id).context("unknown worker")?.clone();
        self.task.transaction(|tx| {
            tx.state.restart_on_next_account.remove(agent_id);
            Ok(())
        })?;
        let config = self.task.config()?;
        if let Err(error) = self.relaunch(&config, &agent) {
            self.task.transaction(|tx| {
                tx.state.say(
                    crate::state::LEADER,
                    format!("{agent_id} could not start again on another account: {error:#}. Resume it when one is free, or stop it."),
                );
                Ok(())
            })?;
            return Ok(false);
        }
        Ok(true)
    }

    /// Whether the agent's pane no longer runs. An agent the usage guard
    /// held back before it started never had a pane.
    fn pane_gone(&self, pane: &str) -> Result<bool> {
        Ok(pane.is_empty() || self.devws.pane_state(pane)? == PaneState::Gone)
    }

    /// The OpenCode settings this task's agents start with: the home-wide
    /// ones, with the models the user picked for this task.
    fn opencode(&self) -> Result<OpencodeSettings> {
        Ok(crate::accounts::opencode_for_task(
            OpencodeSettings::load(self.home)?,
            &self.task.state()?,
        ))
    }

    /// The candidates a worker of `role` starts on, in the order tried.
    fn launch_state(&self, role: &Role, kind: Program) -> Result<LaunchState> {
        let mut launch = LaunchState::new(role, kind, &self.opencode()?);
        self.add_enabled_accounts(&mut launch)?;
        self.prefer_codex(&mut launch.candidates)?;
        Ok(launch)
    }

    /// Every account the user left on for the role's group joins after the
    /// role's own candidates: a role names a few programs, the user's
    /// switches decide which accounts the team may use. OpenCode joins only
    /// through its own switch, which names its model.
    fn add_enabled_accounts(&self, launch: &mut LaunchState) -> Result<()> {
        let state = self.task.state()?;
        let group = launch.role.group;
        for (account, kind) in self.devws.accounts()? {
            let is_candidate = launch
                .candidates
                .iter()
                .any(|candidate| candidate.agent == account);
            if is_candidate
                || kind.provider().own_switches()
                || !crate::accounts::is_enabled_for(&state, &account, group)
            {
                continue;
            }
            let model = match group {
                crate::roles::Group::Floor => crate::roles::floor_model(kind, None),
                _ => None,
            };
            launch.candidates.push(crate::roles::Fallback {
                agent: account,
                model,
                effort: None,
                approvals: None,
                sandbox: None,
            });
        }
        Ok(())
    }

    /// Keeps only `account`'s candidate.
    fn pin_account(&self, launch: &mut LaunchState, account: &str) -> Result<()> {
        // devws runs opencode without listing it among its accounts: what it
        // runs is the test.
        let kind = self
            .devws
            .agent_kind(account)
            .with_context(|| format!("there is no account {account} in devws"))?;
        let group = launch.role.group;
        if !crate::accounts::is_enabled_for(&self.task.state()?, account, group) {
            bail!(
                "{account} is switched off for the {} group of this task: the user turns it on on the board",
                group.name()
            );
        }
        let candidate = launch
            .candidates
            .iter()
            .find(|candidate| candidate.agent == account)
            .cloned()
            .with_context(|| {
                if kind.provider().own_switches() {
                    format!(
                        "{account} is switched off for the {} group, or has no model: the user switches it on (devagentics opencode switch, model)",
                        group.name()
                    )
                } else {
                    format!("{} cannot start on {account}", launch.role.name)
                }
            })?;
        launch.candidates = vec![candidate];
        Ok(())
    }

    /// Persist attempts before effects so a dispatcher restart cannot loop candidates.
    fn launch_worker(
        &self,
        config: &TaskConfig,
        repo: &Repo,
        id: &str,
        launch: &mut LaunchState,
    ) -> Result<(String, String)> {
        loop {
            let state = self.task.state()?;
            let approved = state.usage_limit_approval == UsageApproval::Approved;
            let candidate = launch.next_available(
                self.devws,
                approved,
                &crate::accounts::disabled_for(&state, launch.role.group),
                &crate::accounts::opencode_for_task(OpencodeSettings::load(self.home)?, &state),
            );
            // The model the user picked on the board for the group on that
            // account replaces the role's.
            let candidate = candidate.map(|role| {
                let Some(model) = crate::accounts::group_model(&state, &role.agent, role.group)
                else {
                    return role;
                };
                if let Some(selected) = launch.selected.as_mut() {
                    selected.model = Some(model.to_string());
                }
                Role {
                    model: Some(model.to_string()),
                    ..role
                }
            });
            let needs_consent = candidate
                .as_ref()
                .err()
                .is_some_and(|error| error.is::<crate::worker::UsageLimitRequired>());
            self.task.transaction(|tx| {
                tx.state.worker_launches.insert(id.into(), launch.clone());
                if needs_consent {
                    tx.state.usage_limit_blocks = launch.percentage_blocks.clone();
                    if tx.state.usage_limit_approval == UsageApproval::NotRequested {
                        tx.state.usage_limit_approval = UsageApproval::Pending;
                        tx.state.say(crate::state::LEADER, "Every worker account the task may use reached the percentage guard or is switched off. Ask the user for task-scoped usage-limit approval, then record their decision with set_usage_limit_approval. Resume existing workers after approval. The rules are in application_workflow action=guide topic=accounts.".into());
                        tx.log(Kind::Review, None, None, "worker usage-limit approval requested");
                    }
                }
                Ok(())
            })?;
            if needs_consent && self.task.state()?.usage_limit_approval == UsageApproval::Denied {
                return Err(anyhow::Error::new(crate::worker::UsageLimitRequired).context(
                    "Worker percentage override denied or revoked. Do not ask again unless the user requests reconsideration; resume when usage resets."
                ));
            }
            let role = candidate?;
            match self.launch(config, &role, repo, id) {
                Ok(started) => return Ok(started),
                Err(error) => {
                    let Some(failure) = error.downcast_ref::<StartupFailure>() else {
                        return Err(error);
                    };
                    if failure.scope == FailureScope::Other {
                        return Err(error);
                    }
                    launch.rejected(failure);
                }
            }
        }
    }

    /// Called under the dispatcher's control lock, before a worker is ready.
    pub fn retry_startup(&self, agent: &Agent, failure: &StartupFailure) -> Result<()> {
        let state = self.task.state()?;
        if state.paused || agent.ready {
            return Ok(());
        }
        let Some(mut launch) = state.worker_launches.get(&agent.id).cloned() else {
            // Older tasks have no candidate snapshot: report instead of guessing settings.
            return self.startup_blocked(agent, &failure.reason);
        };
        launch.rejected(failure);
        self.task.transaction(|tx| {
            tx.state
                .worker_launches
                .insert(agent.id.clone(), launch.clone());
            // A rejected startup must never receive queued work, including after a crash.
            if let Some(current) = tx.state.agents.iter_mut().find(|item| item.id == agent.id) {
                current.lost = true;
            }
            Ok(())
        })?;
        if self.devws.pane_state(&agent.pane)? != PaneState::Gone {
            // Close only this failed task-owned worker; never replay work in a healthy pane.
            if let Err(error) = self.devws.stop(&agent.pane) {
                return self
                    .startup_blocked(agent, &format!("failed to close rejected startup: {error}"));
            }
        }
        if failure.scope == FailureScope::Other {
            return self.startup_blocked(agent, &failure.reason);
        }
        let config = self.task.config()?;
        let workplace = lanes::workplace(self.task, agent)?;
        match self.launch_worker(&config, &workplace, &agent.id, &mut launch) {
            Ok((session, pane)) => self.task.transaction(|tx| {
                let current = tx
                    .state
                    .agents
                    .iter_mut()
                    .find(|item| item.id == agent.id)
                    .context("worker disappeared during startup retry")?;
                current.session = session;
                current.pane = pane;
                current.agent = launch.selected.as_ref().unwrap().agent.clone();
                current.ready = false;
                current.unavailable = false;
                current.lost = false;
                tx.log(
                    Kind::Spawn,
                    Some(&agent.id),
                    None,
                    &format!(
                        "startup fallback to {}: {}",
                        program_name(launch.selected.as_ref().unwrap()),
                        launch.failures.join("; ")
                    ),
                );
                Ok(())
            }),
            Err(error) => self.startup_blocked(agent, &format!("{error:#}")),
        }
    }

    fn startup_blocked(&self, agent: &Agent, reason: &str) -> Result<()> {
        let gone = self.devws.pane_state(&agent.pane)? == PaneState::Gone;
        self.task.transaction(|tx| {
            if let Some(current) = tx.state.agents.iter_mut().find(|item| item.id == agent.id) {
                current.live = !gone;
                current.unavailable = true;
                current.lost = true;
                current.ready = false;
            }
            tx.state.say(crate::state::LEADER, format!("{} unavailable during startup: {reason}. Inspect task_status; resume after correcting availability.", agent.id));
            tx.log(Kind::Lost, Some(&agent.id), None, reason);
            Ok(())
        })
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
        let state = self.task.state()?;
        let reviewed = state.workflow == crate::execution::Workflow::Reviewed;
        if reviewed {
            reviewed_adapter(kind, &role.name, role.writes, CodexPolicy::of(&state))?;
        }
        let first_message = opening(role, config, id, repo, AGENT_FIRST);
        let arguments = if reviewed {
            let state = self.task.state()?;
            let review_source = crate::reviews::pending(&state, id)
                .map(|review| crate::reviews::checkout(self.task, review));
            let source = review_source.as_deref().unwrap_or(&repo.worktree);
            let identity = crate::runner::prepare(self.task, id, source, !role.writes)?;
            let mut args = vec![
                "--devagentics-worker".into(),
                crate::executable::own()?.to_string_lossy().into_owned(),
                config.id.clone(),
                id.into(),
                "--identity".into(),
                identity.to_string_lossy().into_owned(),
            ];
            if let Some(model) = &role.model {
                args.extend(["--model".into(), model.clone()]);
            }
            args
        } else {
            agent_args::arguments(&Launch {
                role,
                kind,
                first_message: &first_message,
                mcp: None,
                writable: [self.home.root().to_path_buf()]
                    .into_iter()
                    .chain(crate::checks::git_data(&repo.worktree).unwrap_or_default())
                    .collect(),
            })
        };
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
            .filter(|agent| agent.live || agent.unavailable)
            .cloned()
            .with_context(|| format!("there is no live agent {id}"))?;
        let usage = if agent.pane.is_empty() {
            None
        } else {
            let usage = self.devws.usage(&agent.pane)?;
            if !self.pane_gone(&agent.pane)? {
                self.devws.stop(&agent.pane)?;
            }
            usage
        };
        self.task.transaction(|tx| {
            if let Some(stopped) = tx
                .state
                .agents
                .iter_mut()
                .find(|candidate| candidate.id == id)
            {
                stopped.live = false;
                stopped.unavailable = false;
                stopped.lost = false;
                stopped.usage = usage;
            }
            let open: Vec<_> = tx
                .state
                .orders
                .iter_mut()
                .filter(|order| order.agent == id && order.status == OrderStatus::Open)
                .collect();
            let steps: Vec<_> = open
                .iter()
                .map(|order| (order.step.clone(), order.id.clone()))
                .collect();
            for order in tx
                .state
                .orders
                .iter_mut()
                .filter(|order| order.agent == id && order.status == OrderStatus::Open)
            {
                order.status = OrderStatus::Closed;
            }
            for (step, order) in steps {
                if let Some(found) = tx.state.step_mut(&step) {
                    found.set_status(StepStatus::Blocked, crate::log::now());
                }
                crate::failures::record(
                    &mut tx.state,
                    &step,
                    Some(&order),
                    crate::failures::FailureKind::AgentStopped,
                    &format!("{id} was stopped with the order open"),
                    crate::log::now(),
                );
            }
            tx.state.cancel_probe(id);
            tx.state.queue.retain(|message| message.to != id);
            tx.log(Kind::Stop, Some(id), None, "stopped");
            Ok(())
        })?;
        // Nothing starts a stopped agent again: a new one gets its own source.
        crate::runner::forget(self.task, id)
    }

    /// The user approves the plan: agents that write may start.
    pub fn approve(&self) -> Result<()> {
        anyhow::ensure!(
            self.task.state()?.workflow == crate::execution::Workflow::Legacy,
            "reviewed workflow requires confirmation and approval of an exact application baseline revision"
        );
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
        self.pause_mode(false, None)
    }

    pub fn pause_mode(&self, interrupt: bool, invoking: Option<&str>) -> Result<()> {
        let _control = self.task.control_lock()?;
        self.task.transaction(|tx| {
            tx.state.paused = true;
            tx.log(Kind::Pause, None, None, "paused");
            Ok(())
        })?;
        if interrupt {
            crate::recovery::interrupt(self.task, self.devws, invoking)?;
        }
        Ok(())
    }

    /// Record only an explicit user decision; this does not launch workers.
    pub fn set_usage_limit_approval(&self, approved: bool) -> Result<()> {
        let _control = self.task.control_lock()?;
        self.task.transaction(|tx| {
            if tx.state.usage_limit_approval == UsageApproval::NotRequested {
                bail!("no eligible usage-limit approval request exists");
            }
            tx.state.usage_limit_approval = if approved {
                UsageApproval::Approved
            } else {
                UsageApproval::Denied
            };
            tx.log(
                Kind::Review,
                None,
                None,
                if approved {
                    "user approved worker percentage override for this task"
                } else {
                    "user denied or revoked worker percentage override for this task"
                },
            );
            Ok(())
        })
    }

    /// Takes the task off pause, and starts again the agents whose panels
    /// went (a restart) as new conversations, each told its open order again.
    /// Returns their ids.
    pub fn resume(&self) -> Result<Vec<String>> {
        self.resume_from(None)
    }

    /// Resumes; `invoking` is the pane that asked (the leader calling
    /// resume_task from its turn): it is ready, as it is the one asking.
    pub fn resume_from(&self, invoking: Option<&str>) -> Result<Vec<String>> {
        let _control = self.task.control_lock()?;
        crate::workflow::block_unverified_runner(self.task)?;
        let config = self.task.config()?;
        self.task.transaction(|tx| {
            tx.state.paused = true;
            tx.log(Kind::Pause, None, None, "preparing resume");
            Ok(())
        })?;

        let mut restarted = Vec::new();
        let mut skipped = Vec::new();
        for agent in self
            .task
            .state()?
            .agents
            .iter()
            .filter(|agent| agent.live || agent.unavailable)
        {
            if !agent.lost && !self.pane_gone(&agent.pane)? {
                continue;
            }
            let state = self.task.state()?;
            let can_skip = agent.unavailable
                && !agent.live
                && agent.pane.is_empty()
                && state.open_order_of(&agent.id).is_none()
                && !state.worker_turns.contains_key(&agent.id)
                && crate::reviews::resumption(&state, &agent.id).is_none();
            match self.relaunch(&config, agent) {
                Ok(()) => restarted.push(agent.id.clone()),
                Err(error) if can_skip => skipped.push(format!("{}: {error:#}", agent.id)),
                Err(error) => return Err(error),
            }
        }
        if !skipped.is_empty()
            && !self
                .task
                .state()?
                .agents
                .iter()
                .any(|agent| agent.live && !agent.lost && !agent.pane.is_empty())
        {
            bail!("cannot resume: {}", skipped.join("; "));
        }
        crate::recovery::queue_resumption(self.task, self.devws, invoking)?;
        self.task.transaction(|tx| {
            tx.state.paused = false;
            tx.log(Kind::Pause, None, None, "resumed");
            if !skipped.is_empty() {
                let notice = format!(
                    "Resumed with unavailable workers eligible for retry: {}",
                    skipped.join("; ")
                );
                tx.log(Kind::Pause, None, None, &notice);
                if !tx
                    .state
                    .queue
                    .iter()
                    .any(|message| message.to == crate::state::LEADER && message.text == notice)
                {
                    tx.state.say(crate::state::LEADER, notice);
                }
            }
            Ok(())
        })?;
        Ok(restarted)
    }

    /// Stops every agent and the task with them; the worktrees stay.
    pub fn cancel(&self) -> Result<()> {
        self.pause()?;
        crate::preview::stop(self.task, "the task was cancelled")?;
        for agent in self
            .task
            .state()?
            .agents
            .iter()
            .filter(|agent| agent.live || agent.unavailable)
        {
            self.stop_agent(&agent.id)?;
        }
        Ok(())
    }
}

/// Provider preference for reviewed workers. Claude and OpenCode use only
/// scoped worker MCP tools; Codex requires the task's explicit opt-in.
impl Team<'_> {
    /// A reviewed team the user put on Codex starts each worker on its
    /// Codex candidates rather than Claude; OpenCode keeps its configured
    /// place, and the others stay, in order, behind them.
    fn prefer_codex(&self, candidates: &mut [crate::roles::Fallback]) -> Result<()> {
        let state = self.task.state()?;
        if state.workflow != crate::execution::Workflow::Reviewed
            || CodexPolicy::of(&state) != CodexPolicy::Team
        {
            return Ok(());
        }
        let mut kinds = std::collections::HashMap::new();
        for candidate in candidates.iter() {
            let kind = self
                .devws
                .agent_kind(&candidate.agent)
                .or_else(|error| Program::parse(&candidate.agent).ok_or(error))?;
            kinds.insert(candidate.agent.clone(), kind);
        }
        // Codex replaces Claude, not OpenCode: OpenCode is first only when the
        // user switched it on for the group, and that choice holds whatever
        // program the leader runs. The sort is stable, so the configured order
        // stays among the rest.
        use crate::providers::ReviewedAdapter;
        candidates.sort_by_key(|candidate| {
            !matches!(
                kinds[&candidate.agent].provider().reviewed_worker(),
                Some(ReviewedAdapter::Codex | ReviewedAdapter::Opencode)
            )
        });
        Ok(())
    }
}

/// What the user let Codex run in a reviewed task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexPolicy {
    Off,
    /// The read-only code reviewers (`devagentics task codex-reviewers`).
    Reviewers,
    /// Every role and the leader (`devagentics task codex-team`): Codex's
    /// writes stay in a worker's snapshot, but its shell reads the host.
    Team,
}

impl CodexPolicy {
    pub fn of(state: &crate::state::State) -> Self {
        if state.codex_team {
            Self::Team
        } else if state.codex_reviewers {
            Self::Reviewers
        } else {
            Self::Off
        }
    }
}

fn reviewed_adapter(
    kind: Program,
    role: &str,
    writes: bool,
    policy: CodexPolicy,
) -> std::result::Result<(), StartupFailure> {
    let refuse = |reason: &str| {
        Err(StartupFailure {
            scope: FailureScope::Model,
            reason: reason.into(),
        })
    };
    // The program's reviewed adapter (its provider's); Codex's needs the
    // user's opt-in.
    use crate::providers::ReviewedAdapter;
    match kind.provider().reviewed_worker() {
        Some(ReviewedAdapter::Claude | ReviewedAdapter::Opencode) => Ok(()),
        Some(ReviewedAdapter::Codex) if policy == CodexPolicy::Team => Ok(()),
        Some(ReviewedAdapter::Codex) if role == "code-reviewer" && !writes => {
            if policy == CodexPolicy::Reviewers {
                Ok(())
            } else {
                refuse(
                    "a Codex code reviewer needs your opt-in (devagentics task codex-reviewers <task> on): its read-only shell can still read host files",
                )
            }
        }
        _ => refuse(
            "reviewed execution requires the verified Claude MCP-only adapter; native Codex tools are not an isolation boundary",
        ),
    }
}

/// What a capacity place is for: writers and read-only agents count apart.
fn kind_of_agents(writes: bool) -> &'static str {
    if writes {
        "writers"
    } else {
        "read-only agents"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Kind as Program;

    #[test]
    fn reviewed_floor_and_senior_workers_use_the_managed_opencode_adapter() {
        for (role, writes) in [
            ("backend-data", true),
            ("planner", false),
            ("code-reviewer", false),
        ] {
            assert!(reviewed_adapter(Program::Opencode, role, writes, CodexPolicy::Off).is_ok());
        }
    }

    #[test]
    fn a_reviewed_team_on_codex_puts_codex_ahead_of_claude_but_not_of_opencode() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&dir);
        let devws = devws.with_kind("codex", Program::Codex);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let candidate = |agent: &str| crate::roles::Fallback {
            agent: agent.into(),
            model: None,
            effort: None,
            approvals: None,
            sandbox: None,
        };
        let mut candidates = vec![
            candidate("opencode"),
            candidate("claude"),
            candidate("codex"),
        ];
        team.prefer_codex(&mut candidates).unwrap();
        assert_eq!(
            candidates[0].agent, "opencode",
            "not unless reviewed and opted in"
        );
        task.transaction(|tx| {
            tx.state.workflow = crate::execution::Workflow::Reviewed;
            tx.state.codex_team = true;
            Ok(())
        })
        .unwrap();
        team.prefer_codex(&mut candidates).unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|c| c.agent.as_str())
                .collect::<Vec<_>>(),
            ["opencode", "codex", "claude"],
            "Codex goes ahead of Claude; OpenCode switched on for the group keeps its place"
        );
    }

    #[test]
    fn a_team_the_user_put_on_codex_runs_every_role_on_it() {
        for (role, writes) in [
            ("backend-data", true),
            ("planner", false),
            ("code-reviewer", false),
        ] {
            assert!(
                reviewed_adapter(Program::Codex, role, writes, CodexPolicy::Team).is_ok(),
                "{role}"
            );
        }
        assert!(reviewed_adapter(Program::Claude, "planner", false, CodexPolicy::Team).is_ok());
        let state = crate::state::State {
            codex_team: true,
            ..Default::default()
        };
        assert_eq!(CodexPolicy::of(&state), CodexPolicy::Team);
        let state = crate::state::State {
            codex_reviewers: true,
            ..Default::default()
        };
        assert_eq!(CodexPolicy::of(&state), CodexPolicy::Reviewers);
        assert_eq!(CodexPolicy::of(&Default::default()), CodexPolicy::Off);
    }

    #[test]
    fn codex_runs_only_an_opted_in_read_only_reviewer() {
        let reviewers = CodexPolicy::Reviewers;
        assert!(reviewed_adapter(Program::Claude, "backend-data", true, CodexPolicy::Off).is_ok());
        assert!(reviewed_adapter(Program::Codex, "code-reviewer", false, reviewers).is_ok());
        let refused = |kind, role, writes, opted_in: bool| {
            let policy = if opted_in {
                reviewers
            } else {
                CodexPolicy::Off
            };
            reviewed_adapter(kind, role, writes, policy).unwrap_err()
        };
        let without_opt_in = refused(Program::Codex, "code-reviewer", false, false);
        assert_eq!(
            without_opt_in.scope,
            FailureScope::Model,
            "falls back to Claude"
        );
        assert!(without_opt_in.reason.contains("codex-reviewers"));
        for (role, writes) in [
            ("backend-data", true),
            ("code-reviewer", true),
            ("planner", false),
        ] {
            assert!(
                refused(Program::Codex, role, writes, true)
                    .reason
                    .contains("verified Claude"),
                "{role}"
            );
        }
    }
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
        start_task(&home, &devws, repo.path(), "/bin/devagentics", None).unwrap();
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
    fn a_stopped_agent_leaves_no_source_behind() {
        let f = fixture();
        let planner = f.team().spawn_agent("planner", None).unwrap();
        f.task
            .transaction(|tx| {
                tx.state.runner = Some(crate::runner::tests::configuration());
                Ok(())
            })
            .unwrap();
        crate::runner::prepare(&f.task, &planner.id, f.repo.path(), true).unwrap();

        f.team().stop_agent(&planner.id).unwrap();

        let state = f.task.state().unwrap();
        assert!(!state.runner_workspaces.contains_key(&planner.id));
        assert!(!state.runner_identities.contains_key(&planner.id));
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

        // Readers have places of their own: idle ones never block a writer.
        std::fs::create_dir_all(f.home.roles_dir()).unwrap();
        std::fs::write(
            f.home.roles_dir().join("reviewer.md"),
            "+++\nagent = \"claude\"\n+++\nReview.\n",
        )
        .unwrap();
        for _ in 0..3 {
            f.team().spawn_agent("reviewer", None).unwrap();
        }
        let error = f
            .team()
            .spawn_agent("reviewer", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("3 read-only agents are running"), "{error}");

        // Stopping an agent frees its place and, for a writer, its worktree.
        f.team().stop_agent("executor-1").unwrap();
        assert!(
            f.team().spawn_agent("executor", None).is_ok(),
            "three readers leave a writer's place free"
        );
        assert!(
            f.team().spawn_agent("reviewer", None).is_err(),
            "the readers' places are still taken"
        );

        // Writers are still capped: three lanes, three writers, no fourth.
        f.team().stop_agent("executor-2").unwrap();
        for lane in ["one", "two", "three", "four"] {
            crate::lanes::add(&f.task, lane, &[format!("{lane}/**")], None).unwrap();
        }
        for lane in ["one", "two", "three"] {
            f.team()
                .spawn_agent_in("executor", None, Some(lane))
                .unwrap();
        }
        let error = f
            .team()
            .spawn_agent_in("executor", None, Some("four"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("3 writers are running"), "{error}");
    }

    #[test]
    fn an_account_near_its_limit_gets_no_new_agents_and_an_unknown_one_does() {
        let f = fixture().approved();
        let devws = FakeDevws::new(&f.home_dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_limits(
                "claude",
                Limits {
                    five_hour: Some(95),
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
            error.contains("95%") || error.contains("used 95"),
            "{error}"
        );
        assert!(
            f.task
                .state()
                .unwrap()
                .agents
                .iter()
                .all(|agent| !agent.live),
            "a refused spawn starts no agent; the reserved one waits for the user's consent"
        );

        // The week first: it says so even when the 5-hour window is full too.
        let week = FakeDevws::new(&f.home_dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_limits(
                "claude",
                Limits {
                    five_hour: Some(97),
                    week: Some(95),
                },
            );
        let error = Team {
            task: &f.task,
            home: &f.home,
            devws: &week,
        }
        .spawn_agent("executor", None)
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("claude has used 95% of its week: no new agents on it until it resets"),
            "{error}"
        );

        let below = FakeDevws::new(&f.home_dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_limits(
                "claude",
                Limits {
                    five_hour: Some(94),
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
    fn usage_consent_is_offered_when_the_only_account_left_on_is_over_its_limit() {
        use crate::state::UsageApproval;
        let f = fixture().approved();
        f.task
            .transaction(|tx| {
                tx.state.disabled_accounts.insert("claude".into());
                Ok(())
            })
            .unwrap();
        f.devws.set_limits(
            "codex",
            Limits {
                week: Some(97),
                five_hour: None,
            },
        );
        let error = f
            .team()
            .spawn_agent("executor", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("approval"), "{error}");
        let state = f.task.state().unwrap();
        assert_eq!(state.usage_limit_approval, UsageApproval::Pending);
        assert_eq!(state.usage_limit_blocks.len(), 1, "codex alone");
        f.team().set_usage_limit_approval(true).unwrap();
        assert!(
            f.team()
                .resume()
                .unwrap()
                .iter()
                .any(|id| id.starts_with("executor")),
            "the consent starts it on codex"
        );
    }

    // The user's consent lets workers past the percentage guard, never onto
    // an account that is used up: Claude Code there answers "You've hit your
    // weekly limit" and the panel closes seconds after its order.
    #[test]
    fn a_used_up_account_is_skipped_even_with_usage_consent() {
        use crate::state::UsageApproval;
        let f = fixture().approved();
        f.task
            .transaction(|tx| {
                tx.state.usage_limit_approval = UsageApproval::Approved;
                Ok(())
            })
            .unwrap();
        f.devws.set_limits(
            "claude",
            Limits {
                week: Some(100),
                five_hour: Some(81),
            },
        );
        f.devws.set_limits(
            "codex",
            Limits {
                week: Some(97),
                five_hour: None,
            },
        );
        let agent = f.team().spawn_agent("executor", None).unwrap();
        assert_eq!(agent.agent, "codex", "consent covers 97%, not 100%");
        f.team().stop_agent(&agent.id).unwrap();

        f.devws.set_limits(
            "codex",
            Limits {
                week: None,
                five_hour: Some(100),
            },
        );
        let error = f
            .team()
            .spawn_agent("executor", None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("claude has used 100% of its week")
                && error.contains("codex has used 100% of its 5-hour limit"),
            "{error}"
        );
        assert!(!error.contains("approval"), "consent cannot help: {error}");
    }

    #[test]
    fn usage_consent_requires_a_request_and_survives_reload_and_revocation() {
        use crate::state::UsageApproval;
        let f = fixture().approved();
        assert!(f.team().set_usage_limit_approval(true).is_err());
        for account in ["claude", "codex"] {
            f.devws.set_limits(
                account,
                Limits {
                    week: Some(95),
                    five_hour: None,
                },
            );
        }
        let error = f
            .team()
            .spawn_agent("executor", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("approval"), "{error}");
        let state = f.task.state().unwrap();
        assert_eq!(state.usage_limit_approval, UsageApproval::Pending);
        assert_eq!(state.usage_limit_blocks.len(), 2);
        let worker_id = state.agents[0].id.clone();
        assert!(!state.agents[0].live);
        f.team().set_usage_limit_approval(false).unwrap();
        assert!(f.team().resume().is_err());
        assert_eq!(
            f.task.state().unwrap().usage_limit_approval,
            UsageApproval::Denied
        );
        f.team().set_usage_limit_approval(true).unwrap();
        assert_eq!(
            f.task.state().unwrap().usage_limit_approval,
            UsageApproval::Approved
        );
        assert_eq!(
            f.team().resume().unwrap().as_slice(),
            std::slice::from_ref(&worker_id)
        );
        assert_eq!(f.task.state().unwrap().agents[0].id, worker_id);
        f.team().stop_agent(&worker_id).unwrap();
        f.team().set_usage_limit_approval(false).unwrap();
        assert!(f.team().spawn_agent("executor", None).is_err());
        assert_eq!(
            f.task.state().unwrap().usage_limit_approval,
            UsageApproval::Denied
        );
        let other = fixture().approved();
        assert_eq!(
            other.task.state().unwrap().usage_limit_approval,
            UsageApproval::NotRequested
        );
    }

    #[test]
    fn a_worker_its_account_refused_restarts_alone_on_the_next_account() {
        let f = fixture().approved();
        let worker = f.team().spawn_agent("executor", None).unwrap();
        assert_eq!(worker.agent, "claude");
        let peer = f.team().spawn_agent("planner", None).unwrap();
        f.devws.set_state(&worker.pane, PaneState::Gone);
        f.devws.set_state(&peer.pane, PaneState::Gone);
        let hold = |tx: &mut crate::task::Tx| {
            tx.state.account_holds.insert(
                "claude".into(),
                crate::state::AccountHold {
                    until: crate::log::now() + 60,
                    reason: "provider startup error: insufficient_quota".into(),
                },
            );
            tx.state.restart_on_next_account.insert(worker.id.clone());
        };
        f.task
            .transaction(|tx| {
                hold(tx);
                tx.state.paused = true;
                Ok(())
            })
            .unwrap();
        assert!(
            !f.team().restart_on_next_account(&worker.id).unwrap(),
            "never while paused"
        );
        f.task
            .transaction(|tx| {
                tx.state.paused = false;
                Ok(())
            })
            .unwrap();
        assert!(f.team().restart_on_next_account(&worker.id).unwrap());
        let state = f.task.state().unwrap();
        let restarted = state.agent(&worker.id).unwrap();
        assert_eq!(restarted.agent, "codex", "the held account is skipped");
        assert!(!restarted.lost && restarted.pane != worker.pane);
        assert!(state.restart_on_next_account.is_empty());
        assert_eq!(
            state.agent(&peer.id).unwrap().pane,
            peer.pane,
            "only the refused worker restarts"
        );
        assert!(!state.paused);
    }

    #[test]
    fn resume_restarts_available_workers_when_idle_pane_less_peers_cannot_launch() {
        let f = fixture().approved();
        let worker = f.team().spawn_agent("executor", None).unwrap();
        f.devws.set_state(&worker.pane, PaneState::Gone);
        f.task
            .transaction(|tx| {
                let original = tx.state.agents[0].clone();
                tx.state.probe(&original.id);
                let claim = tx.state.queue.pop().unwrap();
                tx.state.worker_turns.insert(original.id.clone(), claim);
                for id in ["unavailable-one", "unavailable-two"] {
                    let mut peer = original.clone();
                    peer.id = id.into();
                    peer.role = "missing-role".into();
                    peer.writes = false;
                    peer.live = false;
                    peer.lost = true;
                    peer.unavailable = true;
                    peer.pane.clear();
                    tx.state.agents.insert(0, peer);
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(f.team().resume().unwrap(), [worker.id]);
        let state = f.task.state().unwrap();
        assert!(!state.paused && state.worker_turns.is_empty());
        assert!(
            state
                .queue
                .iter()
                .any(|message| message.text.contains("was interrupted"))
        );
        assert!(
            state.agents[..2]
                .iter()
                .all(|agent| agent.unavailable && !agent.live)
        );
        assert!(
            state
                .queue
                .iter()
                .any(|message| message.to == crate::state::LEADER
                    && message.text.contains("unavailable-one")
                    && message.text.contains("unavailable-two"))
        );
        assert!(f.team().resume().unwrap().is_empty());
        assert_eq!(f.task.state().unwrap().agents[2].pane, state.agents[2].pane);

        f.task
            .transaction(|tx| {
                let peer = tx.state.agents[0].id.clone();
                tx.state.orders.push(crate::state::Order {
                    read: Vec::new(),
                    targets: Vec::new(),
                    id: "pending".into(),
                    step: "step".into(),
                    agent: peer,
                    text: "Pending work".into(),
                    allowed: vec![],
                    forbidden: vec![],
                    check: None,
                    base: "base".into(),
                    status: OrderStatus::Open,
                    delivered: false,
                    stalled: false,
                    plan: None,
                });
                Ok(())
            })
            .unwrap();
        assert!(
            f.team()
                .resume()
                .unwrap_err()
                .to_string()
                .contains("missing-role")
        );
        let state = f.task.state().unwrap();
        assert!(state.paused);
        assert_eq!(state.orders[0].status, OrderStatus::Open);
    }

    #[test]
    fn an_agent_the_usage_guard_held_back_has_no_pane_and_still_stops() {
        let f = fixture().approved();
        for account in ["claude", "codex"] {
            f.devws.set_limits(
                account,
                Limits {
                    week: Some(95),
                    five_hour: None,
                },
            );
        }
        assert!(f.team().spawn_agent("executor", None).is_err());
        let held = f.task.state().unwrap().agents[0].clone();
        assert!(held.unavailable && held.pane.is_empty());
        let before = f.devws.calls().len();
        f.team().stop_agent(&held.id).unwrap();
        assert_eq!(before, f.devws.calls().len(), "no pane access");
        let stopped = f.task.state().unwrap().agents[0].clone();
        assert!(!stopped.live && !stopped.unavailable);
    }

    #[test]
    fn usage_override_keeps_provider_rejections_and_execution_guards() {
        let f = fixture().approved();
        for account in ["claude", "codex"] {
            f.devws.set_limits(
                account,
                Limits {
                    week: Some(95),
                    five_hour: Some(95),
                },
            );
        }
        assert!(f.team().spawn_agent("executor", None).is_err());
        f.team().set_usage_limit_approval(true).unwrap();
        f.devws.fail_launch("claude", FailureScope::Account);
        f.devws.fail_launch("codex", FailureScope::Account);
        assert!(f.team().resume().is_err());
        assert!(!f.task.state().unwrap().agents[0].live);
        f.task
            .transaction(|tx| {
                tx.state.approved = false;
                Ok(())
            })
            .unwrap();
        let error = f.team().resume().unwrap_err().to_string();
        assert!(error.contains("plan is not approved"), "{error}");
    }

    #[test]
    fn an_enabled_account_the_role_does_not_name_is_tried_after_its_own() {
        let mut f = fixture().approved();
        f.devws = f.devws.with_kind("claude_work", Program::Claude);
        f.devws.fail_launch("claude", FailureScope::Account);
        f.devws.fail_launch("codex", FailureScope::Account);
        let agent = f.team().spawn_agent("executor", None).unwrap();
        assert_eq!(agent.agent, "claude_work");
        let state = f.task.state().unwrap();
        assert_eq!(
            state.worker_launches[&agent.id]
                .selected
                .as_ref()
                .unwrap()
                .model,
            crate::roles::floor_model(Program::Claude, None),
            "a floor agent on it runs the floor's model"
        );
    }

    #[test]
    fn an_account_switched_off_is_not_added_to_the_candidates() {
        let mut f = fixture().approved();
        f.devws = f.devws.with_kind("claude_work", Program::Claude);
        crate::accounts::set_enabled(&f.task, &f.devws, "claude_work", false).unwrap();
        f.devws.fail_launch("claude", FailureScope::Account);
        f.devws.fail_launch("codex", FailureScope::Account);
        assert!(f.team().spawn_agent("executor", None).is_err());
    }

    #[test]
    fn an_agent_started_on_a_named_account_runs_there_only() {
        let f = fixture().approved();
        let agent = f
            .team()
            .spawn_agent_on("executor", None, None, Some("codex"))
            .unwrap();
        assert_eq!(agent.agent, "codex", "named over the role's own claude");
        let launch = &f.task.state().unwrap().worker_launches[&agent.id];
        assert_eq!(launch.candidates.len(), 1, "no fallback behind it");
    }

    #[test]
    fn an_agent_started_on_opencode_by_name_runs_there_though_devws_does_not_list_it() {
        let mut f = fixture().approved();
        f.devws = f.devws.with_unlisted_kind("opencode", Program::Opencode);
        OpencodeSettings {
            enabled: true,
            floor: crate::opencode::GroupSettings {
                enabled: true,
                model: Some("deepseek/deepseek-flash".into()),
            },
            ..OpencodeSettings::default()
        }
        .save(&f.home)
        .unwrap();
        let agent = f
            .team()
            .spawn_agent_on("executor", None, None, Some("opencode"))
            .unwrap();
        assert_eq!(agent.agent, "opencode");
    }

    #[test]
    fn a_named_account_that_cannot_start_falls_back_to_none() {
        let f = fixture().approved();
        f.devws.fail_launch("codex", FailureScope::Account);
        let error = f
            .team()
            .spawn_agent_on("executor", None, None, Some("codex"))
            .unwrap_err();
        assert!(format!("{error:#}").contains("codex"), "{error:#}");
        assert!(
            f.task
                .state()
                .unwrap()
                .agents
                .iter()
                .all(|agent| !agent.live),
            "nothing started on claude instead"
        );
    }

    #[test]
    fn a_named_account_must_exist_and_be_on_for_the_group() {
        let f = fixture().approved();
        let error = f
            .team()
            .spawn_agent_on("executor", None, None, Some("nobody"))
            .unwrap_err();
        assert!(error.to_string().contains("no account nobody"), "{error}");
        crate::accounts::set_group_enabled(
            &f.task,
            &f.devws,
            "codex",
            crate::roles::Group::Floor,
            false,
        )
        .unwrap();
        let error = f
            .team()
            .spawn_agent_on("executor", None, None, Some("codex"))
            .unwrap_err();
        assert!(error.to_string().contains("switched off"), "{error}");
    }

    #[test]
    fn an_account_turned_off_for_the_floor_is_skipped_by_floor_agents_only() {
        let f = fixture().approved();
        crate::accounts::set_group_enabled(
            &f.task,
            &f.devws,
            "claude",
            crate::roles::Group::Floor,
            false,
        )
        .unwrap();
        let agent = f.team().spawn_agent("executor", None).unwrap();
        assert_eq!(agent.agent, "codex");
        let failures = &f.task.state().unwrap().worker_launches[&agent.id].failures;
        assert!(
            failures
                .iter()
                .any(|failure| failure.contains("claude is turned off")),
            "{failures:?}"
        );
    }

    #[test]
    fn a_group_model_picked_on_the_board_replaces_the_roles() {
        let f = fixture().approved();
        crate::accounts::set_group_model(
            &f.task,
            &f.devws,
            "claude",
            crate::roles::Group::Floor,
            Some("claude-haiku-5-5".into()),
        )
        .unwrap();
        let agent = f.team().spawn_agent("executor", None).unwrap();
        assert_eq!(agent.agent, "claude");
        let state = f.task.state().unwrap();
        let launch = &state.worker_launches[&agent.id];
        assert_eq!(
            launch.selected.as_ref().unwrap().model.as_deref(),
            Some("claude-haiku-5-5")
        );
        let args = std::fs::read_to_string(
            f.task
                .dir()
                .join("agents")
                .join(format!("{}.args", agent.id)),
        )
        .unwrap();
        assert!(
            args.contains("claude-haiku-5-5"),
            "it starts on that model: {args:?}"
        );
    }

    #[test]
    fn exhausted_claude_starts_codex_with_the_same_worker_scope() {
        let f = fixture().approved();
        f.devws.set_limits(
            "claude",
            Limits {
                week: Some(100),
                five_hour: Some(100),
            },
        );
        let agent = f.team().spawn_agent("executor", None).unwrap();
        assert_eq!(agent.agent, "codex");
        assert!(agent.writes);
        assert_eq!(agent.role, "executor");
        let state = f.task.state().unwrap();
        let launch = &state.worker_launches[&agent.id];
        assert_eq!(
            launch.selected.as_ref().unwrap().model.as_deref(),
            Some(crate::providers::FLOOR_CODEX_MODEL),
            "the floor's Codex is Luna"
        );
        assert!(launch.failures[0].contains("week"));
        let args = std::fs::read(f.task.dir().join("agents/executor-1.args")).unwrap();
        let args = String::from_utf8_lossy(&args);
        assert!(
            args.contains("factory floor") && args.contains(crate::providers::FLOOR_CODEX_MODEL)
        );
        assert!(!args.contains("acceptEdits"));
        assert!(state.permissions.is_empty());
    }

    #[test]
    fn no_available_candidate_leaves_no_live_worker() {
        let f = fixture().approved();
        for provider in ["claude", "codex"] {
            f.devws.set_limits(
                provider,
                Limits {
                    week: Some(100),
                    five_hour: None,
                },
            );
        }
        let error = f
            .team()
            .spawn_agent("executor", None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("claude") && error.contains("codex"),
            "{error}"
        );
        assert!(
            f.task
                .state()
                .unwrap()
                .agents
                .iter()
                .all(|agent| !agent.live)
        );
        assert!(
            !f.devws
                .calls()
                .iter()
                .any(|call| call.starts_with("spawn "))
        );
    }

    #[test]
    fn explicit_launch_rejection_falls_back_but_unrelated_errors_do_not() {
        let f = fixture().approved();
        f.devws.fail_launch("claude", FailureScope::Account);
        assert_eq!(
            f.team().spawn_agent("executor", None).unwrap().agent,
            "codex"
        );
        let f = fixture().approved();
        f.devws.fail_launch("claude", FailureScope::Other);
        assert!(f.team().spawn_agent("executor", None).is_err());
        assert!(f.task.state().unwrap().agents.is_empty());
        assert!(!f.devws.calls().iter().any(|call| call.contains(" codex ")));
    }

    #[test]
    fn explicit_resume_uses_corrected_provider_settings_without_widening_scope() {
        let f = fixture().approved();
        let agent = f.team().spawn_agent("executor", None).unwrap();
        f.devws.set_state(&agent.pane, PaneState::Gone);
        std::fs::write(f.home.roles_dir().join("executor.md"), "+++\nagent = \"codex\"\nmodel = \"gpt-6-luna-corrected\"\nwrites = true\ngroup = \"floor\"\n+++\nNew instructions must not replace snapshotted scope.\n").unwrap();
        f.team().resume().unwrap();
        let state = f.task.state().unwrap();
        assert_eq!(state.agents[0].agent, "codex");
        let launch = &state.worker_launches[&agent.id];
        assert_eq!(
            launch.selected.as_ref().unwrap().model.as_deref(),
            Some("gpt-6-luna-corrected")
        );
        assert!(launch.role.instructions.contains("factory floor"));
        assert!(state.agents[0].writes);
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
                    spec: None,
                    review_since: None,
                });
                tx.state.orders.push(crate::state::Order {
                    read: Vec::new(),
                    targets: Vec::new(),
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
                    response_probe: false,
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
    fn interrupt_preserves_panes_and_busy_recovery_keeps_the_task_paused() {
        let f = fixture().approved();
        let agent = f.team().spawn_agent("executor", None).unwrap();
        f.devws.set_state(&agent.pane, PaneState::Working);
        f.team().pause_mode(true, None).unwrap();
        let state = f.task.state().unwrap();
        assert!(state.paused && state.recovery[&agent.id].interrupted);
        assert_eq!(state.agents[0].pane, agent.pane);
        f.devws.set_state(&agent.pane, PaneState::Working);
        assert!(f.team().resume().is_err());
        assert!(f.task.state().unwrap().paused);
        f.devws.set_state(&agent.pane, PaneState::Done);
        f.team().resume().unwrap();
        f.team().resume().unwrap();
        let state = f.task.state().unwrap();
        assert!(!state.paused && state.recovery.is_empty());
        assert!(
            state.queue.is_empty(),
            "no invented assignment for an idle agent"
        );
        assert_eq!(state.agents[0].pane, agent.pane);
    }

    #[test]
    fn partial_interrupt_failure_preserves_successful_recovery_and_pause() {
        let f = fixture().approved();
        let first = f.team().spawn_agent("planner", None).unwrap();
        let second = f.team().spawn_agent("executor", None).unwrap();
        for pane in [&first.pane, &second.pane] {
            f.devws.set_state(pane, PaneState::Working);
        }
        f.devws.fail_interrupt(&second.pane);
        assert!(f.team().pause_mode(true, None).is_err());
        let state = f.task.state().unwrap();
        assert!(state.paused);
        assert!(state.recovery[&first.id].interrupted);
        assert!(state.recovery[&second.id].error.is_some());
        assert!(f.team().resume().is_err());
        assert!(f.task.state().unwrap().paused);
        f.devws.set_state(&second.pane, PaneState::Done);
        f.team().resume().unwrap();
        assert!(!f.task.state().unwrap().paused);
    }

    #[test]
    fn interrupt_exempts_the_invoking_pane() {
        let f = fixture().approved();
        let agent = f.team().spawn_agent("executor", None).unwrap();
        f.devws.set_state(&agent.pane, PaneState::Working);
        f.team().pause_mode(true, Some(&agent.pane)).unwrap();
        assert!(f.task.state().unwrap().recovery.is_empty());
        assert_eq!(f.devws.pane_state(&agent.pane).unwrap(), PaneState::Working);
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
                    spec: None,
                    review_since: None,
                });
                tx.state.orders.push(crate::state::Order {
                    read: Vec::new(),
                    targets: Vec::new(),
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
    fn a_restarted_leader_in_a_new_pane_can_be_resumed() {
        let f = fixture().approved();
        let workspace = f.task.config().unwrap().workspace;
        f.devws.set_role(&workspace, "agent", "%2");
        // It escalated, and waits for the user: the resume is their answer.
        f.devws.set_state("%2", PaneState::Waiting);
        f.devws.set_state("%98", PaneState::Gone);
        f.task
            .transaction(|tx| {
                // Its old pane was interrupted, then closed with the workspace.
                tx.state.recovery.insert(
                    crate::state::LEADER.into(),
                    crate::state::Recovery {
                        pane: "%98".into(),
                        interrupted: false,
                        error: Some("interrupt was not acknowledged by %98".into()),
                    },
                );
                tx.state.paused = true;
                tx.state.leader_wait = crate::state::LeaderWait::Shown;
                Ok(())
            })
            .unwrap();

        f.team().resume().unwrap();
        let state = f.task.state().unwrap();
        assert!(!state.paused);
        assert_eq!(state.leader_wait, crate::state::LeaderWait::No);
        assert!(state.queue.iter().any(
            |message| message.to == crate::state::LEADER && message.text.starts_with("RESUME")
        ));
    }

    #[test]
    fn the_leader_resuming_from_its_own_turn_is_ready() {
        let f = fixture().approved();
        let workspace = f.task.config().unwrap().workspace;
        f.devws.set_role(&workspace, "agent", "%2");
        f.devws.set_state("%2", PaneState::Working);
        f.task
            .transaction(|tx| {
                tx.state.recovery.insert(
                    crate::state::LEADER.into(),
                    crate::state::Recovery {
                        pane: "%98".into(),
                        interrupted: false,
                        error: None,
                    },
                );
                tx.state.paused = true;
                Ok(())
            })
            .unwrap();
        assert!(
            f.team().resume().is_err(),
            "someone else resuming waits for its turn to end"
        );
        f.team().resume_from(Some("%2")).unwrap();
        assert!(!f.task.state().unwrap().paused);
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
