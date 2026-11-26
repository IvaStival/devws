//! What each agent of the team has used, for the board. Reading a
//! conversation's log takes a moment, so a running agent's is read again only
//! now and then; an agent that stopped has its last reading in the state.

use crate::devws::{Devws, Usage};
use crate::task::Task;
use anyhow::Result;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The leader's name in the board; it is not one of the task's agents.
pub const LEADER_NAME: &str = "leader";

/// What a team used, by role (the Usage view) and by agent (each row).
#[derive(Debug, Default)]
pub struct TeamUsage {
    pub by_role: Vec<(String, Usage)>,
    /// By agent id; the leader as `LEADER_NAME`.
    pub by_agent: HashMap<String, Usage>,
}

pub struct UsageReader {
    every: Duration,
    seen: HashMap<String, (Instant, Option<Usage>)>,
}

impl UsageReader {
    pub fn new(every: Duration) -> Self {
        Self {
            every,
            seen: HashMap::new(),
        }
    }

    /// Reads again, in one devws call, every (name, pane) whose reading is
    /// older than `every`.
    fn refresh(&mut self, devws: &dyn Devws, wanted: &[(String, String)]) -> Result<()> {
        let stale: Vec<&(String, String)> = wanted
            .iter()
            .filter(|(name, _)| {
                self.seen
                    .get(name)
                    .is_none_or(|(at, _)| at.elapsed() >= self.every)
            })
            .collect();
        let panes: Vec<String> = stale.iter().map(|(_, pane)| pane.clone()).collect();
        for ((name, _), usage) in stale.into_iter().zip(devws.usages(&panes)?) {
            self.seen.insert(name.clone(), (Instant::now(), usage));
        }
        Ok(())
    }

    fn read(&self, name: &str) -> Option<Usage> {
        self.seen.get(name).and_then(|(_, usage)| *usage)
    }

    /// What the team used: the leader and each agent on its own, and the
    /// leader first, then a row per role in the order its first agent
    /// started (the leader starts agents of a role again and again, and they
    /// count as one). Agents nothing is known about are left out.
    pub fn of_team(&mut self, task: &Task, devws: &dyn Devws) -> Result<TeamUsage> {
        let config = task.config()?;
        let state = task.state()?;
        let mut usages = TeamUsage::default();
        let leader = devws.pane_with_role(&config.workspace, "agent")?;
        let mut wanted: Vec<(String, String)> = leader
            .iter()
            .map(|pane| (LEADER_NAME.to_string(), pane.clone()))
            .collect();
        wanted.extend(
            state
                .agents
                .iter()
                .filter(|agent| agent.live && !agent.lost)
                .map(|agent| (agent.id.clone(), agent.pane.clone())),
        );
        self.refresh(devws, &wanted)?;
        if leader.is_some()
            && let Some(usage) = self.read(LEADER_NAME)
        {
            usages.by_role.push((LEADER_NAME.to_string(), usage));
            usages.by_agent.insert(LEADER_NAME.to_string(), usage);
        }
        for agent in &state.agents {
            let usage = if agent.live && !agent.lost {
                self.read(&agent.id)
            } else {
                agent.usage
            };
            let Some(usage) = usage else { continue };
            usages.by_agent.insert(agent.id.clone(), usage);
            match usages
                .by_role
                .iter_mut()
                .find(|(name, _)| *name == agent.role)
            {
                Some((_, sum)) => *sum = sum.plus(usage),
                None => usages.by_role.push((agent.role.clone(), usage)),
            }
        }
        Ok(usages)
    }
}

pub fn total(usages: &[(String, Usage)]) -> Usage {
    usages
        .iter()
        .fold(Usage::default(), |sum, (_, usage)| sum.plus(*usage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::tests::started;
    use crate::devws::PaneState;
    use crate::team::Team;
    use tempfile::TempDir;

    fn usage(input: u64, cost: f64) -> Usage {
        Usage {
            input,
            output: 10,
            cache: 100,
            cost,
            unpriced: false,
        }
    }

    #[test]
    fn the_leader_and_each_role_are_listed_and_a_stopped_agent_keeps_its_last_reading() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let config = task.config().unwrap();
        devws.set_role(&config.workspace, "agent", "%1");
        devws.set_usage("%1", usage(1000, 0.5));
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let first = team.spawn_agent("planner", None).unwrap();
        let second = team.spawn_agent("planner", None).unwrap();
        devws.set_usage(&first.pane, usage(200, 0.25));
        devws.set_state(&second.pane, PaneState::Working);

        let mut reader = UsageReader::new(Duration::ZERO);
        let usages = reader.of_team(&task, &devws).unwrap();
        let names: Vec<_> = usages
            .by_role
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["leader", "planner"], "a row per role");
        assert_eq!(total(&usages.by_role).input, 1200);

        devws.set_usage(&second.pane, usage(50, 0.5));
        let both = reader.of_team(&task, &devws).unwrap();
        assert_eq!(
            both.by_role.len(),
            2,
            "the role's agents are one row: {both:?}"
        );
        assert_eq!(
            (both.by_role[1].1.input, both.by_role[1].1.cost),
            (250, 0.75),
            "planner-1 and planner-2 added up"
        );
        assert_eq!(both.by_agent["planner-1"].input, 200, "each agent's own");
        assert_eq!(both.by_agent["planner-2"].input, 50);
        assert_eq!(both.by_agent[LEADER_NAME].input, 1000);

        team.stop_agent("planner-1").unwrap();
        devws.set_usage(&first.pane, usage(9999, 9.0));
        let after = reader.of_team(&task, &devws).unwrap();
        assert_eq!(
            after.by_role[1].1.input, 250,
            "a stopped agent keeps what it had"
        );
        assert_eq!(after.by_agent["planner-1"].input, 200);
    }

    #[test]
    fn a_reading_is_reused_until_it_is_old() {
        let dir = TempDir::new().unwrap();
        let (_home, devws, task, _repo) = started(&dir);
        let config = task.config().unwrap();
        devws.set_role(&config.workspace, "agent", "%1");
        devws.set_usage("%1", usage(1, 0.0));
        let mut reader = UsageReader::new(Duration::from_secs(60));
        reader.of_team(&task, &devws).unwrap();
        reader.of_team(&task, &devws).unwrap();
        let reads = devws
            .calls()
            .iter()
            .filter(|call| call.starts_with("usage"))
            .count();
        assert_eq!(reads, 1);
    }
}
