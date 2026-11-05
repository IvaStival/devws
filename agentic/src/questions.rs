//! A factory-floor agent's question about its order: it goes to the senior
//! whose plan the order follows (the CTO when that senior is gone), and the
//! answer is typed back into the agent.

use crate::log::Kind;
use crate::roles::Group;
use crate::state::{LEADER, Question};
use crate::task::Task;
use anyhow::{Context, Result, bail};

/// Who answers.
pub enum Answerer<'a> {
    /// The agent in this pane.
    Pane(&'a str),
    Leader,
}

pub fn ask(task: &Task, pane: &str, text: &str) -> Result<String> {
    if text.trim().is_empty() {
        bail!("the question is empty");
    }
    task.transaction(|tx| {
        let agent = tx
            .state
            .agent_in_pane(pane)
            .with_context(|| format!("{pane} is not a team agent's pane"))?
            .clone();
        if agent.group() != Group::Floor {
            bail!("only the factory floor asks; a senior asks the leader in its report");
        }
        let order = tx
            .state
            .open_order_of(&agent.id)
            .with_context(|| format!("{} has no open order to ask about", agent.id))?
            .clone();
        if let Some(open) = tx.state.open_question_of(&agent.id) {
            bail!(
                "{} is still waiting for the answer to {}",
                agent.id,
                open.id
            );
        }
        let to = order
            .plan
            .as_deref()
            .and_then(|step| tx.state.plan(step))
            .map(|plan| plan.author.clone())
            .filter(|author| tx.state.agent(author).is_some_and(|senior| senior.live))
            .unwrap_or_else(|| LEADER.to_string());
        let id = format!("q{}", tx.state.next_number());
        let how = if to == LEADER {
            format!("Answer with the answer_question tool (question {id}).")
        } else {
            format!("Answer with: devagentics answer {id} \"<your answer>\"")
        };
        let about = order.plan.as_ref().map_or_else(String::new, |plan| {
            format!(", following your plan of step {plan}")
        });
        tx.state.say(
            &to,
            format!(
                "QUESTION {id} from {} on step {}{about}: {text}\n{how}",
                agent.id, order.step
            ),
        );
        tx.log(
            Kind::Question,
            Some(&agent.id),
            Some(&order.step),
            &format!("{id} to {to}: {text}"),
        );
        tx.state.questions.push(Question {
            id: id.clone(),
            from: agent.id.clone(),
            to: to.clone(),
            order: order.id.clone(),
            text: text.to_string(),
            answer: None,
        });
        Ok(format!(
            "Asked {to} ({id}). End your turn now; the answer will be typed in here."
        ))
    })
}

pub fn answer(task: &Task, by: Answerer, id: &str, text: &str) -> Result<String> {
    if text.trim().is_empty() {
        bail!("the answer is empty");
    }
    task.transaction(|tx| {
        let answerer = match by {
            Answerer::Leader => LEADER.to_string(),
            Answerer::Pane(pane) => tx
                .state
                .agent_in_pane(pane)
                .with_context(|| format!("{pane} is not a team agent's pane"))?
                .id
                .clone(),
        };
        let question = tx
            .state
            .questions
            .iter_mut()
            .find(|question| question.id == id)
            .with_context(|| format!("there is no question {id}"))?;
        if question.answer.is_some() {
            bail!("{id} is already answered");
        }
        // The leader may answer any question; an agent only its own.
        if answerer != LEADER && question.to != answerer {
            bail!("{id} was asked of {}, not {answerer}", question.to);
        }
        question.answer = Some(text.to_string());
        let from = question.from.clone();
        tx.state.say(
            &from,
            format!("ANSWER to {id} from {answerer}: {text}\nGo on with your order."),
        );
        tx.log(
            Kind::Answer,
            Some(&answerer),
            None,
            &format!("{id} to {from}: {text}"),
        );
        Ok(format!("The answer goes to {from}."))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::tests::started;
    use crate::orders::{OrderRequest, send_order};
    use crate::state::StepStatus;
    use crate::team::Team;
    use tempfile::TempDir;

    fn request(agent: &str, step: &str, plan: Option<&str>) -> OrderRequest {
        OrderRequest {
            agent: agent.into(),
            step: step.into(),
            title: step.into(),
            text: "do it".into(),
            allowed: vec![],
            forbidden: vec![],
            check: Some("true".into()),
            plan: plan.map(str::to_string),
        }
    }

    /// A planner whose plan of step p1 is accepted, and an executor ordered
    /// to follow it.
    fn ordered(
        dir: &TempDir,
    ) -> (
        crate::devws::fake::FakeDevws,
        Task,
        String,
        String,
        impl Drop,
    ) {
        let (home, devws, task, repo) = started(dir);
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        team.write_plan("1. p1\n").unwrap();
        team.approve().unwrap();
        send_order(&task, request(&planner.id, "p1", None)).unwrap();
        crate::plans::store(&team, &planner.pane, "1. the form\n").unwrap();
        task.transaction(|tx| {
            tx.state.step_mut("p1").unwrap().status = StepStatus::Accepted;
            tx.state.orders.iter_mut().for_each(|order| {
                order.status = crate::state::OrderStatus::Closed;
            });
            Ok(())
        })
        .unwrap();
        let executor = team.spawn_agent("executor", None).unwrap();
        send_order(&task, request(&executor.id, "s1", Some("p1"))).unwrap();
        (devws, task, planner.pane, executor.pane, repo)
    }

    #[test]
    fn a_question_goes_to_the_senior_who_planned_and_the_answer_comes_back() {
        let dir = TempDir::new().unwrap();
        let (_devws, task, planner_pane, executor_pane, _repo) = ordered(&dir);
        let said = ask(&task, &executor_pane, "Which colour?").unwrap();
        assert!(said.contains("Asked planner-1"), "{said}");
        let state = task.state().unwrap();
        let message = state.queue.last().unwrap();
        assert_eq!(message.to, "planner-1");
        assert!(
            message.text.contains("devagentics answer q"),
            "{}",
            message.text
        );
        let id = state.questions[0].id.clone();
        assert!(
            ask(&task, &executor_pane, "And size?")
                .unwrap_err()
                .to_string()
                .contains("still waiting")
        );

        assert!(
            answer(&task, Answerer::Pane(&executor_pane), &id, "blue")
                .unwrap_err()
                .to_string()
                .contains("was asked of planner-1")
        );
        answer(&task, Answerer::Pane(&planner_pane), &id, "blue").unwrap();
        let state = task.state().unwrap();
        let message = state.queue.last().unwrap();
        assert_eq!(message.to, "executor-1");
        assert!(message.text.contains("ANSWER to") && message.text.contains("blue"));
        assert!(state.open_question_of("executor-1").is_none());
        assert!(
            answer(&task, Answerer::Leader, &id, "x").is_err(),
            "answered once"
        );
    }

    #[test]
    fn with_the_senior_gone_the_cto_is_asked_and_may_answer() {
        let dir = TempDir::new().unwrap();
        let (_devws, task, _planner_pane, executor_pane, _repo) = ordered(&dir);
        task.transaction(|tx| {
            tx.state.agents[0].live = false;
            Ok(())
        })
        .unwrap();
        assert!(
            ask(&task, &executor_pane, "Which colour?")
                .unwrap()
                .contains("Asked leader")
        );
        let id = task.state().unwrap().questions[0].id.clone();
        assert!(
            task.state()
                .unwrap()
                .queue
                .last()
                .unwrap()
                .text
                .contains("answer_question")
        );
        answer(&task, Answerer::Leader, &id, "red").unwrap();
        assert_eq!(
            task.state().unwrap().questions[0].answer.as_deref(),
            Some("red")
        );
    }

    #[test]
    fn a_senior_does_not_ask() {
        let dir = TempDir::new().unwrap();
        let (_devws, task, planner_pane, _executor_pane, _repo) = ordered(&dir);
        assert!(
            ask(&task, &planner_pane, "?")
                .unwrap_err()
                .to_string()
                .contains("only the factory floor")
        );
    }
}
