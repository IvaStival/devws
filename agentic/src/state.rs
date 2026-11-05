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
    /// New deliveries and spawns wait.
    pub paused: bool,
    /// The last number handed out for an agent, an order or a message.
    pub next_id: u64,
    pub agents: Vec<Agent>,
    pub steps: Vec<Step>,
    pub orders: Vec<Order>,
    /// What waits to be typed into an agent's pane (the dispatcher does it
    /// when the agent is done or idle).
    pub queue: Vec<Message>,
    /// The seniors' execution plans, one per step they were ordered.
    pub plans: Vec<ExecutionPlan>,
    pub questions: Vec<Question>,
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
