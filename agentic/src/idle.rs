//! Closing agents the team no longer needs. An idle agent uses no tokens, but
//! its panel holds memory, and when it wakes hours later its whole
//! conversation is sent again at full price: a provider's cache lasts
//! minutes. So the dispatcher closes an agent that has had nothing open for
//! a while, after saving what it did as a handoff note in the team's memory;
//! a fresh agent reads the notes (memory_search) when work comes.
use crate::state::{Agent, OrderStatus, State, StepStatus};
use std::time::Duration;

/// How long a reviewer stays with nothing open: every review is
/// independent, so its memory of the last one helps no one.
pub const REVIEWER_IDLE: Duration = Duration::from_secs(5 * 60);

/// Whether `agent` has nothing open: no turn running, no order or message
/// waiting for it, no question it asked or must answer, no review it owes.
/// A writer also has no unmerged work: its worktree holds its last step
/// until that step is accepted or closed.
pub fn has_nothing_open(state: &State, agent: &Agent) -> bool {
    let busy = state.worker_turns.contains_key(&agent.id)
        || state
            .orders
            .iter()
            .any(|order| order.agent == agent.id && order.status == OrderStatus::Open)
        || state.queue.iter().any(|message| message.to == agent.id)
        || state.questions.iter().any(|question| {
            question.answer.is_none() && (question.from == agent.id || question.to == agent.id)
        })
        || state
            .reviews
            .iter()
            .any(|review| review.reviewer == agent.id && review.verdict.is_none());
    !busy && !(agent.writes && holds_unmerged_work(state, agent))
}

/// Its newest step is not accepted or closed yet: the work is still in its
/// worktree, waiting for review or a retry.
fn holds_unmerged_work(state: &State, agent: &Agent) -> bool {
    let Some(order) = state
        .orders
        .iter()
        .rev()
        .find(|order| order.agent == agent.id)
    else {
        return false;
    };
    !state.steps.iter().any(|step| {
        step.id == order.step && matches!(step.status, StepStatus::Accepted | StepStatus::Closed)
    })
}

/// How long `agent` may stay with nothing open before it is closed; None
/// when the task keeps its agents (`idle_close_minutes = 0`). A reviewer
/// goes sooner, never later than the task says.
pub fn allowed_idle(agent: &Agent, idle_close_minutes: u64) -> Option<Duration> {
    if idle_close_minutes == 0 {
        return None;
    }
    let configured = Duration::from_secs(idle_close_minutes * 60);
    Some(if agent.role.contains("reviewer") {
        configured.min(REVIEWER_IDLE)
    } else {
        configured
    })
}

/// The handoff note an agent leaves when it is closed: what it last did,
/// from the task's own records, so it costs no turn.
pub fn handoff(
    state: &State,
    agent: &Agent,
    idle: Duration,
    last_report: Option<&str>,
) -> crate::memory::Note {
    let last = state
        .orders
        .iter()
        .rev()
        .find(|order| order.agent == agent.id);
    let mut body = format!(
        "{} ({}) was closed after {} minutes with nothing open.",
        agent.id,
        agent.role,
        idle.as_secs() / 60
    );
    if let Some(order) = last {
        let status = state
            .steps
            .iter()
            .find(|step| step.id == order.step)
            .map_or("unknown".to_string(), |step| {
                format!("{:?}", step.status).to_lowercase()
            });
        body.push_str(&format!(
            " Its last step: {} ({status}), order {}.",
            order.step, order.id
        ));
    }
    if let Some(report) = last_report {
        body.push_str(&format!(" Its last report: {report}"));
    }
    crate::memory::Note {
        kind: crate::memory::NoteKind::Handoff,
        title: format!("{} closed while idle", agent.id),
        body,
        author: agent.id.clone(),
        step: last.map(|order| order.step.clone()),
        area: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Order, Step};

    fn agent(id: &str, role: &str, writes: bool) -> Agent {
        Agent {
            id: id.into(),
            role: role.into(),
            agent: "codex".into(),
            repo: "app".into(),
            session: String::new(),
            pane: "%9".into(),
            writes,
            group: None,
            lane: None,
            seat: 0,
            live: true,
            unavailable: false,
            ready: true,
            lost: false,
            usage: None,
            shows_references: false,
        }
    }

    fn order(id: &str, agent: &str, step: &str, status: OrderStatus) -> Order {
        Order {
            id: id.into(),
            step: step.into(),
            agent: agent.into(),
            text: String::new(),
            allowed: vec![],
            forbidden: vec![],
            check: None,
            base: String::new(),
            status,
            delivered: true,
            stalled: false,
            plan: None,
            read: vec![],
            targets: vec![],
        }
    }

    fn step(id: &str, status: StepStatus) -> Step {
        Step {
            id: id.into(),
            title: id.into(),
            status,
            attempts: 1,
            spec: None,
            review_since: None,
        }
    }

    #[test]
    fn an_agent_with_anything_open_stays() {
        let designer = agent("designer-4", "designer", false);
        let mut state = State::default();
        assert!(has_nothing_open(&state, &designer));

        state
            .orders
            .push(order("o1", "designer-4", "PLAN", OrderStatus::Open));
        assert!(!has_nothing_open(&state, &designer), "an open order");
        state.orders[0].status = OrderStatus::Closed;
        assert!(has_nothing_open(&state, &designer));

        state.queue.push(crate::state::Message {
            id: 1,
            to: "designer-4".into(),
            text: "next".into(),
            order: None,
            response_probe: false,
        });
        assert!(
            !has_nothing_open(&state, &designer),
            "a message waits for it"
        );
        let waiting = state.queue.remove(0);
        state.worker_turns.insert("designer-4".into(), waiting);
        assert!(!has_nothing_open(&state, &designer), "its turn runs");
    }

    #[test]
    fn a_writer_stays_while_its_worktree_holds_unmerged_work() {
        let hammer = agent("hammer-48", "hammer", true);
        let mut state = State::default();
        state
            .orders
            .push(order("o1", "hammer-48", "G1", OrderStatus::Closed));
        state.steps.push(step("G1", StepStatus::Reported));
        assert!(
            !has_nothing_open(&state, &hammer),
            "G1 waits for its review"
        );
        state.steps[0].status = StepStatus::Accepted;
        assert!(
            has_nothing_open(&state, &hammer),
            "merged: nothing left in its worktree"
        );
        state.steps[0].status = StepStatus::Failed;
        assert!(
            !has_nothing_open(&state, &hammer),
            "a failed step is retried there"
        );
        state.steps[0].status = StepStatus::Closed;
        assert!(has_nothing_open(&state, &hammer));
    }

    #[test]
    fn a_reviewer_goes_sooner_and_zero_keeps_everyone() {
        let reviewer = agent("code-reviewer-50", "code-reviewer", false);
        let designer = agent("designer-4", "designer", false);
        assert_eq!(
            allowed_idle(&designer, 30),
            Some(Duration::from_secs(30 * 60))
        );
        assert_eq!(allowed_idle(&reviewer, 30), Some(REVIEWER_IDLE));
        assert_eq!(
            allowed_idle(&reviewer, 2),
            Some(Duration::from_secs(2 * 60))
        );
        assert_eq!(allowed_idle(&designer, 0), None);
    }

    #[test]
    fn the_handoff_says_what_it_last_did() {
        let hammer = agent("hammer-48", "hammer", true);
        let mut state = State::default();
        state
            .orders
            .push(order("o7", "hammer-48", "G1", OrderStatus::Closed));
        state.steps.push(step("G1", StepStatus::Accepted));
        let note = handoff(
            &state,
            &hammer,
            Duration::from_secs(31 * 60),
            Some("G1 done: team read API"),
        );
        assert_eq!(note.kind, crate::memory::NoteKind::Handoff);
        assert_eq!(note.title, "hammer-48 closed while idle");
        assert_eq!(note.step.as_deref(), Some("G1"));
        assert_eq!(
            note.body,
            "hammer-48 (hammer) was closed after 31 minutes with nothing open. Its last step: G1 (accepted), order o7. Its last report: G1 done: team read API"
        );
    }
}
