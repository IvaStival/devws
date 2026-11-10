//! The team board: one view of where the team stands (`devagentics board`).
//! `Board` is plain data built from the task's state, the roles and what devws
//! says about each pane; `render` draws it.

use crate::board_usage::{LEADER_NAME, TeamUsage};
use crate::devws::{Devws, PaneState, Usage};
use crate::roles::{Group, Role, Roles};
use crate::state::LEADER;
use crate::state::StepStatus;
use crate::task::Task;
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, PartialEq)]
pub struct Board {
    pub task: String,
    pub branch: String,
    pub summary: String,
    pub phase: String,
    pub leader: String,
    pub leader_pane: Option<String>,
    /// What the leader's pane is doing; `None` without a pane.
    pub leader_state: Option<PaneState>,
    pub leader_usage: Option<Usage>,
    pub steps: Vec<StepRow>,
    /// Full context keyed by stable row identity; presentation never truncates this.
    pub details: BTreeMap<String, String>,
    pub accounts: Vec<Account>,
    /// Every role there is, running or not, by group: the CTO, the seniors,
    /// the factory floor.
    pub roles: Vec<RoleRow>,
    /// Questions not answered yet: `q7 executor-1 → planner-1: …`.
    pub questions: Vec<String>,
    /// Steps the leader has to act on: reported, failed, blocked or sent back.
    pub review: Vec<StepRow>,
    /// Orders waiting to be typed into an agent.
    pub queue: Vec<QueueRow>,
    pub done: Vec<StepRow>,
    /// The leader and each agent: what they used.
    pub usage: Vec<(String, Usage)>,
}

#[derive(Debug, PartialEq)]
pub struct Account {
    pub name: String,
    pub five_hour: Option<u32>,
    pub week: Option<u32>,
}

#[derive(Debug, PartialEq)]
pub struct RoleRow {
    pub name: String,
    pub group: Group,
    /// The program and how it is set: `claude · high`.
    pub program: String,
    pub description: Option<String>,
    pub agents: Vec<AgentRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentRow {
    /// The key that shows it (1–9), in the board's order.
    pub number: Option<u8>,
    pub id: String,
    pub pane: String,
    /// The lane it writes in.
    pub lane: Option<String>,
    pub state: PaneState,
    /// The step it has an order for.
    pub now: Option<String>,
    /// The steps ordered for it that it has not been told yet.
    pub next: Vec<String>,
    /// What it used, as far as devws knows.
    pub usage: Option<Usage>,
    /// Steps it was given that are now accepted.
    pub accepted: u32,
    /// Orders it got for a step after its first one there.
    pub retries: u32,
}

#[derive(Debug, PartialEq)]
pub struct StepRow {
    pub id: String,
    pub title: String,
    pub status: StepStatus,
    pub attempts: u32,
}

#[derive(Debug, PartialEq)]
pub struct QueueRow {
    pub id: String,
    pub label: String,
}

fn program(role: &Role) -> String {
    let mut parts = vec![role.agent.clone()];
    parts.extend(role.model.clone());
    parts.extend(role.effort.clone());
    parts.join(" · ")
}

impl Board {
    pub fn build(task: &Task, devws: &dyn Devws, roles: &Roles, usage: TeamUsage) -> Result<Self> {
        let config = task.config()?;
        let state = task.state()?;
        let step_title = |id: &str| {
            state
                .steps
                .iter()
                .find(|step| step.id == id)
                .map_or_else(|| id.to_string(), |step| format!("{id} {}", step.title))
        };

        let mut rows: Vec<RoleRow> = roles
            .all()
            .iter()
            .map(|role| RoleRow {
                name: role.name.clone(),
                group: role.group,
                // A leader on its fallback (leader.rs) shows the program it
                // runs now.
                program: match role.on_fallback() {
                    Some(fallback)
                        if state.leader_agent.as_deref() == Some(fallback.agent.as_str())
                            && fallback.agent != role.agent =>
                    {
                        format!("{} · fallback", program(&fallback))
                    }
                    _ => program(role),
                },
                description: role.description.clone(),
                agents: Vec::new(),
            })
            .collect();
        let mut working = 0;
        for agent in state.agents.iter().filter(|agent| agent.live) {
            let pane_state = if agent.lost {
                PaneState::Gone
            } else {
                devws.pane_state(&agent.pane)?
            };
            if pane_state == PaneState::Working {
                working += 1;
            }
            let next = state
                .queue
                .iter()
                .filter(|message| message.to == agent.id)
                .filter_map(|message| message.order.as_deref())
                .filter_map(|order_id| state.orders.iter().find(|order| order.id == order_id))
                .map(|order| step_title(&order.step))
                .collect();
            // An agent's orders by step: the steps it got accepted, and every
            // order after its first on a step a retry.
            let mut steps_given: BTreeMap<&str, u32> = BTreeMap::new();
            for order in state.orders.iter().filter(|order| order.agent == agent.id) {
                *steps_given.entry(order.step.as_str()).or_default() += 1;
            }
            let accepted = steps_given
                .keys()
                .filter(|step| {
                    state
                        .steps
                        .iter()
                        .any(|known| known.id == **step && known.status == StepStatus::Accepted)
                })
                .count() as u32;
            let retries = steps_given.values().map(|orders| orders - 1).sum();
            let row = AgentRow {
                usage: usage.by_agent.get(&agent.id).copied(),
                accepted,
                retries,
                number: None,
                id: agent.id.clone(),
                pane: agent.pane.clone(),
                lane: agent.lane.clone(),
                state: pane_state,
                now: state
                    .open_order_of(&agent.id)
                    .filter(|order| order.delivered)
                    .map(|order| step_title(&order.step)),
                next,
            };
            // A role removed after its agent started still shows, as itself.
            if let Some(role) = rows.iter_mut().find(|role| role.name == agent.role) {
                role.agents.push(row);
            } else {
                rows.push(RoleRow {
                    name: agent.role.clone(),
                    group: agent.group(),
                    program: agent.agent.clone(),
                    description: None,
                    agents: vec![row],
                });
            }
        }

        rows.sort_by(|a, b| (a.group, &a.name).cmp(&(b.group, &b.name)));
        for (number, agent) in rows
            .iter_mut()
            .flat_map(|row| row.agents.iter_mut())
            .filter(|agent| agent.state != PaneState::Gone)
            .take(9)
            .enumerate()
        {
            agent.number = Some(number as u8 + 1);
        }

        let accounts: BTreeSet<_> = state
            .agents
            .iter()
            .filter(|agent| agent.live)
            .map(|agent| agent.agent.clone())
            .chain(
                state
                    .leader_agent
                    .clone()
                    .or_else(|| roles.get("leader").map(|role| role.agent.clone())),
            )
            .collect();
        let mut account_rows = Vec::new();
        for name in accounts {
            let used = devws.limits(&name)?;
            account_rows.push(Account {
                name,
                five_hour: used.five_hour,
                week: used.week,
            });
        }

        let step_row = |step: &crate::state::Step| StepRow {
            id: step.id.clone(),
            title: step.title.clone(),
            status: step.status,
            attempts: step.attempts,
        };
        let review = state
            .steps
            .iter()
            .filter(|step| {
                matches!(
                    step.status,
                    StepStatus::Reported
                        | StepStatus::Failed
                        | StepStatus::Blocked
                        | StepStatus::Rejected
                )
            })
            .map(step_row)
            .collect();
        let done: Vec<StepRow> = state
            .steps
            .iter()
            .rev()
            .filter(|step| step.status == StepStatus::Accepted)
            .map(step_row)
            .collect();
        let queue = state
            .queue
            .iter()
            .filter(|message| message.to != LEADER)
            .filter_map(|message| {
                let order = state
                    .orders
                    .iter()
                    .find(|order| Some(&order.id) == message.order.as_ref())?;
                Some(QueueRow {
                    id: order.id.clone(),
                    label: format!("{}  for {}", step_title(&order.step), message.to),
                })
            })
            .collect();

        let plan = if !task.dir().join("plan.md").is_file() {
            "No plan yet."
        } else if state.approved {
            "Plan approved."
        } else {
            "Plan written; it waits for your approval."
        };
        let mut summary = vec![plan.to_string()];
        if working > 0 {
            summary.push(format!("{working} working."));
        }
        if !state.steps.is_empty() {
            summary.push(format!(
                "{} of {} steps accepted.",
                done.len(),
                state.steps.len()
            ));
        }
        let for_leader = state
            .queue
            .iter()
            .filter(|message| message.to == LEADER)
            .count();
        if for_leader > 0 {
            summary.push(format!("{for_leader} to tell the leader."));
        }
        let questions: Vec<String> = state
            .questions
            .iter()
            .filter(|question| question.answer.is_none())
            .map(|question| {
                format!(
                    "{} {} → {}: {}",
                    question.id, question.from, question.to, question.text
                )
            })
            .collect();
        if !questions.is_empty() {
            summary.push(format!("{} open questions.", questions.len()));
        }
        if state.paused {
            summary.push("Paused.".to_string());
        }

        let mut details = BTreeMap::new();
        for agent in state.agents.iter().filter(|agent| agent.live) {
            let order = state.open_order_of(&agent.id);
            let mut text = format!(
                "{}\nRole: {}\nRepository: {}\nLane: {}\nReady: {}",
                agent.id,
                agent.role,
                agent.repo,
                agent.lane.as_deref().unwrap_or("—"),
                agent.ready
            );
            if let Some(order) = order {
                text.push_str(&format!(
                    "\nOrder: {} · {}\nDelivered: {} · Stalled: {}\n\n{}",
                    order.id,
                    step_title(&order.step),
                    order.delivered,
                    order.stalled,
                    order.text
                ));
            }
            details.insert(format!("agent:{}", agent.id), text);
        }
        for order in &state.orders {
            details.insert(
                format!("queue:{}", order.id),
                format!(
                    "Order {} → {}\n{}\nDelivered: {} · Stalled: {}\n\n{}",
                    order.id,
                    order.agent,
                    step_title(&order.step),
                    order.delivered,
                    order.stalled,
                    order.text
                ),
            );
        }
        for step in &state.steps {
            let mut text = format!(
                "{} {}\nStatus: {:?} · Attempts: {}",
                step.id, step.title, step.status, step.attempts
            );
            for order in state.orders.iter().filter(|order| order.step == step.id) {
                text.push_str(&format!(
                    "\n\nOrder {} → {} · {:?}\nDelivered: {} · Stalled: {}\n{}",
                    order.id, order.agent, order.status, order.delivered, order.stalled, order.text
                ));
            }
            details.insert(format!("step:{}", step.id), text);
        }
        let leader_program = rows
            .iter()
            .find(|row| row.group == Group::Cto)
            .map(|row| row.program.clone())
            .unwrap_or_else(|| "unknown".into());
        let leader = match &state.leader_switch {
            Some(reason) => format!("{leader_program} · handover pending: {reason}"),
            None => leader_program,
        };
        let leader_pane = devws.pane_with_role(&config.workspace, "agent")?;
        let leader_state = match &leader_pane {
            Some(pane) => Some(devws.pane_state(pane)?),
            None => None,
        };
        Ok(Self {
            phase: if state.paused {
                "Paused"
            } else if state.approved {
                "Approved"
            } else if task.dir().join("plan.md").is_file() {
                "Awaiting approval"
            } else {
                "Planning"
            }
            .into(),
            leader,
            leader_pane,
            leader_state,
            leader_usage: usage.by_agent.get(LEADER_NAME).copied(),
            steps: state.steps.iter().map(step_row).collect(),
            details,
            task: config.id,
            branch: config.branch,
            summary: summary.join(" "),
            accounts: account_rows,
            roles: rows,
            questions,
            review,
            queue,
            done,
            usage: usage.by_role,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent_args::Kind as Program;
    use crate::devws::Limits;
    use crate::devws::fake::FakeDevws;
    use crate::home::Home;
    use crate::start::start_task;
    use crate::team::Team;
    use std::fs;
    use tempfile::TempDir;

    /// A started task on a repo, with a claude agent that has 40% of its five
    /// hours used.
    pub(crate) fn started(dir: &TempDir) -> (Home, FakeDevws, Task, tempfile::TempDir) {
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_limits(
                "claude",
                Limits {
                    five_hour: Some(40),
                    week: None,
                },
            );
        start_task(&home, &devws, repo.path(), "d").unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        (home, devws, task, repo)
    }

    #[test]
    fn every_role_is_listed_and_a_running_agent_shows_under_its_role() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("home/roles")).unwrap();
        fs::write(
            dir.path().join("home/roles/tester.md"),
            "+++\nagent = \"codex\"\ndescription = \"Runs the tests\"\n+++\nTest it.\n",
        )
        .unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        devws.set_state(&planner.pane, PaneState::Working);

        let roles = Roles::load(&home, None).unwrap();
        let board = Board::build(&task, &devws, &roles, TeamUsage::default()).unwrap();
        let names: Vec<_> = board.roles.iter().map(|role| role.name.as_str()).collect();
        assert_eq!(
            names,
            ["leader", "planner", "tester", "executor"],
            "by group"
        );
        let tester = &board.roles[2];
        assert_eq!(tester.program, "codex");
        assert_eq!(tester.description.as_deref(), Some("Runs the tests"));
        assert!(tester.agents.is_empty());
        let running = &board.roles[1].agents[0];
        assert_eq!(running.number, Some(1), "the first agent is key 1");
        assert_eq!(
            (running.id.as_str(), running.state),
            ("planner-1", PaneState::Working)
        );
        assert_eq!(
            board.accounts,
            [Account {
                name: "claude".into(),
                five_hour: Some(40),
                week: None
            }]
        );
        assert_eq!(board.summary, "No plan yet. 1 working.");
    }

    #[test]
    fn each_agent_shows_what_it_used_got_accepted_and_retried() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        team.spawn_agent("planner", None).unwrap();
        let order = |id: &str, step: &str| crate::state::Order {
            id: id.into(),
            step: step.into(),
            agent: "planner-1".into(),
            text: String::new(),
            allowed: vec![],
            forbidden: vec![],
            check: None,
            base: String::new(),
            status: crate::state::OrderStatus::Closed,
            delivered: true,
            stalled: false,
            plan: None,
        };
        task.transaction(|tx| {
            for (id, status, attempts) in [
                ("s1", StepStatus::Accepted, 1),
                ("s2", StepStatus::Accepted, 3),
                ("s3", StepStatus::Ordered, 1),
            ] {
                tx.state.steps.push(crate::state::Step {
                    id: id.into(),
                    title: format!("title {id}"),
                    status,
                    attempts,
                });
            }
            tx.state.orders.extend([
                order("o1", "s1"),
                order("o2", "s2"),
                order("o3", "s2"),
                order("o4", "s2"),
                order("o5", "s3"),
            ]);
            Ok(())
        })
        .unwrap();
        let used = |input| Usage {
            input,
            output: 1,
            cache: 0,
            cost: 0.5,
            unpriced: false,
        };
        let mut usage = TeamUsage::default();
        usage.by_agent.insert("planner-1".into(), used(700));
        usage.by_agent.insert(LEADER_NAME.into(), used(9000));
        let roles = Roles::load(&home, None).unwrap();
        let board = Board::build(&task, &devws, &roles, usage).unwrap();
        let planner = &board.roles[1].agents[0];
        assert_eq!(planner.usage.map(|usage| usage.input), Some(700));
        assert_eq!(planner.accepted, 2, "s1 and s2 are accepted");
        assert_eq!(planner.retries, 2, "s2 took three orders");
        assert_eq!(board.leader_usage.map(|usage| usage.input), Some(9000));
    }

    #[test]
    fn steps_split_into_done_and_what_needs_the_leader() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        task.transaction(|tx| {
            for (id, status) in [
                ("s1", StepStatus::Accepted),
                ("s2", StepStatus::Reported),
                ("s3", StepStatus::Accepted),
                ("s4", StepStatus::Ordered),
            ] {
                tx.state.steps.push(crate::state::Step {
                    id: id.into(),
                    title: format!("title {id}"),
                    status,
                    attempts: 1,
                });
            }
            tx.state.say(LEADER, "x".into());
            tx.state.paused = true;
            Ok(())
        })
        .unwrap();
        let roles = Roles::load(&home, None).unwrap();
        let board = Board::build(&task, &devws, &roles, TeamUsage::default()).unwrap();
        let ids = |rows: &[StepRow]| rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&board.done), ["s3", "s1"], "newest first");
        assert_eq!(ids(&board.review), ["s2"]);
        assert_eq!(
            board.summary,
            "No plan yet. 2 of 4 steps accepted. 1 to tell the leader. Paused."
        );
    }

    #[test]
    fn an_order_not_yet_typed_in_is_the_agents_next_and_in_the_queue() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        task.transaction(|tx| {
            tx.state.steps.push(crate::state::Step {
                id: "s1".into(),
                title: "add a file".into(),
                status: StepStatus::Ordered,
                attempts: 1,
            });
            tx.state.orders.push(crate::state::Order {
                id: "o1".into(),
                step: "s1".into(),
                agent: planner.id.clone(),
                text: "do it".into(),
                allowed: vec![],
                forbidden: vec![],
                check: None,
                base: "abc".into(),
                status: crate::state::OrderStatus::Open,
                delivered: false,
                stalled: false,
                plan: None,
            });
            let number = tx.state.next_number();
            tx.state.queue.push(crate::state::Message {
                id: number,
                to: planner.id.clone(),
                text: "ORDER o1".into(),
                order: Some("o1".into()),
            });
            Ok(())
        })
        .unwrap();
        let roles = Roles::load(&home, None).unwrap();
        let board = Board::build(&task, &devws, &roles, TeamUsage::default()).unwrap();
        let row = &board.roles[1].agents[0];
        assert_eq!(row.now, None, "not told yet");
        assert_eq!(row.next, ["s1 add a file"]);
        assert_eq!(
            board.queue,
            [QueueRow {
                id: "o1".into(),
                label: "s1 add a file  for planner-1".into()
            }]
        );
        assert!(board.details["queue:o1"].contains("do it"));
        assert!(board.details["agent:planner-1"].contains("Delivered: false"));
        assert!(board.details["step:s1"].contains("Order o1"));
    }
}
