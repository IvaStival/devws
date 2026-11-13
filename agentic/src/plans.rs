//! The plans: the overall one the user approves, and the seniors' execution
//! plans the CTO approves by accepting their step.

use crate::log::Kind;
use crate::roles::Group;
use crate::state::ExecutionPlan;
use crate::task::Task;
use crate::team::Team;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub enum Stored {
    /// The task's plan, for the user to approve.
    Overall,
    /// A senior's plan for the step of its order.
    Execution { step: String },
}

fn path(task: &Task, step: &str) -> PathBuf {
    task.dir().join("plans").join(format!("{step}.md"))
}

/// Stores a plan written from an agent's pane: the overall plan until the user
/// has approved one, then the execution plan of the senior's open order.
pub fn store(team: &Team, pane: &str, text: &str) -> Result<Stored> {
    let state = team.task.state()?;
    let agent = state
        .agent_in_pane(pane)
        .with_context(|| format!("{pane} is not a team agent's pane"))?;
    if !state.approved {
        team.write_plan(text)?;
        return Ok(Stored::Overall);
    }
    if agent.group() != Group::Senior {
        bail!(
            "only a senior writes an execution plan; {} is on the {}",
            agent.id,
            agent.group().label().to_lowercase()
        );
    }
    let order = state.open_order_of(&agent.id).with_context(|| {
        format!(
            "{} has no open order: an execution plan is for the step it was ordered",
            agent.id
        )
    })?;
    if text.trim().is_empty() {
        bail!("the plan is empty");
    }
    let (step, author) = (order.step.clone(), agent.id.clone());
    let file = path(team.task, &step);
    std::fs::create_dir_all(file.parent().unwrap())?;
    let temporary = file.with_extension("tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, &file)?;
    team.task.transaction(|tx| {
        tx.state.plans.retain(|plan| plan.step != step);
        tx.state.plans.push(ExecutionPlan {
            step: step.clone(),
            author: author.clone(),
        });
        tx.log(
            Kind::Plan,
            Some(&author),
            Some(&step),
            &format!("execution plan written ({} lines)", text.lines().count()),
        );
        Ok(())
    })?;
    Ok(Stored::Execution { step })
}

/// The execution plan of a step.
pub fn read(task: &Task, step: &str) -> Result<String> {
    if task.state()?.plan(step).is_none() {
        bail!("step {step} has no execution plan");
    }
    std::fs::read_to_string(path(task, step))
        .with_context(|| format!("reading the plan of step {step}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::tests::started;
    use crate::orders::{OrderRequest, send_order};
    use tempfile::TempDir;

    fn order(task: &Task, agent: &str, step: &str) {
        send_order(
            task,
            OrderRequest {
                profiles: Vec::new(),
                agent: agent.into(),
                step: step.into(),
                title: "plan the screen".into(),
                text: "plan it".into(),
                allowed: vec![],
                forbidden: vec![],
                check: Some("true".into()),
                plan: None,
                spec: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn a_plan_is_the_overall_one_until_approved_then_a_seniors_execution_plan() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        assert_eq!(
            store(&team, &planner.pane, "1. s1: the screen\n").unwrap(),
            Stored::Overall
        );
        assert!(read(&task, "s1").is_err());
        team.approve().unwrap();

        let error = store(&team, &planner.pane, "x").unwrap_err().to_string();
        assert!(error.contains("no open order"), "{error}");
        order(&task, &planner.id, "s1");
        assert_eq!(
            task.state()
                .unwrap()
                .open_order_of("planner-1")
                .unwrap()
                .forbidden,
            ["**"],
            "a senior's order forbids every file"
        );
        assert_eq!(
            store(&team, &planner.pane, "1. build the form\n").unwrap(),
            Stored::Execution { step: "s1".into() }
        );
        assert_eq!(read(&task, "s1").unwrap(), "1. build the form\n");
        let state = task.state().unwrap();
        assert!(
            state.approved,
            "an execution plan leaves the overall plan approved"
        );
        assert_eq!(state.plan("s1").unwrap().author, "planner-1");

        let executor = team.spawn_agent("executor", None).unwrap();
        let error = store(&team, &executor.pane, "x").unwrap_err().to_string();
        assert!(error.contains("only a senior"), "{error}");
    }
}
