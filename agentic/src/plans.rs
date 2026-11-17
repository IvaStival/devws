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
    let step = store_execution(team.task, &agent.id, text)?;
    Ok(Stored::Execution { step })
}

/// Stores a senior's execution plan for the step of its open order; says
/// which step. A reviewed planner reaches it through its worker tools.
pub fn store_execution(task: &Task, agent: &str, text: &str) -> Result<String> {
    let state = task.state()?;
    let order = state.open_order_of(agent).with_context(|| {
        format!("{agent} has no open order: an execution plan is for the step it was ordered")
    })?;
    if text.trim().is_empty() {
        bail!("the plan is empty");
    }
    let (step, author) = (order.step.clone(), agent.to_string());
    let file = path(task, &step);
    std::fs::create_dir_all(file.parent().unwrap())?;
    let temporary = file.with_extension("tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, &file)?;
    task.transaction(|tx| {
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
    Ok(step)
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

    #[test]
    fn a_reviewed_planner_stores_the_execution_plan_of_its_order() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        team.write_plan("1. s1\n").unwrap();
        team.approve().unwrap();
        let error = store_execution(&task, &planner.id, "x")
            .unwrap_err()
            .to_string();
        assert!(error.contains("no open order"), "{error}");
        order(&task, &planner.id, "s1");
        assert_eq!(
            store_execution(&task, &planner.id, "1. manifest\n").unwrap(),
            "s1"
        );
        assert_eq!(read(&task, "s1").unwrap(), "1. manifest\n");
        assert!(
            store_execution(&task, &planner.id, "  ").is_err(),
            "an empty plan"
        );
    }

    #[test]
    fn a_worker_reads_the_plan_its_order_follows() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        team.write_plan("1. s1\n").unwrap();
        team.approve().unwrap();
        order(&task, &planner.id, "s1");
        store_execution(&task, &planner.id, "1. write the manifest\n").unwrap();
        let reader = planner.id.clone();
        let read = crate::worker_tools::call(
            &task,
            &reader,
            "plan_read",
            &serde_json::json!({"step": "s1"}),
        )
        .unwrap();
        assert_eq!(read["plan"], "1. write the manifest\n");
        let missing = crate::worker_tools::call(
            &task,
            &reader,
            "plan_read",
            &serde_json::json!({"step": "nope"}),
        );
        assert!(
            missing
                .unwrap_err()
                .to_string()
                .contains("no execution plan")
        );
    }

    #[test]
    fn a_planner_reads_the_baseline_in_pieces_and_amends_it() {
        let dir = TempDir::new().unwrap();
        let (home, devws, task, _repo) = started(&dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        crate::execution::store(&task, crate::execution::tests::baseline(), "first").unwrap();
        crate::execution::confirm(&task, 1, false).unwrap();
        crate::execution::confirm(&task, 1, true).unwrap();
        let planner = team.spawn_agent("planner", None).unwrap();
        let call = |action: &str, input: serde_json::Value| {
            crate::worker_tools::call(&task, &planner.id, action, &input)
        };
        let outline = call("application_read", serde_json::json!({})).unwrap();
        assert_eq!(outline["items"]["S01-B01"]["title"], "Token service");
        assert!(outline["items"]["S01-B01"].get("acceptance").is_none());
        let full = call(
            "application_read",
            serde_json::json!({"items": ["S01-B01"]}),
        )
        .unwrap();
        assert_eq!(
            full["items"]["S01-B01"]["acceptance"][0],
            "Service contract passes"
        );

        let amended = call(
            "plan_amend",
            serde_json::json!({"reason": "narrow goal", "changes": {"goal": "Accounts only"}}),
        )
        .unwrap();
        assert_eq!(amended["proposed"], true);
        let proposal = task.state().unwrap().application_proposal.unwrap();
        assert_eq!(
            (proposal.revision, proposal.goal.as_str()),
            (2, "Accounts only")
        );
        assert_eq!(proposal.items, crate::execution::tests::baseline().items);
        let pending = call("application_read", serde_json::json!({"proposal": true})).unwrap();
        assert_eq!(pending["goal"], "Accounts only");

        let executor = team.spawn_agent("executor", None).unwrap();
        for action in ["application_read", "plan_amend"] {
            let refused = crate::worker_tools::call(
                &task,
                &executor.id,
                action,
                &serde_json::json!({"reason": "x", "changes": {}}),
            );
            assert!(refused.is_err(), "{action} is not a writer's");
        }
    }
}
