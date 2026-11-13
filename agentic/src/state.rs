use crate::roles::Group;
use serde::{Deserialize, Serialize};

/// The recipient of what is said to the leader.
pub const LEADER: &str = "leader";

/// What changes while a task runs, kept in `state.json` and only ever changed
/// through `Task::transaction`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// The user has approved the plan: agents that write may start.
    pub approved: bool,
    pub order_profiles: std::collections::BTreeMap<String, Vec<String>>,
    pub order_revisions: std::collections::BTreeMap<String, u64>,
    pub amendments: Vec<Amendment>,
    pub check_evidence: std::collections::BTreeMap<String, crate::check_profiles::Evidence>,
    pub recovery: std::collections::BTreeMap<String, Recovery>,
    /// Exact MCP calls awaiting or holding a one-use user authorization.
    pub permissions: Vec<PermissionRequest>,
    /// New deliveries and spawns wait.
    pub paused: bool,
    /// The last number handed out for an agent, an order or a message.
    pub next_id: u64,
    pub agents: Vec<Agent>,
    /// Snapshotted worker candidates and attempts, independent of pane lifetime.
    pub worker_launches: std::collections::BTreeMap<String, crate::worker::LaunchState>,
    pub steps: Vec<Step>,
    pub orders: Vec<Order>,
    /// What waits to be typed into an agent's pane (the dispatcher does it
    /// when the agent is done or idle).
    pub queue: Vec<Message>,
    /// The seniors' execution plans, one per step they were ordered.
    pub plans: Vec<ExecutionPlan>,
    pub questions: Vec<Question>,
    /// The devws agent the leader runs on now: its role's, or its fallback's
    /// (leader.rs). Tasks from before have none: the role's.
    pub leader_agent: Option<String>,
    /// A move of the leader to its other program that was asked for, with
    /// why: the board makes it once the leader's turn is over (leader.rs).
    pub leader_switch: Option<String>,
    /// The devws agent the user picked to lead when the team was opened; the
    /// leader's role then runs on it (leader.rs `led_by`). None: the role's.
    pub leader_pick: Option<String>,
}

/// A senior's plan for the floor, kept as `plans/<step>.md`. The CTO approves
/// it by accepting its step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub step: String,
    pub author: String,
}

/// A floor agent's question about its order, for the senior who planned it
/// (or the CTO).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub from: String,
    /// An agent's id, or `LEADER`.
    pub to: String,
    pub order: String,
    pub text: String,
    pub answer: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub role: String,
    /// The devws agent name it runs as (`claude`, `claude_work`…).
    pub agent: String,
    pub repo: String,
    /// The devws session whose panel it is.
    pub session: String,
    pub pane: String,
    /// Whether its role changes the repo.
    pub writes: bool,
    /// Its role's group; tasks from before groups have none.
    #[serde(default)]
    pub group: Option<Group>,
    /// The lane it writes in, when the task has lanes.
    #[serde(default)]
    pub lane: Option<String>,
    /// Its panel is there.
    pub live: bool,
    /// Startup alternatives were exhausted or rejected; explicit resume may retry.
    #[serde(default)]
    pub unavailable: bool,
    /// It has answered its first message: orders may be typed into it.
    pub ready: bool,
    /// Its panel went (a restart, a closed pane); the leader has been told.
    #[serde(default)]
    pub lost: bool,
    /// What it had used when it stopped (while it runs, devws is asked).
    #[serde(default)]
    pub usage: Option<crate::devws::Usage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// An agent is working on it.
    Ordered,
    /// Done and checked: the leader reviews it.
    Reported,
    /// Done, but the tree, the files or the check were not right.
    Failed,
    /// The agent could not go on.
    Blocked,
    /// The leader sent it back.
    Rejected,
    Accepted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub title: String,
    pub status: StepStatus,
    /// How many orders it has had.
    pub attempts: u32,
    /// The spec or plan part it belongs to (`H2f`, `1.83.0`): the board
    /// shows finished work by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    /// When it started waiting for the leader (reported, failed, blocked or
    /// sent back), in epoch seconds. Unknown for steps saved before this was
    /// kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_since: Option<u64>,
}

impl Step {
    /// Moves the step on; a status the leader has to act on is stamped with
    /// `now`, so the board can tell a review the leader left behind.
    pub fn set_status(&mut self, status: StepStatus, now: u64) {
        let waits_for_leader = matches!(
            status,
            StepStatus::Reported | StepStatus::Failed | StepStatus::Blocked | StepStatus::Rejected
        );
        self.review_since = waits_for_leader.then_some(now);
        self.status = status;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Open,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub step: String,
    pub agent: String,
    pub text: String,
    /// Globs of the files it may change; none means any.
    pub allowed: Vec<String>,
    pub forbidden: Vec<String>,
    /// The command that decides it is done; the repo's by default.
    pub check: Option<String>,
    /// The commit its work is measured from.
    pub base: String,
    pub status: OrderStatus,
    /// The agent has been told.
    #[serde(default)]
    pub delivered: bool,
    /// The agent finished a turn with no report and the leader was told.
    #[serde(default)]
    pub stalled: bool,
    /// The step whose execution plan it follows (orders to the floor).
    #[serde(default)]
    pub plan: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: u64,
    /// An agent's id, or `LEADER`.
    pub to: String,
    pub text: String,
    /// The order this message gives, once it is delivered the order is.
    #[serde(default)]
    pub order: Option<String>,
}

impl Agent {
    pub fn group(&self) -> Group {
        self.group.unwrap_or(if self.writes {
            Group::Floor
        } else {
            Group::Senior
        })
    }
}

impl State {
    pub fn agent(&self, id: &str) -> Option<&Agent> {
        self.agents.iter().find(|agent| agent.id == id)
    }

    pub fn agent_in_pane(&self, pane: &str) -> Option<&Agent> {
        self.agents
            .iter()
            .find(|agent| agent.live && agent.pane == pane)
    }

    pub fn open_order_of(&self, agent: &str) -> Option<&Order> {
        self.orders
            .iter()
            .find(|order| order.agent == agent && order.status == OrderStatus::Open)
    }

    pub fn plan(&self, step: &str) -> Option<&ExecutionPlan> {
        self.plans.iter().find(|plan| plan.step == step)
    }

    pub fn open_question_of(&self, agent: &str) -> Option<&Question> {
        self.questions
            .iter()
            .find(|question| question.from == agent && question.answer.is_none())
    }

    pub fn step_mut(&mut self, id: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|step| step.id == id)
    }

    pub fn next_number(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Something for an agent or the leader to be told.
    pub fn say(&mut self, to: &str, text: String) {
        self.say_about(to, text, None);
    }

    /// The message that gives an order.
    pub fn say_about(&mut self, to: &str, text: String, order: Option<String>) {
        let id = self.next_number();
        self.queue.push(Message {
            id,
            to: to.to_string(),
            text,
            order,
        });
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Amendment {
    pub order: String,
    pub revision: u64,
    pub allowed: Vec<String>,
    pub reason: String,
    pub source: String,
}

impl State {
    pub fn revision(&self, order: &str) -> u64 {
        self.order_revisions.get(order).copied().unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recovery {
    pub pane: String,
    pub interrupted: bool,
    pub error: Option<String>,
}

/// A request to run one specific MCP invocation outside the system effect tier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub tool: String,
    pub arguments: serde_json::Value,
    pub effect: Effect,
    pub status: PermissionStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    System,
    External,
    Destructive,
}

impl Effect {
    pub fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::External => "external",
            Self::Destructive => "destructive",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    Pending,
    Authorized,
    Denied,
    Consumed,
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[test]
    fn legacy_orders_load_without_execution_metadata() {
        let state: State = serde_json::from_value(serde_json::json!({"approved":true,"orders":[{
            "id":"o1", "step":"s1", "agent":"executor-1", "text":"work", "allowed":[], "forbidden":[], "check":null,"base":"abc","status":"open"
        }]})).unwrap();
        assert_eq!(state.revision("o1"), 0);
        let step: Step = serde_json::from_value(serde_json::json!({
            "id":"s1", "title":"t", "status":"accepted", "attempts":1
        }))
        .unwrap();
        assert_eq!(step.spec, None, "a step from before specs has none");
        assert!(!serde_json::to_string(&step).unwrap().contains("spec"));
        assert!(
            state.recovery.is_empty()
                && state.check_evidence.is_empty()
                && state.amendments.is_empty()
                && state.permissions.is_empty()
        );
        let encoded = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<State>(&encoded).unwrap(), state);
        let config: crate::task::TaskConfig =
            toml::from_str("id = 't'\nbranch = 'feat/x'\nworkspace = 'dev-x'\n").unwrap();
        assert!(config.check_profiles.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(status: StepStatus) -> Step {
        Step {
            id: "s1".into(),
            title: "title".into(),
            status,
            attempts: 1,
            review_since: None,
            spec: None,
        }
    }

    #[test]
    fn a_step_waiting_for_the_leader_remembers_since_when() {
        for status in [
            StepStatus::Reported,
            StepStatus::Failed,
            StepStatus::Blocked,
            StepStatus::Rejected,
        ] {
            let mut waiting = step(StepStatus::Ordered);
            waiting.set_status(status, 100);
            assert_eq!(waiting.review_since, Some(100), "{status:?}");
        }
    }

    #[test]
    fn a_step_out_of_review_forgets_it() {
        for status in [StepStatus::Ordered, StepStatus::Accepted] {
            let mut moved = step(StepStatus::Reported);
            moved.review_since = Some(100);
            moved.set_status(status, 200);
            assert_eq!(moved.review_since, None, "{status:?}");
        }
    }

    #[test]
    fn a_step_saved_before_the_field_loads() {
        let loaded: Step =
            serde_json::from_str(r#"{"id":"ph","title":"plan","status":"reported","attempts":2}"#)
                .unwrap();
        assert_eq!(loaded.review_since, None);
    }
}
