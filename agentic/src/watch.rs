//! The dispatcher, and the log pane of the task's workspace
//! (`devagentics watch <task>`). It types what is waiting into the agents'
//! panels when they can take it, notices a panel that went or an agent that
//! stopped without reporting, and shows the log as it grows.

use crate::devws::{Delivery, Devws, PaneState};
use crate::log::Kind;
use crate::state::{Agent, LEADER, OrderStatus};
use crate::task::Task;
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

pub struct Dispatcher<'a> {
    task: &'a Task,
    devws: &'a dyn Devws,
    grace: Duration,
    /// When each agent was first seen done with an open order.
    done_since: HashMap<String, Instant>,
}

impl<'a> Dispatcher<'a> {
    pub fn new(task: &'a Task, devws: &'a dyn Devws, grace: Duration) -> Self {
        Self {
            task,
            devws,
            grace,
            done_since: HashMap::new(),
        }
    }

    /// One pass over the team.
    pub fn tick(&mut self) -> Result<()> {
        let state = self.task.state()?;
        for agent in state
            .agents
            .iter()
            .filter(|agent| agent.live && !agent.lost)
        {
            let pane_state = self.devws.pane_state(&agent.pane)?;
            if pane_state == PaneState::Gone {
                self.lose(agent)?;
                continue;
            }
            self.watch_for_a_stall(agent, pane_state)?;

            // The first thing it did was answer its role: now it takes orders.
            let ready =
                agent.ready || (pane_state == PaneState::Done && self.make_ready(&agent.id)?);
            if !ready || state.paused || !matches!(pane_state, PaneState::Done | PaneState::Idle) {
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
        if !state.paused {
            self.tell_the_leader()?;
        }
        Ok(())
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
        let Some(pane) = self
            .devws
            .pane_with_role(&self.task.config()?.workspace, "agent")?
        else {
            return Ok(());
        };
        if !matches!(
            self.devws.pane_state(&pane)?,
            PaneState::Done | PaneState::Idle
        ) {
            return Ok(());
        }
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
            tx.state.queue.retain(|message| !ids.contains(&message.id));
            if let Some(order) = order.and_then(|id| {
                tx.state
                    .orders
                    .iter_mut()
                    .find(|candidate| candidate.id == id)
            }) {
                order.delivered = true;
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

    /// The panel went: the leader is told once.
    fn lose(&self, agent: &Agent) -> Result<()> {
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
                    "{}'s panel is gone.{order} `devagentics task resume` starts it again.",
                    agent.id
                ),
            );
            tx.log(Kind::Lost, Some(&agent.id), None, "its panel is gone");
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
pub fn format_line(line: &str) -> String {
    let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
        return line.to_string();
    };
    let at = entry["at"]
        .as_i64()
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
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
    use crate::agent_args::Kind as Program;
    use crate::devws::fake::FakeDevws;
    use crate::home::Home;
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
        let devws = FakeDevws::new(&dir.path().join("w")).with_kind("claude", Program::Claude);
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
            Dispatcher::new(&self.task, &self.devws, Duration::ZERO)
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
    fn a_lost_panel_is_told_to_the_leader_once() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.task
            .transaction(|tx| {
                tx.state.orders.push(crate::state::Order {
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

    #[test]
    fn an_agent_that_ends_its_turn_without_reporting_is_told_to_the_leader_once() {
        let f = fixture();
        let agent = f.spawn("executor");
        f.task
            .transaction(|tx| {
                tx.state.agents[0].ready = true;
                tx.state.orders.push(crate::state::Order {
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
        let mut patient = Dispatcher::new(&f.task, &f.devws, Duration::from_secs(60));
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
