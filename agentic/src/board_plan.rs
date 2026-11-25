//! The PLAN view's data: the plan's parts and their items in their own
//! words, so the codes the team works by (D4, D4-AT, D4-invariants-plan)
//! say what they build.
use crate::{
    execution::Contract,
    state::{OrderStatus, State, StepStatus},
};

/// A part of the plan (a milestone): `D`, its release and what it delivers.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanPart {
    pub id: String,
    pub version: String,
    pub title: String,
    pub items: Vec<PlanItem>,
}

impl PlanPart {
    /// Its items accepted (or closed), and all of them.
    pub fn progress(&self) -> (usize, usize) {
        let done = self.items.iter().filter(|item| item.is_done()).count();
        (done, self.items.len())
    }
}

/// An item of the plan, with the item that writes its tests first.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanItem {
    pub id: String,
    /// What it builds, without the plan's numbering: `Move reusable…`.
    pub title: String,
    /// None: not started yet.
    pub status: Option<StepStatus>,
    /// The agent working on it now.
    pub agent: Option<String>,
    pub behavior: Vec<String>,
    pub acceptance: Vec<String>,
    pub dependencies: Vec<String>,
    /// The item that writes its protected tests: `D4-AT`.
    pub tests: Option<PlanTests>,
    /// The open helper steps serving it: its plan, a diagnosis.
    pub helpers: Vec<Helper>,
}

impl PlanItem {
    pub fn is_done(&self) -> bool {
        matches!(self.status, Some(StepStatus::Accepted | StepStatus::Closed))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanTests {
    pub id: String,
    pub status: Option<StepStatus>,
    pub agent: Option<String>,
}

/// A step the leader added for an item: `D4-invariants-plan`.
#[derive(Clone, Debug, PartialEq)]
pub struct Helper {
    pub step: String,
    pub title: String,
    pub agent: Option<String>,
}

/// The parts in their order, each with its items: an item that writes
/// another's tests goes under it, not as a row of its own.
pub fn parts(state: &State) -> Vec<PlanPart> {
    let items = &state.application.items;
    let tested: Vec<&str> = items
        .values()
        .flat_map(|item| item.dependencies.iter())
        .filter(|dependency| {
            items
                .get(*dependency)
                .is_some_and(|found| found.writes_tests)
        })
        .map(String::as_str)
        .collect();
    state
        .application
        .milestones
        .iter()
        .map(|(id, milestone)| PlanPart {
            id: id.clone(),
            version: milestone.version.clone(),
            title: milestone.title.clone(),
            items: milestone
                .required
                .iter()
                .filter(|item| !tested.contains(&item.as_str()))
                .filter_map(|item| Some(plan_item(state, item, items.get(item)?)))
                .collect(),
        })
        .collect()
}

fn plan_item(state: &State, id: &str, contract: &Contract) -> PlanItem {
    let items = &state.application.items;
    let tests = contract
        .dependencies
        .iter()
        .find(|dependency| {
            items
                .get(*dependency)
                .is_some_and(|found| found.writes_tests)
        })
        .map(|tests| PlanTests {
            id: tests.clone(),
            status: status(state, tests),
            agent: working_on(state, tests),
        });
    PlanItem {
        id: id.to_string(),
        title: plain_title(id, &contract.title),
        status: status(state, id),
        agent: working_on(state, id),
        behavior: contract.behavior.clone(),
        acceptance: contract.acceptance.clone(),
        dependencies: contract.dependencies.clone(),
        helpers: helpers(state, id),
        tests,
    }
}

/// The item a step serves: an item itself, or the longest item id its own
/// id starts with (`D4-invariants-plan` → `D4`), and that item's title.
pub fn item_of<'s>(state: &'s State, step: &str) -> Option<(&'s str, &'s Contract)> {
    let items = &state.application.items;
    if let Some((id, contract)) = items.get_key_value(step) {
        return Some((id, contract));
    }
    items
        .iter()
        .filter(|(id, _)| {
            step.strip_prefix(id.as_str())
                .is_some_and(|rest| rest.starts_with('-'))
        })
        .max_by_key(|(id, _)| id.len())
        .map(|(id, contract)| (id.as_str(), contract))
}

/// The item a step works for when it is not that item itself, in plain
/// words: a helper's item (`D4-invariants-plan` → D4), or the item whose
/// tests a test item writes (`D4-AT` → D4). An agent's "now" line names it.
pub fn serves(state: &State, step: &str) -> Option<(String, String)> {
    let items = &state.application.items;
    if items.get(step).is_some_and(|item| item.writes_tests) {
        return items
            .iter()
            .find(|(_, item)| {
                item.dependencies
                    .iter()
                    .any(|dependency| dependency == step)
            })
            .map(|(id, item)| (id.clone(), plain_title(id, &item.title)));
    }
    if items.contains_key(step) {
        return None;
    }
    item_of(state, step).map(|(id, item)| (id.to_string(), plain_title(id, &item.title)))
}

/// `17. D4 move reusable…` → `Move reusable…`: the plan numbers its items
/// and repeats their ids, which the row already shows.
pub fn plain_title(id: &str, title: &str) -> String {
    let title = title.trim();
    let unnumbered = title
        .split_once(". ")
        .filter(|(number, _)| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
        .map_or(title, |(_, rest)| rest);
    // Its id, or the group its id belongs to: `B1` of `B1-1`.
    let without_id = unnumbered
        .split_once(' ')
        .filter(|(word, _)| id == *word || id.starts_with(&format!("{word}-")))
        .map_or(unnumbered, |(_, rest)| rest.trim_start());
    let mut characters = without_id.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

fn status(state: &State, step: &str) -> Option<StepStatus> {
    state
        .steps
        .iter()
        .find(|found| found.id == step)
        .map(|found| found.status)
}

fn working_on(state: &State, step: &str) -> Option<String> {
    state
        .orders
        .iter()
        .rev()
        .find(|order| order.step == step && order.status == OrderStatus::Open)
        .map(|order| order.agent.clone())
}

fn helpers(state: &State, item: &str) -> Vec<Helper> {
    state
        .steps
        .iter()
        .filter(|step| matches!(step.status, StepStatus::Ordered | StepStatus::Reported))
        .filter(|step| !state.application.items.contains_key(&step.id))
        .filter(|step| item_of(state, &step.id).is_some_and(|(id, _)| id == item))
        .map(|step| Helper {
            step: step.id.clone(),
            title: step.title.clone(),
            agent: working_on(state, &step.id),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Order, Step};

    fn contract(title: &str, dependencies: &[&str], writes_tests: bool) -> Contract {
        Contract {
            title: title.into(),
            dependencies: dependencies.iter().map(|id| id.to_string()).collect(),
            writes_tests,
            behavior: vec![format!("does {title}")],
            ..Contract::default()
        }
    }

    fn step(id: &str, status: StepStatus) -> Step {
        Step {
            id: id.into(),
            title: format!("title of {id}"),
            status,
            attempts: 1,
            spec: None,
            review_since: None,
        }
    }

    fn order(id: &str, step: &str, agent: &str) -> Order {
        Order {
            id: id.into(),
            step: step.into(),
            agent: agent.into(),
            text: String::new(),
            allowed: Vec::new(),
            forbidden: Vec::new(),
            check: None,
            base: String::new(),
            status: OrderStatus::Open,
            delivered: true,
            stalled: false,
            plan: None,
            read: Vec::new(),
            targets: Vec::new(),
        }
    }

    /// Part D of the system-observer task, cut down: D3 done, D4 being
    /// planned, its tests not started.
    fn state() -> State {
        let mut state = State::default();
        let items = &mut state.application.items;
        items.insert(
            "D3-AT".into(),
            contract("Acceptance test first for D3", &[], true),
        );
        items.insert(
            "D3".into(),
            contract("16. D3 SystemFlowStage", &["D3-AT"], false),
        );
        items.insert(
            "D4-AT".into(),
            contract("Acceptance test first for D4", &["D3"], true),
        );
        items.insert(
            "D4".into(),
            contract("17. D4 move reusable inspectors", &["D3", "D4-AT"], false),
        );
        state.application.milestones.insert(
            "D".into(),
            serde_json::from_value(serde_json::json!({
                "version": "1.134.0",
                "title": "Ledger Live/History workspaces",
                "required": ["D3-AT", "D4-AT", "D3", "D4"],
            }))
            .unwrap(),
        );
        state.steps = vec![
            step("D3-AT", StepStatus::Accepted),
            step("D3", StepStatus::Accepted),
            step("D4-invariants-plan", StepStatus::Ordered),
            step("D3-review-clarification", StepStatus::Closed),
        ];
        state.orders = vec![order("o1", "D4-invariants-plan", "planner-20")];
        state
    }

    #[test]
    fn a_part_lists_its_items_in_plain_words_with_their_tests_under_them() {
        let parts = parts(&state());

        assert_eq!(parts.len(), 1);
        let part = &parts[0];
        assert_eq!((part.id.as_str(), part.version.as_str()), ("D", "1.134.0"));
        let ids: Vec<_> = part.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["D3", "D4"], "a test item goes under its item");
        assert_eq!(part.progress(), (1, 2));

        let d4 = &part.items[1];
        assert_eq!(d4.title, "Move reusable inspectors");
        assert_eq!(d4.status, None, "not started");
        assert_eq!(d4.behavior, ["does 17. D4 move reusable inspectors"]);
        assert_eq!(
            d4.tests,
            Some(PlanTests {
                id: "D4-AT".into(),
                status: None,
                agent: None
            })
        );
        assert_eq!(
            d4.helpers,
            [Helper {
                step: "D4-invariants-plan".into(),
                title: "title of D4-invariants-plan".into(),
                agent: Some("planner-20".into()),
            }],
            "its plan, being written, and who writes it"
        );
        assert!(
            part.items[0].helpers.is_empty(),
            "a closed helper is not shown"
        );
    }

    #[test]
    fn a_helper_step_names_the_item_it_serves() {
        let state = state();
        assert_eq!(
            item_of(&state, "D4-invariants-plan").map(|(id, _)| id),
            Some("D4")
        );
        assert_eq!(item_of(&state, "D4-AT").map(|(id, _)| id), Some("D4-AT"));
        assert_eq!(item_of(&state, "D40-plan"), None, "D4 is not D40's prefix");
        assert_eq!(item_of(&state, "runner-php-image"), None);
        let d4 = Some(("D4".to_string(), "Move reusable inspectors".to_string()));
        assert_eq!(serves(&state, "D4-invariants-plan"), d4);
        assert_eq!(serves(&state, "D4-AT"), d4, "its tests serve D4");
        assert_eq!(serves(&state, "D4"), None, "an item is itself");
        assert_eq!(serves(&state, "runner-php-image"), None);
    }

    #[test]
    fn a_title_loses_the_plans_numbering_and_repeated_id() {
        assert_eq!(
            plain_title("D4", "17. D4 move reusable evidence inspectors"),
            "Move reusable evidence inspectors"
        );
        assert_eq!(
            plain_title("E2", "E2 remove dead routes"),
            "Remove dead routes"
        );
        assert_eq!(
            plain_title("B1-1", "B1 systems registry schemas"),
            "Systems registry schemas",
            "the group its id is in"
        );
        assert_eq!(
            plain_title("D4", "D40 is another item"),
            "D40 is another item",
            "only a whole word"
        );
        assert_eq!(
            plain_title("V1", "Final browser acceptance"),
            "Final browser acceptance"
        );
        assert_eq!(plain_title("X", ""), "");
    }
}
