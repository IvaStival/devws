//! The dispatcher, and the log pane of the task's workspace
//! (`devagentics watch <task>`). It types what is waiting into the agents'
//! panels when they can take it, notices a panel that went or an agent that
//! stopped without reporting, and shows the log as it grows.

use crate::devws::{Delivery, Devws, PaneState};
use crate::log::Kind;
use crate::state::{Agent, LEADER, LeaderWait, OrderStatus, StepRetry, StepStatus};
use crate::task::Task;
use crate::team::Team;
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::{Duration, Instant};

/// How long an agent may sit done, with an order and no report, before the
/// leader is told.
pub const STALL_GRACE: Duration = Duration::from_secs(120);

/// How many log lines the pane starts with.
const BACKLOG: usize = 20;

/// How long a step that used its attempts waits for its one retry: long
/// enough for a passing problem (a limit, a service, a fix being installed)
/// to clear.
pub const RETRY_AFTER_SECS: u64 = 15 * 60;

/// What wakes a reviewed leader when its inbox has news.
pub const LEADER_NUDGE: &str = "Check your inbox and continue.";

pub struct Dispatcher<'a> {
    task: &'a Task,
    home: &'a crate::home::Home,
    devws: &'a dyn Devws,
    grace: Duration,
    /// When each agent was first seen done with an open order.
    done_since: HashMap<String, Instant>,
    /// The team's usage, read from its panes no more than once a minute.
    usage: crate::board_usage::UsageReader,
    budget_checked: Option<Instant>,
    /// When each agent was first seen with nothing open (`idle.rs`). Kept
    /// here only: after a restart an agent gets its full time again.
    idle_since: HashMap<String, Instant>,
}

/// How often the budget is checked: usage changes in minutes, not seconds.
const BUDGET_EVERY: Duration = Duration::from_secs(30);

/// Where the team stands against its budget.
#[derive(Debug, PartialEq)]
enum Spend {
    Within,
    /// 80% or more of a limit: what to tell the leader.
    Near(String),
    /// A limit reached: why the task pauses.
    Reached(String),
}

fn spend(budget: &crate::task::Budget, tokens: Option<u64>, hours: f64) -> Spend {
    let mut near = None;
    let mut measures = Vec::new();
    if let (Some(limit), Some(tokens)) = (budget.max_tokens, tokens) {
        measures.push((
            tokens as f64,
            limit as f64,
            format!("{tokens} of {limit} tokens"),
        ));
    }
    if let Some(limit) = budget.max_hours {
        measures.push((hours, limit, format!("{hours:.1} of {limit} hours")));
    }
    for (used, limit, text) in measures {
        if used >= limit {
            return Spend::Reached(text);
        }
        if used >= 0.8 * limit {
            near.get_or_insert(text);
        }
    }
    near.map_or(Spend::Within, Spend::Near)
}

impl<'a> Dispatcher<'a> {
    pub fn new(
        task: &'a Task,
        devws: &'a dyn Devws,
        home: &'a crate::home::Home,
        grace: Duration,
    ) -> Self {
        Self {
            task,
            home,
            devws,
            grace,
            done_since: HashMap::new(),
            usage: crate::board_usage::UsageReader::new(Duration::from_secs(60)),
            budget_checked: None,
            idle_since: HashMap::new(),
        }
    }

    /// One pass over the team.
    pub fn tick(&mut self) -> Result<()> {
        let _control = self.task.control_lock()?;
        let state = self.task.state()?;
        for agent in state
            .agents
            .iter()
            .filter(|agent| agent.live && !agent.lost)
        {
            let pane_state = self.devws.pane_state(&agent.pane)?;
            if pane_state != PaneState::Gone
                && !state.paused
                && !agent.ready
                && let Some(failure) = self.devws.startup_failure(&agent.pane)?
            {
                Team {
                    task: self.task,
                    home: self.home,
                    devws: self.devws,
                }
                .retry_startup(agent, &failure)?;
                continue;
            }
            if pane_state == PaneState::Gone {
                self.lose(agent)?;
                continue;
            }
            if !state.paused && !state.recovery.contains_key(&agent.id) {
                self.watch_for_a_stall(agent, pane_state)?;
            }

            // The first thing it did was answer its role: now it takes orders.
            let ready =
                agent.ready || (pane_state == PaneState::Done && self.make_ready(&agent.id)?);
            if !ready || state.paused || !matches!(pane_state, PaneState::Done | PaneState::Idle) {
                continue;
            }
            if state.workflow == crate::execution::Workflow::Reviewed {
                continue;
            }
            if let Some(message) = state.queue.iter().find(|message| message.to == agent.id) {
                match self.devws.send(&agent.pane, &message.text)? {
                    Delivery::Delivered => {
                        self.delivered(&[message.id], Some(&agent.id), message.order.as_deref())?
                    }
                    Delivery::Busy(_) => {}
                    Delivery::Gone => self.lose(agent)?,
                }
            }
        }
        self.keep_the_leader_waiting()?;
        if !state.paused {
            self.retry_spent_steps(crate::log::now())?;
            self.remind_left_reviews(crate::log::now())?;
            self.keep_within_budget(crate::log::now())?;
            self.close_idle_agents(&state, Instant::now())?;
        }
        // The budget may have paused the task in this pass.
        if !self.task.state()?.paused {
            match state.workflow {
                crate::execution::Workflow::Legacy => self.tell_the_leader()?,
                crate::execution::Workflow::Reviewed => self.nudge_the_leader()?,
            }
        }
        Ok(())
    }

    /// Closes each agent that has had nothing open for longer than it may
    /// (`idle.rs`), after a handoff note in the team's memory. The leader sees
    /// it in its roster and the log; nothing wakes it.
    fn close_idle_agents(&mut self, state: &crate::state::State, now: Instant) -> Result<()> {
        let minutes = self.task.config()?.idle_close_minutes;
        let live: Vec<&crate::state::Agent> = state
            .agents
            .iter()
            .filter(|agent| agent.live && !agent.lost && agent.ready)
            .collect();
        self.idle_since
            .retain(|id, _| live.iter().any(|agent| &agent.id == id));
        for agent in live {
            if !crate::idle::has_nothing_open(state, agent) {
                self.idle_since.remove(&agent.id);
                continue;
            }
            let Some(allowed) = crate::idle::allowed_idle(agent, minutes) else {
                continue;
            };
            let since = *self.idle_since.entry(agent.id.clone()).or_insert(now);
            let idle = now.duration_since(since);
            if idle < allowed {
                continue;
            }
            let report = self.last_report(&agent.id)?;
            let note = crate::idle::handoff(state, agent, idle, report.as_deref());
            // The note helps the next agent; a memory that cannot take it
            // does not keep an idle agent open.
            let saved = crate::notes::save(self.task, self.devws, &note).unwrap_or_else(|error| {
                eprintln!("handoff note for {}: {error:#}", agent.id);
                None
            });
            Team {
                task: self.task,
                home: self.home,
                devws: self.devws,
            }
            .stop_agent(&agent.id)?;
            self.task.transaction(|tx| {
                tx.log(
                    Kind::Stop,
                    Some(&agent.id),
                    None,
                    &format!(
                        "closed after {} minutes with nothing open{}",
                        idle.as_secs() / 60,
                        if saved.is_some() {
                            "; its handoff note is in the team's memory"
                        } else {
                            ""
                        }
                    ),
                );
                Ok(())
            })?;
            self.idle_since.remove(&agent.id);
        }
        Ok(())
    }

    /// The summary of the agent's latest report, from the log.
    fn last_report(&self, agent: &str) -> Result<Option<String>> {
        Ok(self.task.log_lines()?.iter().rev().find_map(|line| {
            let entry: serde_json::Value = serde_json::from_str(line).ok()?;
            (entry["kind"] == "report" && entry["agent"] == agent)
                .then(|| entry["text"].as_str().map(str::to_string))
                .flatten()
        }))
    }

    /// Tells the leader once at 80% of a limit, and pauses the task at the
    /// limit for the user to decide. A log line names the limit it was for,
    /// so a raised limit warns again.
    fn keep_within_budget(&mut self, now: u64) -> Result<()> {
        let budget = self.task.config()?.budget;
        if budget.is_unlimited()
            || self
                .budget_checked
                .is_some_and(|checked| checked.elapsed() < BUDGET_EVERY)
        {
            return Ok(());
        }
        self.budget_checked = Some(Instant::now());
        let tokens = budget
            .max_tokens
            // Usage that cannot be read this time counts as unknown: a
            // pane's hiccup must not stop the dispatcher's pass.
            .and_then(|_| self.usage.of_team(self.task, self.devws).ok())
            .map(|team| {
                let usage = crate::board_usage::total(&team.by_role);
                usage.input + usage.output + usage.cache
            });
        let log: Vec<crate::log::Entry> = self
            .task
            .log_lines()?
            .iter()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        let started = log.iter().map(|entry| entry.at).min().unwrap_or(now);
        let hours = now.saturating_sub(started) as f64 / 3600.0;
        let limit = format!(
            "(limit: {} tokens, {} hours)",
            budget
                .max_tokens
                .map_or("no".into(), |tokens| tokens.to_string()),
            budget
                .max_hours
                .map_or("no".into(), |hours| hours.to_string())
        );
        match spend(&budget, tokens, hours) {
            Spend::Within => Ok(()),
            Spend::Near(used) => {
                let said = log.iter().any(|entry| {
                    entry.kind == Kind::Budget
                        && entry.text.starts_with("80%")
                        && entry.text.ends_with(&limit)
                });
                if said {
                    return Ok(());
                }
                self.task.transaction(|tx| {
                    tx.state.say(
                        LEADER,
                        format!(
                            "BUDGET: 80% used ({used}). Finish what is open, start nothing that is not needed, and tell the user what is left."
                        ),
                    );
                    tx.log(Kind::Budget, None, None, &format!("80% used: {used} {limit}"));
                    Ok(())
                })
            }
            Spend::Reached(used) => {
                let config = self.task.config()?;
                self.task.transaction(|tx| {
                    let accepted = tx
                        .state
                        .steps
                        .iter()
                        .filter(|step| step.status == StepStatus::Accepted)
                        .count();
                    tx.state.paused = true;
                    tx.log(Kind::Budget, None, None, &format!("reached: {used} {limit}"));
                    tx.log(Kind::Pause, None, None, "paused: the budget is reached");
                    tx.log(
                        Kind::Escalate,
                        None,
                        None,
                        &format!(
                            "The budget is reached ({used}); the task is paused and nothing is lost. {accepted} of {} steps are accepted (devagentics task status {}). To go on, raise [budget] in task.toml and resume (devagentics task resume {}); or stop.",
                            tx.state.steps.len(),
                            config.id,
                            config.id
                        ),
                    );
                    Ok(())
                })?;
                if let Some(pane) = self.devws.pane_with_role(&config.workspace, "agent")? {
                    self.devws.mark(&pane, "waiting")?;
                }
                Ok(())
            }
        }
    }

    /// A reviewed leader fetches its own inbox, so
    /// nothing would start its next turn: when it is not busy and a message
    /// arrived since the last nudge, a fixed line wakes it. No message text
    /// is typed in.
    fn nudge_the_leader(&self) -> Result<()> {
        let state = self.task.state()?;
        let Some(newest) = state
            .queue
            .iter()
            .filter(|message| message.to == LEADER)
            .map(|message| message.id)
            .max()
        else {
            return Ok(());
        };
        // Waiting for the user: their answer starts its next turn.
        if newest <= state.leader_nudged || state.leader_wait != LeaderWait::No {
            return Ok(());
        }
        // Waiting here is only an answered escalation's mark (leader_wait is
        // No): the leader sits at its prompt.
        let Some(pane) = self.idle_leader_pane(true)? else {
            return Ok(());
        };
        // devws types into no pane that waits for its user: the mark goes first.
        if self.devws.pane_state(&pane)? == PaneState::Waiting {
            self.devws.mark(&pane, "done")?;
        }
        if self.devws.send(&pane, LEADER_NUDGE)? == Delivery::Delivered {
            self.task.transaction(|tx| {
                tx.state.recovery.remove(LEADER);
                tx.state.leader_nudged = newest;
                tx.log(
                    Kind::Delivered,
                    Some(LEADER),
                    None,
                    "nudged to read its inbox",
                );
                Ok(())
            })?;
        }
        Ok(())
    }

    /// A step that ended badly with every attempt used gets one more after
    /// `RETRY_AFTER_SECS`; when that one ends badly too, it is exhausted.
    /// The leader hears each move once.
    pub(crate) fn retry_spent_steps(&self, now: u64) -> Result<()> {
        let max_attempts = self.task.config()?.max_attempts;
        self.task.transaction(|tx| {
            for index in 0..tx.state.steps.len() {
                let step = &tx.state.steps[index];
                let id = step.id.clone();
                // Only failures of the work use attempts (failures.rs).
                let attempts = crate::failures::work_failures(&tx.state, &id);
                let ended_badly = matches!(
                    step.status,
                    StepStatus::Blocked | StepStatus::Failed | StepStatus::Rejected
                );
                let spent = attempts
                    >= crate::orders::attempt_limit(&tx.state, &id, max_attempts);
                if !(ended_badly && spent) {
                    // A wait for a step that is no longer spent ends.
                    if matches!(tx.state.step_retries.get(&id), Some(StepRetry::Waiting { .. })) {
                        tx.state.step_retries.remove(&id);
                    }
                    continue;
                }
                match tx.state.step_retries.get(&id).copied() {
                    None => {
                        let at = now + RETRY_AFTER_SECS;
                        tx.state.step_retries.insert(id.clone(), StepRetry::Waiting { at });
                        tx.state.say(LEADER, format!(
                            "Step {id} used its {attempts} attempts. devAgentics retries it once at {}: then send its order again.",
                            clock(at)
                        ));
                    }
                    Some(StepRetry::Waiting { at }) if now >= at => {
                        tx.state.attempt_limits.insert(id.clone(), attempts + 1);
                        tx.state.step_retries.insert(id.clone(), StepRetry::Granted);
                        tx.state.say(LEADER, format!(
                            "Step {id} has one more attempt now: send its order again."
                        ));
                        tx.log(Kind::Review, None, Some(&id), "devAgentics allowed one more attempt after a wait");
                    }
                    Some(StepRetry::Granted) => {
                        tx.state.steps[index].set_status(StepStatus::Exhausted, now);
                        tx.state.say(LEADER, format!(
                            "Step {id} failed after {attempts} attempts and a retry. When the next attempt can succeed (the cause is understood, or was devAgentics or the environment), give it more with allow_attempts and say why; when it keeps failing the same way, change the approach or close it."
                        ));
                        tx.log(Kind::Review, None, Some(&id), "failed: attempts and retry used");
                    }
                    Some(StepRetry::Waiting { .. }) => {}
                }
            }
            Ok(())
        })
    }

    /// Steps that waited for the leader longer than `STALE_REVIEW_SECS` (or
    /// since an unknown time) look like open problems on the board: the
    /// leader hears them in one message, each once per wait, and settles
    /// them.
    pub(crate) fn remind_left_reviews(&self, now: u64) -> Result<()> {
        self.task.transaction(|tx| {
            let left: Vec<(String, u64)> = tx
                .state
                .steps
                .iter()
                .filter(|step| step.status.waits_for_leader())
                .map(|step| (step.id.clone(), step.review_since.unwrap_or(0)))
                .filter(|(_, since)| now.saturating_sub(*since) > crate::board::STALE_REVIEW_SECS)
                .filter(|(id, since)| tx.state.reminded_reviews.get(id) != Some(since))
                .collect();
            if left.is_empty() {
                return Ok(());
            }
            let ids: Vec<&str> = left.iter().map(|(id, _)| id.as_str()).collect();
            tx.state.say(
                LEADER,
                format!(
                    "These steps have been waiting for you for a while, and the user sees them as open problems: {}. Settle each now: accept_step what you keep, reject_step what must be done again, close_step a read-only step whose report you used or that nothing needs any more. A blocked step you cannot settle: say what it waits for.",
                    ids.join(", ")
                ),
            );
            tx.log(
                Kind::Review,
                Some(LEADER),
                None,
                &format!("reminded of steps left waiting: {}", ids.join(", ")),
            );
            tx.state.reminded_reviews.extend(left);
            Ok(())
        })
    }

    /// The leader's own hooks mark it done when the turn that escalated ends:
    /// it is marked as needing the user again, until the user's answer
    /// starts its next turn.
    fn keep_the_leader_waiting(&self) -> Result<()> {
        let wait = self.task.state()?.leader_wait;
        if wait == LeaderWait::No {
            return Ok(());
        }
        let Some(pane) = self
            .devws
            .pane_with_role(&self.task.config()?.workspace, "agent")?
        else {
            return Ok(());
        };
        let next = match (wait, self.devws.pane_state(&pane)?) {
            (_, PaneState::Done | PaneState::Idle) => {
                self.devws.mark(&pane, "waiting")?;
                LeaderWait::Shown
            }
            (LeaderWait::Shown, PaneState::Working) => LeaderWait::No,
            _ => return Ok(()),
        };
        self.task.transaction(|tx| {
            tx.state.leader_wait = next;
            Ok(())
        })
    }

    /// The leader's pane, when its turn is over (or, `waiting_is_idle`, when it
    /// is marked as waiting for the user).
    fn idle_leader_pane(&self, waiting_is_idle: bool) -> Result<Option<String>> {
        let Some(pane) = self
            .devws
            .pane_with_role(&self.task.config()?.workspace, "agent")?
        else {
            return Ok(None);
        };
        let is_idle = match self.devws.pane_state(&pane)? {
            PaneState::Done | PaneState::Idle => true,
            PaneState::Waiting => waiting_is_idle,
            _ => false,
        };
        Ok(is_idle.then_some(pane))
    }

    fn make_ready(&self, id: &str) -> Result<bool> {
        self.task.transaction(|tx| {
            if let Some(agent) = tx.state.agents.iter_mut().find(|agent| agent.id == id) {
                agent.ready = true;
            }
            Ok(true)
        })
    }

    /// What the leader has been told, as one message: it is a conversation, and
    /// a turn for each report would be a waste.
    fn tell_the_leader(&self) -> Result<()> {
        let state = self.task.state()?;
        let waiting: Vec<_> = state
            .queue
            .iter()
            .filter(|message| message.to == LEADER)
            .collect();
        if waiting.is_empty() {
            return Ok(());
        }
        let Some(pane) = self.idle_leader_pane(false)? else {
            return Ok(());
        };
        let text = waiting
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        if self.devws.send(&pane, &text)? == Delivery::Delivered {
            let ids: Vec<_> = waiting.iter().map(|message| message.id).collect();
            self.delivered(&ids, None, None)?;
        }
        Ok(())
    }

    fn delivered(&self, ids: &[u64], agent: Option<&str>, order: Option<&str>) -> Result<()> {
        self.task.transaction(|tx| {
            let leader_delivered = tx
                .state
                .queue
                .iter()
                .any(|message| ids.contains(&message.id) && message.to == LEADER);
            tx.state.queue.retain(|message| !ids.contains(&message.id));
            if leader_delivered {
                tx.state.recovery.remove(LEADER);
            }
            if let Some(order) = order.and_then(|id| {
                tx.state
                    .orders
                    .iter_mut()
                    .find(|candidate| candidate.id == id)
            }) {
                order.delivered = true;
                let agent = order.agent.clone();
                tx.state.recovery.remove(&agent);
            }
            tx.log(
                Kind::Delivered,
                agent,
                None,
                &format!("{} message(s) typed in", ids.len()),
            );
            Ok(())
        })
    }

    /// The panel went: a worker its account refused before it did anything
    /// starts again on its next account; any other, the leader is told once.
    fn lose(&self, agent: &Agent) -> Result<()> {
        if self
            .task
            .state()?
            .restart_on_next_account
            .contains(&agent.id)
        {
            let team = Team {
                task: self.task,
                home: self.home,
                devws: self.devws,
            };
            if team.restart_on_next_account(&agent.id)? {
                return Ok(());
            }
        }
        let error = crate::worker_tools::last_error(self.task, &agent.id)
            .map(|error| format!(" Its last turn failed: {error}"))
            .unwrap_or_default();
        self.task.transaction(|tx| {
            let Some(lost) = tx
                .state
                .agents
                .iter_mut()
                .find(|candidate| candidate.id == agent.id)
            else {
                return Ok(());
            };
            if lost.lost {
                return Ok(());
            }
            lost.lost = true;
            let order = tx
                .state
                .open_order_of(&agent.id)
                .map(|order| format!(" It had order {} (step {}).", order.id, order.step))
                .unwrap_or_default();
            tx.state.say(
                LEADER,
                format!(
                    "{}'s panel is gone.{error}{order} `devagentics task resume` starts it again.",
                    agent.id
                ),
            );
            tx.log(
                Kind::Lost,
                Some(&agent.id),
                None,
                &format!("its panel is gone.{error}"),
            );
            Ok(())
        })
    }

    /// A turn that ended with an order open and no report: after a grace
    /// period, the leader is told once. An agent waiting for the answer to
    /// its question is not stalled.
    fn watch_for_a_stall(&mut self, agent: &Agent, pane_state: PaneState) -> Result<()> {
        let state = self.task.state()?;
        let idle_with_order = pane_state == PaneState::Done
            && state.open_question_of(&agent.id).is_none()
            && state
                .open_order_of(&agent.id)
                .is_some_and(|order| order.delivered && !order.stalled);
        if !idle_with_order {
            self.done_since.remove(&agent.id);
            return Ok(());
        }
        let since = *self
            .done_since
            .entry(agent.id.clone())
            .or_insert_with(Instant::now);
        if since.elapsed() < self.grace {
            return Ok(());
        }
        self.task.transaction(|tx| {
            let Some(order) = tx.state.orders.iter_mut().find(|order| {
                order.agent == agent.id && order.status == OrderStatus::Open && !order.stalled
            }) else {
                return Ok(());
            };
            order.stalled = true;
            let (id, step) = (order.id.clone(), order.step.clone());
            tx.state.say(
                LEADER,
                format!(
                    "{} stopped without reporting on order {id} (step {step}): ask it, or stop it.",
                    agent.id
                ),
            );
            tx.log(
                Kind::Lost,
                Some(&agent.id),
                Some(&step),
                "stopped without reporting",
            );
            Ok(())
        })
    }

    /// Runs until `max_ticks` (None: for good), printing the log as it grows.
    pub fn run(
        &mut self,
        output: &mut dyn Write,
        interval: Duration,
        max_ticks: Option<u32>,
    ) -> Result<()> {
        let _lock = lock(self.task)?;
        crate::preview::expire(self.task)?;
        crate::runner::prune_at_start(self.task)?;
        let lines = self.task.log_lines()?;
        let mut printed = lines.len().saturating_sub(BACKLOG);
        let mut ticks = 0;
        loop {
            if let Err(error) = self.tick() {
                writeln!(output, "watch: {error:#}")?;
            }
            let lines = self.task.log_lines()?;
            for line in &lines[printed.min(lines.len())..] {
                writeln!(output, "{}", format_line(line))?;
            }
            printed = lines.len();
            output.flush()?;
            ticks += 1;
            if max_ticks.is_some_and(|max| ticks >= max) {
                return Ok(());
            }
            std::thread::sleep(interval);
        }
    }
}

/// One dispatcher per task: held while it runs.
#[derive(Debug)]
pub struct WatchLock {
    _file: File,
}

pub fn lock(task: &Task) -> Result<WatchLock> {
    let file = File::create(task.dir().join("watch.lock")).context("opening the watch lock")?;
    if file.try_lock_exclusive().is_err() {
        bail!("a dispatcher is already running for this task");
    }
    Ok(WatchLock { _file: file })
}

/// A log line for the pane: "14:03:22 order     backend-1 s2  Write src/…".
/// Epoch seconds as local time, in `format`.
fn local_time(seconds: i64, format: &str) -> Option<String> {
    chrono::DateTime::from_timestamp(seconds, 0).map(|time| {
        time.with_timezone(&chrono::Local)
            .format(format)
            .to_string()
    })
}

/// When a retry is due, for the leader: `14:05`.
fn clock(at: u64) -> String {
    local_time(at as i64, "%H:%M").unwrap_or_else(|| format!("{at}"))
}

pub fn format_line(line: &str) -> String {
    let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
        return line.to_string();
    };
    let at = entry["at"]
        .as_i64()
        .and_then(|seconds| local_time(seconds, "%H:%M:%S"))
        .unwrap_or_else(|| "--:--:--".to_string());
    let text = entry["text"]
        .as_str()
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("");
    let text: String = text.chars().take(140).collect();
    let step = entry["step"]
        .as_str()
        .map(|step| format!(" {step}"))
        .unwrap_or_default();
    format!(
        "{at} {:<9} {}{step}  {text}",
        entry["kind"].as_str().unwrap_or("?"),
        entry["agent"].as_str().unwrap_or("-")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devws::fake::FakeDevws;
    use crate::home::Home;
    use crate::providers::Kind as Program;
    use crate::start::start_task;
    use crate::team::Team;
    use tempfile::TempDir;

    struct Fixture {
        _dir: TempDir,
        home: Home,
        task: Task,
        devws: FakeDevws,
        _repo: TempDir,
    }

    /// A started and approved task, its leader's pane %1 (role agent).
    fn fixture() -> Fixture {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_kind("codex", Program::Codex);
        start_task(&home, &devws, repo.path(), "d", None).unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        devws.set_role(&task.config().unwrap().workspace, "agent", "%1");
        devws.set_state("%1", PaneState::Done);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        team.write_plan("1. go\n").unwrap();
        team.approve().unwrap();
        Fixture {
            _dir: dir,
            home,
            task,
            devws,
            _repo: repo,
        }
    }

    impl Fixture {
        fn spawn(&self, role: &str) -> Agent {
            Team {
                task: &self.task,
                home: &self.home,
                devws: &self.devws,
            }
            .spawn_agent(role, None)
            .unwrap()
        }

        fn queue(&self, to: &str, text: &str) {
            self.task
                .transaction(|tx| {
                    tx.state.say(to, text.to_string());
                    Ok(())
                })
                .unwrap();
        }

        fn dispatcher(&self) -> Dispatcher<'_> {
            Dispatcher::new(&self.task, &self.devws, &self.home, Duration::ZERO)
        }

        fn sent_to(&self, pane: &str) -> Vec<String> {
            self.devws
                .sent()
                .into_iter()
                .filter(|(to, _)| to == pane)
                .map(|(_, text)| text)
                .collect()
        }
    }

    // An agent with nothing open for its whole time is closed, its handoff
    // note saved; a writer whose worktree holds unmerged work stays.
    #[test]
    fn an_agent_with_nothing_open_is_closed_after_its_time_with_a_note() {
        let f = fixture();
        f.devws.memory_on();
        let planner = f.spawn("planner");
        let hammer = f.spawn("executor");
        f.task
            .transaction(|tx| {
                for agent in &mut tx.state.agents {
                    agent.ready = true;
                }
                tx.state.orders.push(crate::state::Order {
                    id: "o9".into(),
                    step: "G1".into(),
                    agent: hammer.id.clone(),
                    text: String::new(),
                    allowed: vec![],
                    forbidden: vec![],
                    check: None,
                    base: String::new(),
                    status: crate::state::OrderStatus::Closed,
                    delivered: true,
                    stalled: false,
                    plan: None,
                    read: vec![],
                    targets: vec![],
                });
                tx.state.steps.push(crate::state::Step {
                    id: "G1".into(),
                    title: "G1".into(),
                    status: StepStatus::Reported,
                    attempts: 1,
                    spec: None,
                    review_since: None,
                });
                Ok(())
            })
            .unwrap();
        let mut dispatcher = f.dispatcher();
        let start = Instant::now();
        let live = |id: &str| f.task.state().unwrap().agent(id).unwrap().live;

        dispatcher
            .close_idle_agents(&f.task.state().unwrap(), start)
            .unwrap();
        dispatcher
            .close_idle_agents(
                &f.task.state().unwrap(),
                start + Duration::from_secs(29 * 60),
            )
            .unwrap();
        assert!(live(&planner.id), "not yet its 30 minutes");
        dispatcher
            .close_idle_agents(
                &f.task.state().unwrap(),
                start + Duration::from_secs(31 * 60),
            )
            .unwrap();
        assert!(
            !live(&planner.id),
            "closed after 30 minutes with nothing open"
        );
        assert!(live(&hammer.id), "G1 is still in its worktree");
        let notes = f.devws.notes();
        assert!(
            notes
                .iter()
                .any(|(_, _, note)| note.title == format!("{} closed while idle", planner.id)),
            "{notes:?}"
        );
        assert!(f.task.log_lines().unwrap().iter().any(|line| line.contains(
            "closed after 31 minutes with nothing open; its handoff note is in the team's memory"
        )));
    }

    #[test]
    fn spend_is_near_at_80_percent_and_reached_at_the_limit() {
        let budget = crate::task::Budget {
            max_tokens: Some(1_000),
            max_hours: Some(2.0),
        };
        assert_eq!(spend(&budget, Some(100), 0.5), Spend::Within);
        assert_eq!(
            spend(&budget, Some(850), 0.5),
            Spend::Near("850 of 1000 tokens".into())
        );
        assert_eq!(
            spend(&budget, Some(100), 2.0),
            Spend::Reached("2.0 of 2 hours".into())
        );
        assert_eq!(
            spend(&budget, None, 0.5),
            Spend::Within,
            "unreadable usage never pauses the task"
        );
        assert_eq!(
            spend(&crate::task::Budget::default(), Some(9), 9.0),
            Spend::Within
        );
    }

    fn started_hours_ago(f: &Fixture, hours: f64, budget: crate::task::Budget) {
        let mut config = f.task.config().unwrap();
        config.budget = budget;
        f.task.write_config(&config).unwrap();
        let at = crate::log::now() - (hours * 3600.0) as u64;
        let mut log = std::fs::OpenOptions::new()
            .append(true)
            .open(f.task.dir().join("log.jsonl"))
            .unwrap();
        writeln!(
            log,
            "{}",
            serde_json::json!({"at": at, "kind": "start", "text": "task started"})
        )
        .unwrap();
    }

    #[test]
    fn near_the_budget_the_leader_is_told_once() {
        let f = fixture();
        started_hours_ago(
            &f,
            0.9,
            crate::task::Budget {
                max_hours: Some(1.0),
                ..Default::default()
            },
        );
        let mut dispatcher = f.dispatcher();
        dispatcher.keep_within_budget(crate::log::now()).unwrap();
        dispatcher.budget_checked = None;
        dispatcher.keep_within_budget(crate::log::now()).unwrap();
        let state = f.task.state().unwrap();
        let told: Vec<_> = state
            .queue
            .iter()
            .filter(|message| message.to == LEADER && message.text.starts_with("BUDGET: 80% used"))
            .collect();
        assert_eq!(told.len(), 1, "once per limit");
        assert!(!state.paused);
    }

    #[test]
    fn at_the_budget_the_task_pauses_and_the_user_decides() {
        let f = fixture();
        started_hours_ago(
            &f,
            2.0,
            crate::task::Budget {
                max_hours: Some(1.5),
                ..Default::default()
            },
        );
        f.dispatcher()
            .keep_within_budget(crate::log::now())
            .unwrap();
        assert!(f.task.state().unwrap().paused, "paused, nothing dropped");
        let log = f.task.log_lines().unwrap().join("\n");
        assert!(
            log.contains(
                "The budget is reached (2.0 of 1.5 hours); the task is paused and nothing is lost"
            ) && log.contains("raise [budget] in task.toml and resume"),
            "{log}"
        );
    }

    #[test]
    fn without_a_budget_nothing_is_checked() {
        let f = fixture();
        started_hours_ago(&f, 100.0, crate::task::Budget::default());
        f.dispatcher()
            .keep_within_budget(crate::log::now())
            .unwrap();
        assert!(!f.task.state().unwrap().paused);
    }

    #[test]
    fn an_order_waits_for_the_agent_to_answer_its_first_message_then_goes_in_once() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.queue(&agent.id, "ORDER one");
        let mut dispatcher = f.dispatcher();

        // Fresh, idle, not yet answered: nothing is typed into it.
        dispatcher.tick().unwrap();
        assert!(
            f.sent_to(&agent.pane).is_empty(),
            "an agent still starting up got an order"
        );
        assert!(!f.task.state().unwrap().agents[0].ready);

        // It answered (its turn ended): it is ready, and the order goes in.
        f.devws.set_state(&agent.pane, PaneState::Done);
        dispatcher.tick().unwrap();
        assert_eq!(f.sent_to(&agent.pane), ["ORDER one"]);
        let state = f.task.state().unwrap();
        assert!(state.agents[0].ready && state.queue.is_empty());

        // It is working on it: nothing more, and not the same twice.
        dispatcher.tick().unwrap();
        assert_eq!(f.sent_to(&agent.pane).len(), 1);
    }

    #[test]
    fn a_pass_restarts_a_refused_worker_whose_panel_went_on_its_next_account() {
        // The pass holds the control lock: the restart it calls must not take
        // it again, or the dispatcher waits on itself and the team stops.
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let f = fixture();
            let worker = f.spawn("executor");
            f.task
                .transaction(|tx| {
                    tx.state.account_holds.insert(
                        "claude".into(),
                        crate::state::AccountHold {
                            until: crate::log::now() + 60,
                            reason: "provider startup error: insufficient_quota".into(),
                        },
                    );
                    tx.state.restart_on_next_account.insert(worker.id.clone());
                    Ok(())
                })
                .unwrap();
            f.devws.set_state(&worker.pane, PaneState::Gone);
            f.dispatcher().tick().unwrap();
            let state = f.task.state().unwrap();
            let restarted = state.agent(&worker.id).unwrap().clone();
            sender
                .send((restarted, worker.pane, state.restart_on_next_account))
                .unwrap();
        });
        let (restarted, former_pane, pending) = receiver
            .recv_timeout(Duration::from_secs(60))
            .expect("the dispatcher pass finished instead of waiting on its own lock");
        assert_eq!(restarted.agent, "codex", "the held account is skipped");
        assert!(restarted.live && !restarted.lost);
        assert_ne!(restarted.pane, former_pane);
        assert!(pending.is_empty());
    }

    #[test]
    fn startup_quota_retries_on_codex_and_preserves_queued_work_once() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.task
            .transaction(|tx| {
                tx.state.orders.push(crate::state::Order {
                    read: Vec::new(),
                    targets: Vec::new(),
                    id: "original".into(),
                    step: "s1".into(),
                    agent: agent.id.clone(),
                    text: "Build".into(),
                    allowed: vec!["src/**".into()],
                    forbidden: vec![],
                    check: None,
                    base: "base-commit".into(),
                    status: OrderStatus::Open,
                    delivered: false,
                    stalled: false,
                    plan: None,
                });
                tx.state
                    .say_about(&agent.id, "ORDER original".into(), Some("original".into()));
                Ok(())
            })
            .unwrap();
        f.devws
            .fail_startup(&agent.pane, crate::worker::FailureScope::Account);
        f.devws.set_state(&agent.pane, PaneState::Done);
        f.dispatcher().tick().unwrap();
        let state = f.task.state().unwrap();
        let replacement = &state.agents[0];
        assert_eq!(replacement.id, agent.id);
        assert_eq!(replacement.agent, "codex");
        assert!(replacement.writes && !replacement.ready && !replacement.lost);
        assert_eq!(state.queue[0].text, "ORDER original");
        assert_eq!(state.orders[0].id, "original");
        assert_eq!(state.orders[0].base, "base-commit");
        assert!(!state.orders[0].delivered);
        assert_ne!(replacement.pane, agent.pane);
        assert_eq!(f.devws.pane_state(&agent.pane).unwrap(), PaneState::Gone);
        // A new dispatcher must not retry the stale error from the former pane.
        f.devws.set_state(&replacement.pane, PaneState::Done);
        f.dispatcher().tick().unwrap();
        f.dispatcher().tick().unwrap();
        assert_eq!(f.sent_to(&replacement.pane), ["ORDER original"]);
        assert!(f.sent_to(&agent.pane).is_empty());
        assert!(f.task.state().unwrap().orders[0].delivered);
        assert_eq!(f.task.state().unwrap().worker_launches[&agent.id].next, 2);
    }

    #[test]
    fn unavailable_model_tries_next_model_on_same_provider() {
        let f = fixture();
        std::fs::write(f.home.roles_dir().join("planner.md"), "+++\nagent = \"claude\"\nmodel = \"preferred\"\n[fallback]\nagent = \"claude\"\nmodel = \"alternative\"\n+++\nPlan.\n").unwrap();
        let agent = f.spawn("planner");
        f.devws
            .fail_startup(&agent.pane, crate::worker::FailureScope::Model);
        f.dispatcher().tick().unwrap();
        let state = f.task.state().unwrap();
        assert_eq!(state.agents[0].agent, "claude");
        assert_ne!(state.agents[0].pane, agent.pane);
        assert_eq!(
            state.worker_launches[&agent.id]
                .selected
                .as_ref()
                .unwrap()
                .model
                .as_deref(),
            Some("alternative")
        );
    }

    #[test]
    fn failed_alternatives_stop_retrying_and_explicit_resume_rechecks_availability() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.queue(&agent.id, "pending work");
        f.devws.set_limits(
            "codex",
            crate::devws::Limits {
                week: Some(100),
                five_hour: None,
            },
        );
        f.devws
            .fail_startup(&agent.pane, crate::worker::FailureScope::Account);
        f.dispatcher().tick().unwrap();
        f.dispatcher().tick().unwrap();
        let state = f.task.state().unwrap();
        assert!(state.agents[0].unavailable && !state.agents[0].live);
        assert_eq!(
            state
                .queue
                .iter()
                .filter(|message| message.to == agent.id)
                .count(),
            1
        );
        assert_eq!(state.worker_launches[&agent.id].next, 2);
        assert!(f.sent_to("%1")[0].contains("unavailable during startup"));
        // Retry only on explicit resume, after the quota has reset.
        f.devws.set_limits("codex", crate::devws::Limits::default());
        f.devws.set_limits(
            "claude",
            crate::devws::Limits {
                week: Some(100),
                five_hour: None,
            },
        );
        Team {
            task: &f.task,
            home: &f.home,
            devws: &f.devws,
        }
        .resume()
        .unwrap();
        let resumed = &f.task.state().unwrap().agents[0];
        assert!(resumed.live && !resumed.unavailable);
        assert_eq!(resumed.agent, "codex");
    }

    #[test]
    fn failed_cleanup_never_duplicates_a_worker_even_on_resume() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.devws.fail_stop(&agent.pane);
        f.devws
            .fail_startup(&agent.pane, crate::worker::FailureScope::Account);
        f.dispatcher().tick().unwrap();
        assert!(f.task.state().unwrap().agents[0].unavailable);
        let result = Team {
            task: &f.task,
            home: &f.home,
            devws: &f.devws,
        }
        .resume();
        assert!(result.is_err());
        assert_eq!(
            f.devws
                .calls()
                .iter()
                .filter(|call| call.starts_with("spawn "))
                .count(),
            1
        );
        assert_eq!(f.task.state().unwrap().agents[0].pane, agent.pane);
        assert!(f.task.state().unwrap().paused);
    }

    #[test]
    fn unrelated_errors_pause_and_healthy_workers_never_trigger_automatic_fallback() {
        let f = fixture();
        let agent = f.spawn("planner");
        f.devws
            .fail_startup(&agent.pane, crate::worker::FailureScope::Other);
        f.dispatcher().tick().unwrap();
        assert!(f.task.state().unwrap().agents[0].unavailable);
        assert_eq!(f.task.state().unwrap().worker_launches[&agent.id].next, 1);
        let f = fixture();
        let agent = f.spawn("planner");
        f.devws
            .fail_startup(&agent.pane, crate::worker::FailureScope::Account);
        Team {
            task: &f.task,
            home: &f.home,
            devws: &f.devws,
        }
        .pause()
        .unwrap();
        f.dispatcher().tick().unwrap();
        assert_eq!(f.task.state().unwrap().agents[0].pane, agent.pane);
        f.task
            .transaction(|tx| {
                tx.state.paused = false;
                tx.state.agents[0].ready = true;
                Ok(())
            })
            .unwrap();
        f.devws.set_state(&agent.pane, PaneState::Working);
        f.dispatcher().tick().unwrap();
        assert_eq!(f.task.state().unwrap().agents[0].pane, agent.pane);
    }

    #[test]
    fn messages_go_in_oldest_first_one_at_a_time_and_only_when_the_agent_can_take_them() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.task
            .transaction(|tx| {
                tx.state.agents[0].ready = true;
                Ok(())
            })
            .unwrap();
        f.queue(&agent.id, "first");
        f.queue(&agent.id, "second");
        let mut dispatcher = f.dispatcher();

        f.devws.set_state(&agent.pane, PaneState::Working);
        dispatcher.tick().unwrap();
        f.devws.set_state(&agent.pane, PaneState::Waiting);
        dispatcher.tick().unwrap();
        assert!(
            f.sent_to(&agent.pane).is_empty(),
            "a busy agent was typed into"
        );

        f.devws.set_state(&agent.pane, PaneState::Idle);
        dispatcher.tick().unwrap();
        assert_eq!(f.sent_to(&agent.pane), ["first"]);
        f.devws.set_state(&agent.pane, PaneState::Done);
        dispatcher.tick().unwrap();
        assert_eq!(f.sent_to(&agent.pane), ["first", "second"]);
    }

    #[test]
    fn delivering_an_order_marks_it_delivered() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.task
            .transaction(|tx| {
                tx.state.agents[0].ready = true;
                tx.state.orders.push(crate::state::Order {
                    read: Vec::new(),
                    targets: Vec::new(),
                    id: "o1".into(),
                    step: "s1".into(),
                    agent: agent.id.clone(),
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
                tx.state
                    .say_about(&agent.id, "ORDER o1".into(), Some("o1".into()));
                Ok(())
            })
            .unwrap();
        f.devws.set_state(&agent.pane, PaneState::Done);
        f.dispatcher().tick().unwrap();
        assert!(f.task.state().unwrap().orders[0].delivered);
        assert!(
            f.task
                .log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("\"kind\":\"delivered\""))
        );
    }

    #[test]
    fn a_paused_task_delivers_nothing() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.task
            .transaction(|tx| {
                tx.state.agents[0].ready = true;
                Ok(())
            })
            .unwrap();
        f.queue(&agent.id, "wait");
        f.queue(LEADER, "report");
        f.devws.set_state(&agent.pane, PaneState::Done);
        Team {
            task: &f.task,
            home: &f.home,
            devws: &f.devws,
        }
        .pause()
        .unwrap();
        f.dispatcher().tick().unwrap();
        assert!(f.devws.sent().is_empty());
        assert_eq!(f.task.state().unwrap().queue.len(), 2);
    }

    #[test]
    fn the_leader_hears_what_is_waiting_as_one_message_when_it_is_not_busy() {
        let f = fixture();
        f.queue(LEADER, "executor-1 reports step s1 done.");
        f.queue(LEADER, "executor-2 is blocked on step s2: no schema");
        let mut dispatcher = f.dispatcher();

        f.devws.set_state("%1", PaneState::Working);
        dispatcher.tick().unwrap();
        assert!(f.devws.sent().is_empty(), "the leader was interrupted");

        f.devws.set_state("%1", PaneState::Done);
        dispatcher.tick().unwrap();
        let sent = f.sent_to("%1");
        assert_eq!(sent.len(), 1);
        assert!(
            sent[0].contains("executor-1 reports")
                && sent[0].contains("executor-2 is blocked")
                && sent[0].contains("---"),
            "{}",
            sent[0]
        );
        assert!(f.task.state().unwrap().queue.is_empty());
    }

    #[test]
    fn a_reviewed_leader_is_nudged_once_per_new_inbox_message_when_not_busy() {
        let f = fixture();
        f.task
            .transaction(|tx| {
                tx.state.workflow = crate::execution::Workflow::Reviewed;
                Ok(())
            })
            .unwrap();
        f.queue(
            LEADER,
            "Planner planner-5 proposed an application baseline.",
        );
        let mut dispatcher = f.dispatcher();

        f.devws.set_state("%1", PaneState::Working);
        dispatcher.tick().unwrap();
        assert!(f.devws.sent().is_empty(), "the leader was interrupted");

        f.devws.set_state("%1", PaneState::Done);
        dispatcher.tick().unwrap();
        assert_eq!(f.sent_to("%1"), [LEADER_NUDGE]);
        assert_eq!(
            f.task.state().unwrap().queue.len(),
            1,
            "the message stays for leader_inbox"
        );

        dispatcher.tick().unwrap();
        assert_eq!(
            f.sent_to("%1").len(),
            1,
            "nudged again for the same message"
        );

        f.devws.set_state("%1", PaneState::Done);
        dispatcher.tick().unwrap();
        assert_eq!(
            f.sent_to("%1").len(),
            1,
            "nudged again for the same message"
        );

        f.queue(LEADER, "executor-1 reports step s1 done.");
        dispatcher.tick().unwrap();
        assert_eq!(f.sent_to("%1"), [LEADER_NUDGE, LEADER_NUDGE]);
    }

    #[test]
    fn an_answered_leader_still_marked_waiting_is_nudged_and_its_recovery_ends() {
        let f = fixture();
        f.task
            .transaction(|tx| {
                tx.state.workflow = crate::execution::Workflow::Reviewed;
                tx.state.recovery.insert(
                    LEADER.into(),
                    crate::state::Recovery {
                        pane: "%98".into(),
                        interrupted: false,
                        error: None,
                    },
                );
                Ok(())
            })
            .unwrap();
        f.queue(LEADER, "RESUME: Read task_status and continue.");
        // Its mark from the escalation the user has answered (leader_wait is No).
        f.devws.set_state("%1", PaneState::Waiting);
        f.dispatcher().tick().unwrap();
        assert_eq!(f.sent_to("%1"), [LEADER_NUDGE]);
        assert!(
            f.task.state().unwrap().recovery.is_empty(),
            "the resumed leader is woken"
        );
    }

    #[test]
    fn a_leader_waiting_for_the_user_stays_marked_until_the_user_answers() {
        let f = fixture();
        f.task
            .transaction(|tx| {
                tx.state.workflow = crate::execution::Workflow::Reviewed;
                tx.state.leader_wait = crate::state::LeaderWait::Asked;
                Ok(())
            })
            .unwrap();
        f.queue(LEADER, "executor-1 reports step s1 done.");
        let mut dispatcher = f.dispatcher();
        let wait = || f.task.state().unwrap().leader_wait;

        // Still in the turn that asked: its own hooks mark it, not us.
        f.devws.set_state("%1", PaneState::Working);
        dispatcher.tick().unwrap();
        assert_eq!(wait(), crate::state::LeaderWait::Asked);

        // The turn ended as done: it needs the user, and is not nudged.
        f.devws.set_state("%1", PaneState::Done);
        dispatcher.tick().unwrap();
        assert_eq!(f.devws.pane_state("%1").unwrap(), PaneState::Waiting);
        assert_eq!(wait(), crate::state::LeaderWait::Shown);
        dispatcher.tick().unwrap();
        assert!(
            f.sent_to("%1").is_empty(),
            "nudged while waiting for the user"
        );

        // The user answered: the leader works again and the wait is over.
        f.devws.set_state("%1", PaneState::Working);
        dispatcher.tick().unwrap();
        assert_eq!(wait(), crate::state::LeaderWait::No);
    }

    fn spent_step(f: &Fixture, status: crate::state::StepStatus) {
        let limit = f.task.config().unwrap().max_attempts;
        f.task
            .transaction(|tx| {
                tx.state.steps.push(crate::state::Step {
                    id: "s9".into(),
                    title: "Tests".into(),
                    status,
                    attempts: limit,
                    spec: None,
                    review_since: None,
                });
                for order in 0..limit {
                    work_failed(&mut tx.state, order);
                }
                Ok(())
            })
            .unwrap();
    }

    fn waiting_step(f: &Fixture, id: &str, since: Option<u64>) {
        f.task
            .transaction(|tx| {
                tx.state.steps.push(crate::state::Step {
                    id: id.into(),
                    title: "Preflight".into(),
                    status: crate::state::StepStatus::Reported,
                    attempts: 1,
                    spec: None,
                    review_since: since,
                });
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_review_the_leader_left_is_reminded_once_per_wait() {
        let f = fixture();
        let now = crate::log::now();
        let left = Some(now - crate::board::STALE_REVIEW_SECS - 1);
        waiting_step(&f, "old-plan", left);
        waiting_step(&f, "fresh-plan", Some(now));
        waiting_step(&f, "unknown-since", None);
        // Busy, so its messages stay queued where the test counts them.
        f.devws.set_state("%1", PaneState::Working);
        let mut dispatcher = f.dispatcher();

        f.task
            .transaction(|tx| {
                tx.state.paused = true;
                Ok(())
            })
            .unwrap();
        dispatcher.tick().unwrap();
        assert_eq!(
            leader_heard(&f, "waiting for you"),
            0,
            "reminded while paused"
        );
        f.task
            .transaction(|tx| {
                tx.state.paused = false;
                Ok(())
            })
            .unwrap();

        dispatcher.tick().unwrap();
        assert_eq!(leader_heard(&f, "waiting for you"), 1);
        assert_eq!(leader_heard(&f, "old-plan"), 1);
        assert_eq!(leader_heard(&f, "unknown-since"), 1);
        assert_eq!(
            leader_heard(&f, "fresh-plan"),
            0,
            "a fresh review is not left"
        );
        assert_eq!(
            leader_heard(&f, "close_step"),
            1,
            "it says how to settle them"
        );

        dispatcher.tick().unwrap();
        assert_eq!(leader_heard(&f, "waiting for you"), 1, "reminded twice");

        // Ordered again and reported again: a new wait, reminded again.
        f.task
            .transaction(|tx| {
                tx.state.step_mut("old-plan").unwrap().review_since = Some(now - 7200);
                Ok(())
            })
            .unwrap();
        dispatcher.tick().unwrap();
        assert_eq!(leader_heard(&f, "old-plan"), 2);
        assert_eq!(leader_heard(&f, "unknown-since"), 1);
    }

    /// One more failure of s9's work, on an order of its own.
    fn work_failed(state: &mut crate::state::State, order: u32) {
        crate::failures::record(
            state,
            "s9",
            Some(&format!("o-s9-{order}")),
            crate::failures::FailureKind::CheckFailed,
            "tests failed",
            u64::from(order),
        );
    }

    fn leader_heard(f: &Fixture, text: &str) -> usize {
        f.task
            .state()
            .unwrap()
            .queue
            .iter()
            .filter(|message| message.to == LEADER && message.text.contains(text))
            .count()
    }

    #[test]
    fn a_spent_step_is_retried_once_after_a_wait_then_marked_failed() {
        use crate::state::{StepRetry, StepStatus};
        let f = fixture();
        spent_step(&f, StepStatus::Blocked);
        let dispatcher = f.dispatcher();
        let limit = f.task.config().unwrap().max_attempts;

        dispatcher.retry_spent_steps(1_000).unwrap();
        dispatcher.retry_spent_steps(1_001).unwrap();
        let state = f.task.state().unwrap();
        assert_eq!(
            state.step_retries.get("s9"),
            Some(&StepRetry::Waiting {
                at: 1_000 + RETRY_AFTER_SECS
            })
        );
        assert_eq!(leader_heard(&f, "retries it once"), 1);

        dispatcher
            .retry_spent_steps(1_000 + RETRY_AFTER_SECS - 1)
            .unwrap();
        assert_eq!(f.task.state().unwrap().attempt_limits.get("s9"), None);

        dispatcher
            .retry_spent_steps(1_000 + RETRY_AFTER_SECS)
            .unwrap();
        let state = f.task.state().unwrap();
        assert_eq!(state.attempt_limits.get("s9"), Some(&(limit + 1)));
        assert_eq!(state.step_retries.get("s9"), Some(&StepRetry::Granted));
        assert_eq!(leader_heard(&f, "one more attempt"), 1);

        // The retry was blocked too.
        f.task
            .transaction(|tx| {
                let step = tx.state.step_mut("s9").unwrap();
                step.attempts = limit + 1;
                step.status = StepStatus::Blocked;
                work_failed(&mut tx.state, limit);
                Ok(())
            })
            .unwrap();
        dispatcher.retry_spent_steps(5_000).unwrap();
        dispatcher.retry_spent_steps(5_001).unwrap();
        let state = f.task.state().unwrap();
        assert_eq!(
            state
                .steps
                .iter()
                .find(|step| step.id == "s9")
                .unwrap()
                .status,
            StepStatus::Exhausted
        );
        assert_eq!(leader_heard(&f, "failed after"), 1);
    }

    #[test]
    fn a_step_with_attempts_left_or_accepted_is_not_retried() {
        use crate::state::StepStatus;
        let f = fixture();
        spent_step(&f, StepStatus::Accepted);
        f.dispatcher().retry_spent_steps(1_000).unwrap();
        assert!(f.task.state().unwrap().step_retries.is_empty());
    }

    // Failures that were not the work's (the environment) use no attempts:
    // the step is not spent, and no retry wait starts.
    #[test]
    fn a_step_that_failed_only_for_the_environment_is_not_spent() {
        use crate::state::StepStatus;
        let f = fixture();
        let limit = f.task.config().unwrap().max_attempts;
        f.task
            .transaction(|tx| {
                tx.state.steps.push(crate::state::Step {
                    id: "s9".into(),
                    title: "Tests".into(),
                    status: StepStatus::Blocked,
                    attempts: limit + 3,
                    spec: None,
                    review_since: None,
                });
                for order in 0..limit + 3 {
                    crate::failures::record(
                        &mut tx.state,
                        "s9",
                        Some(&format!("o-env-{order}")),
                        crate::failures::FailureKind::CheckEnvironment,
                        "docker was down",
                        u64::from(order),
                    );
                }
                Ok(())
            })
            .unwrap();
        f.dispatcher().retry_spent_steps(1_000).unwrap();
        assert!(f.task.state().unwrap().step_retries.is_empty());
        assert_eq!(leader_heard(&f, "retries it once"), 0);
    }

    #[test]
    fn the_user_allowing_more_attempts_starts_the_retry_over() {
        use crate::state::StepStatus;
        let f = fixture();
        spent_step(&f, StepStatus::Blocked);
        f.dispatcher().retry_spent_steps(1_000).unwrap();
        let limit = f.task.config().unwrap().max_attempts;
        crate::orders::allow_attempts(&f.task, "s9", limit + 2).unwrap();
        assert!(f.task.state().unwrap().step_retries.is_empty());
    }

    #[test]
    fn a_lost_panel_is_told_to_the_leader_once() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.task
            .transaction(|tx| {
                tx.state.orders.push(crate::state::Order {
                    read: Vec::new(),
                    targets: Vec::new(),
                    id: "o1".into(),
                    step: "s1".into(),
                    agent: agent.id.clone(),
                    text: "x".into(),
                    allowed: vec![],
                    forbidden: vec![],
                    check: None,
                    base: "abc".into(),
                    status: OrderStatus::Open,
                    delivered: true,
                    stalled: false,
                    plan: None,
                });
                Ok(())
            })
            .unwrap();
        f.devws.set_state(&agent.pane, PaneState::Gone);
        let mut dispatcher = f.dispatcher();

        dispatcher.tick().unwrap();
        dispatcher.tick().unwrap();
        let sent = f.sent_to("%1");
        assert_eq!(sent.len(), 1, "told twice: {sent:?}");
        assert!(
            sent[0].contains("executor-1's panel is gone")
                && sent[0].contains("order o1 (step s1)")
                && sent[0].contains("task resume"),
            "{}",
            sent[0]
        );
        assert!(f.task.state().unwrap().agents[0].lost);
    }

    // A worker whose program failed (a used-up account) closes its panel at
    // once: the leader hears why, not only that the panel went.
    #[test]
    fn a_lost_panel_says_the_error_its_last_turn_ended_with() {
        let f = fixture();
        let agent = f.spawn("executor");
        let control = f.task.dir().join("workers").join(&agent.id).join("control");
        std::fs::create_dir_all(&control).unwrap();
        std::fs::write(
            control.join("turns.jsonl"),
            concat!(
                r#"{"type":"system","subtype":"init"}"#,
                "\n",
                r#"{"type":"result","is_error":true,"api_error":"usage_limit_reached","result":"You've hit your weekly limit · resets Nov 22, 2am"}"#,
                "\n"
            ),
        )
        .unwrap();
        f.devws.set_state(&agent.pane, PaneState::Gone);

        f.dispatcher().tick().unwrap();
        let sent = f.sent_to("%1");
        assert!(
            sent[0].contains("executor-1's panel is gone")
                && sent[0].contains("You've hit your weekly limit"),
            "{}",
            sent[0]
        );
        let log = f.task.log_lines().unwrap().join("\n");
        assert!(log.contains("You've hit your weekly limit"), "{log}");
    }

    #[test]
    fn an_agent_that_ends_its_turn_without_reporting_is_told_to_the_leader_once() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.task
            .transaction(|tx| {
                tx.state.agents[0].ready = true;
                tx.state.orders.push(crate::state::Order {
                    read: Vec::new(),
                    targets: Vec::new(),
                    id: "o1".into(),
                    step: "s1".into(),
                    agent: agent.id.clone(),
                    text: "x".into(),
                    allowed: vec![],
                    forbidden: vec![],
                    check: None,
                    base: "abc".into(),
                    status: OrderStatus::Open,
                    delivered: true,
                    stalled: false,
                    plan: None,
                });
                Ok(())
            })
            .unwrap();
        f.devws.set_state(&agent.pane, PaneState::Working);
        let mut dispatcher = f.dispatcher();
        dispatcher.tick().unwrap();
        assert!(
            f.sent_to("%1").is_empty(),
            "an agent at work is not stalled"
        );

        f.devws.set_state(&agent.pane, PaneState::Done);
        dispatcher.tick().unwrap();
        f.devws.set_state("%1", PaneState::Done);
        dispatcher.tick().unwrap();
        dispatcher.tick().unwrap();
        let sent = f.sent_to("%1");
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert!(
            sent[0].contains("executor-1 stopped without reporting on order o1 (step s1)"),
            "{}",
            sent[0]
        );
    }

    #[test]
    fn an_agent_that_is_given_time_is_not_reported_early() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.task
            .transaction(|tx| {
                tx.state.agents[0].ready = true;
                tx.state.orders.push(crate::state::Order {
                    read: Vec::new(),
                    targets: Vec::new(),
                    id: "o1".into(),
                    step: "s1".into(),
                    agent: agent.id.clone(),
                    text: "x".into(),
                    allowed: vec![],
                    forbidden: vec![],
                    check: None,
                    base: "abc".into(),
                    status: OrderStatus::Open,
                    delivered: true,
                    stalled: false,
                    plan: None,
                });
                Ok(())
            })
            .unwrap();
        f.devws.set_state(&agent.pane, PaneState::Done);
        let mut patient = Dispatcher::new(&f.task, &f.devws, &f.home, Duration::from_secs(60));
        patient.tick().unwrap();
        patient.tick().unwrap();
        assert!(f.sent_to("%1").is_empty());
    }

    #[test]
    fn only_one_dispatcher_runs_per_task() {
        let f = fixture();
        let first = lock(&f.task).unwrap();
        assert!(
            lock(&f.task)
                .unwrap_err()
                .to_string()
                .contains("already running")
        );
        drop(first);
        // A process another test forks at that moment holds a copy of the
        // lock until it execs, so the lock comes back within a moment.
        let freed = (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(20));
            lock(&f.task).is_ok()
        });
        assert!(freed, "the lock is free once its holder goes");
    }

    #[test]
    fn the_pane_shows_the_recent_log_and_then_what_is_new() {
        let f = fixture();
        let mut output = Vec::new();
        f.task
            .transaction(|tx| {
                tx.log(
                    Kind::Order,
                    Some("backend-1"),
                    Some("s2"),
                    "Write src/login.rs\nsecond line",
                );
                Ok(())
            })
            .unwrap();
        f.dispatcher()
            .run(&mut output, Duration::from_millis(1), Some(2))
            .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(
            text.contains("order     backend-1 s2  Write src/login.rs"),
            "{text}"
        );
        assert!(!text.contains("second line"), "only the first line shows");
        assert!(text.contains("task started"));
    }

    #[test]
    fn a_log_line_is_one_readable_line() {
        let line = r#"{"at":1700000000,"kind":"report","agent":"executor-1","step":"s3","text":"added login"}"#;
        let shown = format_line(line);
        assert!(
            shown.ends_with("report    executor-1 s3  added login"),
            "{shown}"
        );
        assert_eq!(format_line("not json"), "not json");
        assert!(format_line(r#"{"at":1,"kind":"start","text":"x"}"#).contains("start     -  x"));
    }
}
