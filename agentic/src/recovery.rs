//! Durable interrupt intent and resumption, independent of pane lifetime.
use crate::{
    devws::{Devws, PaneState},
    orders,
    state::{LEADER, Recovery},
    task::Task,
};
use anyhow::{Result, bail};

pub fn interrupt(task: &Task, devws: &dyn Devws, invoking: Option<&str>) -> Result<()> {
    let state = task.state()?;
    let mut targets: Vec<_> = state
        .agents
        .iter()
        .filter(|agent| agent.live && !agent.lost)
        .map(|agent| (agent.id.clone(), agent.pane.clone()))
        .collect();
    if let Some(pane) = devws.pane_with_role(&task.config()?.workspace, "agent")? {
        targets.push((LEADER.into(), pane));
    }
    let mut failures = Vec::new();
    for (id, pane) in targets {
        if invoking == Some(pane.as_str()) {
            continue;
        }
        if state
            .recovery
            .get(&id)
            .is_some_and(|entry| entry.interrupted)
        {
            continue;
        }
        let current = match devws.pane_state(&pane) {
            Ok(current) => current,
            Err(error) => {
                failures.push(format!("{id}: {error}"));
                continue;
            }
        };
        if !matches!(current, PaneState::Working | PaneState::Waiting) {
            continue;
        }
        // Persist intent first: a crash after the terminal action is recoverable.
        task.transaction(|tx| {
            tx.state.recovery.insert(
                id.clone(),
                Recovery {
                    pane: pane.clone(),
                    interrupted: false,
                    error: Some("interruption pending; inspect before retry".into()),
                },
            );
            Ok(())
        })?;
        let result = devws.interrupt(&pane);
        task.transaction(|tx| {
            let entry = tx.state.recovery.get_mut(&id).unwrap();
            entry.interrupted = result.is_ok();
            entry.error = result.as_ref().err().map(ToString::to_string);
            Ok(())
        })?;
        if let Err(error) = result {
            failures.push(format!("{id}: {error}"));
        }
    }
    if !failures.is_empty() {
        bail!("task remains paused: {}", failures.join("; "));
    }
    Ok(())
}

pub fn queue_resumption(task: &Task, devws: &dyn Devws) -> Result<()> {
    let state = task.state()?;
    for (id, entry) in &state.recovery {
        if state.agent(id).is_some_and(|agent| !agent.live) {
            continue;
        }
        let pane = state
            .agent(id)
            .filter(|agent| agent.live)
            .map(|agent| agent.pane.as_str())
            .unwrap_or(&entry.pane);
        if !matches!(devws.pane_state(pane)?, PaneState::Done | PaneState::Idle) {
            bail!("task remains paused: {id} is not ready to resume");
        }
    }
    task.transaction(|tx| {
        for id in state.recovery.keys() {
            if tx.state.agent(id).is_some_and(|agent| !agent.live) {
                tx.state.recovery.remove(id);
                continue;
            }
            tx.state.queue.retain(|message| {
                &message.to != id || (id == LEADER && !message.text.starts_with("RESUME"))
            });
            if id == LEADER {
                tx.state.say(
                    LEADER,
                    "RESUME: Read task_status and continue the authoritative current work.".into(),
                );
            } else if let Some(order) = tx.state.open_order_of(id).cloned() {
                let title = tx
                    .state
                    .steps
                    .iter()
                    .find(|step| step.id == order.step)
                    .map(|step| step.title.as_str())
                    .unwrap_or(&order.step);
                let text = orders::order_message(
                    &order,
                    title,
                    tx.state
                        .order_profiles
                        .get(&order.id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                );
                tx.state.say_about(id, text, Some(order.id.clone()));
                if let Some(current) = tx
                    .state
                    .orders
                    .iter_mut()
                    .find(|current| current.id == order.id)
                {
                    current.delivered = false;
                    current.stalled = false;
                }
            } else {
                tx.state.recovery.remove(id);
            }
        }
        Ok(())
    })
}
