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

    fn read(&mut self, devws: &dyn Devws, name: &str, pane: &str) -> Result<Option<Usage>> {
        if let Some((at, usage)) = self.seen.get(name)
            && at.elapsed() < self.every
        {
            return Ok(*usage);
        }
        let usage = devws.usage(pane)?;
        self.seen.insert(name.to_string(), (Instant::now(), usage));
        Ok(usage)
    }

    /// What the team used: the leader and each agent on its own, and the
    /// leader first, then a row per role in the order its first agent
    /// started (the leader starts agents of a role again and again, and they
    /// count as one). Agents nothing is known about are left out.
    pub fn of_team(&mut self, task: &Task, devws: &dyn Devws) -> Result<TeamUsage> {
        let config = task.config()?;
        let state = task.state()?;
        let mut usages = TeamUsage::default();
        if let Some(pane) = devws.pane_with_role(&config.workspace, "agent")?
            && let Some(usage) = self.read(devws, LEADER_NAME, &pane)?
        {
            usages.by_role.push((LEADER_NAME.to_string(), usage));
            usages.by_agent.insert(LEADER_NAME.to_string(), usage);
        }
        for agent in &state.agents {
            let usage = if agent.live && !agent.lost {
                self.read(devws, &agent.id, &agent.pane)?
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
