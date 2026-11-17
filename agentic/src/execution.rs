//! Versioned application decisions, work contracts and milestone readiness.
use crate::{
    log::Kind,
    state::{State, StepStatus},
    task::Task,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const STANDARDS_VERSION: &str = "1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Workflow {
    #[default]
    Legacy,
    Reviewed,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Application {
    pub revision: u64,
    pub goal: String,
    pub non_goals: Vec<String>,
    pub decisions: Vec<Decision>,
    pub scopes: BTreeMap<String, String>,
    pub items: BTreeMap<String, Contract>,
    pub milestones: BTreeMap<String, Milestone>,
    pub check_configuration: BTreeMap<String, crate::check_profiles::RepositoryChecks>,
    pub confirmed_revision: Option<u64>,
    pub approved_revision: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    pub round: u64,
    pub question: String,
    pub recommendation: String,
    pub prerequisites: Vec<String>,
    pub answer: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Contract {
    pub scope: String,
    pub title: String,
    pub role: String,
    pub repo: String,
    pub lane: String,
    pub dependencies: Vec<String>,
    pub allowed: Vec<String>,
    pub supporting_files: Vec<String>,
    pub reuse: Vec<String>,
    pub interfaces: Vec<String>,
    pub behavior: Vec<String>,
    pub errors: Vec<String>,
    pub tests: Vec<String>,
    pub profiles: Vec<String>,
    pub acceptance: Vec<String>,
    pub non_goals: Vec<String>,
    pub questions: Vec<String>,
    pub standards: String,
    pub data_effects: Vec<String>,
    /// Test files (globs) that judge this item: an implementation may read
    /// them but not change them, so it cannot pass by weakening its tests.
    pub protected_tests: Vec<String>,
    /// This item writes the protected tests (of the items that depend on it).
    pub writes_tests: bool,
}

impl Contract {
    /// What an order for this item may not change: the leader's `requested`
    /// globs, and, unless the item writes them, its protected tests.
    pub fn forbidden_for_order(&self, requested: &[String]) -> Vec<String> {
        let mut forbidden = requested.to_vec();
        if self.writes_tests {
            return forbidden;
        }
        for glob in &self.protected_tests {
            if !forbidden.contains(glob) {
                forbidden.push(glob.clone());
            }
        }
        forbidden
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Milestone {
    pub version: String,
    pub title: String,
    pub required: Vec<String>,
    pub dependencies: Vec<String>,
    /// A release is never inferred from numbering, acceptance or a draft PR.
    pub release_record: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub previous: Application,
    pub reason: String,
    pub affected: Vec<String>,
}

fn text(value: &str, name: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "{name} must not be empty");
    Ok(())
}
fn identifier(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-'),
        "invalid identifier: {value}"
    );
    Ok(())
}
fn paths(values: &[String]) -> Result<()> {
    for value in values {
        ensure!(
            !value.starts_with('/')
                && !value.contains('\\')
                && !value.split('/').any(|part| part == "..")
                && !value.trim().is_empty(),
            "path must stay in the project: {value}"
        );
    }
    crate::checks::glob_set(values)?;
    Ok(())
}
fn acyclic(graph: &BTreeMap<String, Vec<String>>) -> Result<()> {
    fn visit(
        name: &str,
        graph: &BTreeMap<String, Vec<String>>,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        ensure!(active.insert(name.into()), "dependency cycle at {name}");
        for dependency in graph.get(name).context("unknown dependency")? {
            ensure!(
                graph.contains_key(dependency),
                "unknown dependency {dependency}"
            );
            visit(dependency, graph, active, done)?;
        }
        active.remove(name);
        done.insert(name.into());
        Ok(())
    }
    let (mut active, mut done) = (BTreeSet::new(), BTreeSet::new());
    for name in graph.keys() {
        visit(name, graph, &mut active, &mut done)?;
    }
    Ok(())
}
impl Application {
    pub fn validate(&self) -> Result<()> {
        text(&self.goal, "application goal")?;
        ensure!(self.revision > 0, "baseline revision must be positive");
        let decision_graph = self
            .decisions
            .iter()
            .map(|decision| (decision.id.clone(), decision.prerequisites.clone()))
            .collect::<BTreeMap<_, _>>();
        ensure!(
            decision_graph.len() == self.decisions.len(),
            "duplicate decision IDs"
        );
        acyclic(&decision_graph)?;
        for decision in &self.decisions {
            identifier(&decision.id)?;
            text(&decision.question, "question")?;
            text(&decision.recommendation, "recommendation")?;
            if let Some(answer) = &decision.answer {
                text(answer, "answer")?;
            }
        }
        ensure!(!self.scopes.is_empty(), "application needs scopes");
        for (id, title) in &self.scopes {
            identifier(id)?;
            text(title, "scope title")?;
        }
        for (id, item) in &self.items {
            identifier(id)?;
            ensure!(self.scopes.contains_key(&item.scope), "{id}: unknown scope");
            for (name, value) in [
                ("title", &item.title),
                ("role", &item.role),
                ("repo", &item.repo),
                ("lane", &item.lane),
            ] {
                text(value, name)?;
            }
            ensure!(
                item.standards == STANDARDS_VERSION,
                "{id}: unknown coding standards version"
            );
            for (name, values) in [
                ("allowed", &item.allowed),
                ("reuse", &item.reuse),
                ("interfaces", &item.interfaces),
                ("behavior", &item.behavior),
                ("errors", &item.errors),
                ("tests", &item.tests),
                ("profiles", &item.profiles),
                ("acceptance", &item.acceptance),
            ] {
                ensure!(
                    !values.is_empty() && values.iter().all(|value| !value.trim().is_empty()),
                    "{id}: missing {name}"
                );
            }
            let checks = self
                .check_configuration
                .get(&item.repo)
                .context("baseline must declare required named check definitions")?;
            crate::check_profiles::validate(checks)?;
            ensure!(
                item.profiles
                    .iter()
                    .all(|profile| checks.checks.contains_key(profile)),
                "baseline check definitions are incomplete"
            );
            paths(&item.allowed)?;
            paths(&item.supporting_files)?;
            paths(&item.protected_tests)?;
            self.validate_protected_tests(id, item)?;
        }
        acyclic(
            &self
                .items
                .iter()
                .map(|(id, item)| (id.clone(), item.dependencies.clone()))
                .collect(),
        )?;
        let mut versions = BTreeSet::new();
        for (id, milestone) in &self.milestones {
            identifier(id)?;
            text(&milestone.title, "milestone title")?;
            let parts = milestone
                .version
                .split('.')
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ensure!(
                parts.len() == 3 && versions.insert(milestone.version.clone()),
                "duplicate or invalid milestone version"
            );
            ensure!(
                !milestone.required.is_empty(),
                "milestone requires implementation steps"
            );
            for item in &milestone.required {
                ensure!(
                    self.items.contains_key(item),
                    "unknown milestone item {item}"
                );
            }
        }
        ensure!(
            !self.items.is_empty() && !self.milestones.is_empty(),
            "baseline needs steps and milestones"
        );
        for scope in self.scopes.keys() {
            ensure!(
                self.items.values().any(|item| &item.scope == scope),
                "scope {scope} has no execution items"
            );
        }
        for item in self.items.keys() {
            ensure!(
                self.milestones
                    .values()
                    .any(|milestone| milestone.required.contains(item)),
                "item {item} has no milestone"
            );
        }
        acyclic(
            &self
                .milestones
                .iter()
                .map(|(id, milestone)| (id.clone(), milestone.dependencies.clone()))
                .collect(),
        )?;
        Ok(())
    }
    /// A test-writing item can write its tests; an item judged by protected
    /// tests depends on the item that writes them, so they land first.
    fn validate_protected_tests(&self, id: &str, item: &Contract) -> Result<()> {
        if item.writes_tests || item.protected_tests.is_empty() {
            return Ok(());
        }
        let writers: Vec<&Contract> = item
            .dependencies
            .iter()
            .filter_map(|dependency| self.items.get(dependency))
            .filter(|dependency| dependency.writes_tests)
            .collect();
        ensure!(
            !writers.is_empty(),
            "{id}: must depend on the item that writes its protected tests"
        );
        for glob in &item.protected_tests {
            let mut writable = false;
            for writer in &writers {
                writable |= crate::checks::within(&writer.allowed, glob)?;
            }
            ensure!(
                writable,
                "{id}: the test-writing item may not write its protected tests ({glob})"
            );
        }
        Ok(())
    }
    pub fn resolved(&self) -> bool {
        self.decisions.iter().all(|decision| {
            decision
                .answer
                .as_ref()
                .is_some_and(|answer| !answer.trim().is_empty())
        }) && self.items.values().all(|item| item.questions.is_empty())
    }
    pub fn approved(&self) -> bool {
        self.approved_revision == Some(self.revision)
            && self.confirmed_revision == Some(self.revision)
            && self.resolved()
    }
    pub fn ready(&self, id: &str, state: &State) -> Result<&Contract> {
        ensure!(
            self.approved(),
            "application baseline is unresolved or not approved at this revision"
        );
        let item = self.items.get(id).context("unknown execution item")?;
        for dependency in &item.dependencies {
            ensure!(
                state
                    .steps
                    .iter()
                    .any(|step| step.id == *dependency && step.status == StepStatus::Accepted),
                "dependency {dependency} is not accepted"
            );
        }
        Ok(item)
    }
}

pub fn store(task: &Task, mut proposal: Application, reason: &str) -> Result<Vec<String>> {
    let _control = task.control_lock()?;
    text(reason, "change reason")?;
    let configured = task.config()?.check_profiles;
    for repo in proposal
        .items
        .values()
        .map(|item| item.repo.clone())
        .collect::<BTreeSet<_>>()
    {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            proposal.check_configuration.entry(repo.clone())
        {
            if let Some(checks) = configured.get(&repo) {
                entry.insert(checks.clone());
            }
        }
    }
    proposal.validate()?;
    proposal.confirmed_revision = None;
    proposal.approved_revision = None;
    // Release evidence is not writable through a plan submission.
    ensure!(
        proposal
            .milestones
            .values()
            .all(|milestone| milestone.release_record.is_none()),
        "release records cannot be supplied by a planner"
    );
    task.transaction(|tx| {
        let old = &tx.state.application;
        ensure!(
            proposal.revision == old.revision + 1,
            "baseline proposal must be the next revision"
        );
        let affected = old
            .items
            .keys()
            .chain(proposal.items.keys())
            .filter(|id| old.items.get(*id) != proposal.items.get(*id))
            .cloned()
            .collect::<BTreeSet<_>>();
        for (id, milestone) in &old.milestones {
            if milestone.release_record.is_some() {
                ensure!(
                    proposal.milestones.get(id) == Some(milestone),
                    "released milestone cannot change"
                );
            }
        }
        tx.state.application_revisions.push(Revision {
            previous: old.clone(),
            reason: reason.into(),
            affected: affected.iter().cloned().collect(),
        });
        tx.state.application_proposal = Some(proposal);
        tx.log(
            Kind::Plan,
            None,
            None,
            &format!("application baseline revised: {reason}"),
        );
        Ok(affected.into_iter().collect())
    })
}
/// The changes to a baseline, and only those: what an amendment sends
/// instead of the whole application, which may be too big for one call.
/// `None` in a map removes that entry; a decision replaces the one with its id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Amendment {
    pub goal: Option<String>,
    pub non_goals: Option<Vec<String>>,
    pub scopes: BTreeMap<String, Option<String>>,
    pub items: BTreeMap<String, Option<Contract>>,
    pub milestones: BTreeMap<String, Option<Milestone>>,
    pub decisions: Vec<Decision>,
    pub check_configuration: BTreeMap<String, Option<crate::check_profiles::RepositoryChecks>>,
}

/// Applies `changes` to `base` as the next revision after `current`.
fn amended(base: &Application, current: u64, changes: Amendment) -> Application {
    fn apply<T>(entries: &mut BTreeMap<String, T>, changes: BTreeMap<String, Option<T>>) {
        for (id, change) in changes {
            match change {
                Some(value) => entries.insert(id, value),
                None => entries.remove(&id),
            };
        }
    }
    let mut next = base.clone();
    next.revision = current + 1;
    if let Some(goal) = changes.goal {
        next.goal = goal;
    }
    if let Some(non_goals) = changes.non_goals {
        next.non_goals = non_goals;
    }
    apply(&mut next.scopes, changes.scopes);
    apply(&mut next.items, changes.items);
    apply(&mut next.milestones, changes.milestones);
    apply(&mut next.check_configuration, changes.check_configuration);
    for decision in changes.decisions {
        match next.decisions.iter_mut().find(|old| old.id == decision.id) {
            Some(old) => *old = decision,
            None => next.decisions.push(decision),
        }
    }
    next
}

/// Proposes the next revision as `changes` to the pending proposal, when
/// there is one, so amendments add up, or to the baseline. It is checked and
/// stored as any proposal is.
pub fn amend(task: &Task, changes: Amendment, reason: &str) -> Result<Vec<String>> {
    let state = task.state()?;
    let current = state.application.revision;
    let base = state
        .application_proposal
        .as_ref()
        .filter(|proposal| proposal.revision == current + 1)
        .unwrap_or(&state.application);
    store(task, amended(base, current, changes), reason)
}

/// The baseline without its contracts' bodies, small enough to read in one
/// call: each item is its title, scope, role, lane and dependencies; `read`
/// gives the full contracts asked for.
pub fn outline(application: &Application) -> serde_json::Value {
    let mut outline = serde_json::to_value(application).unwrap_or_default();
    outline["items"] = application
        .items
        .iter()
        .map(|(id, item)| {
            (
                id.clone(),
                serde_json::json!({"title":item.title,"scope":item.scope,"role":item.role,"lane":item.lane,"dependencies":item.dependencies}),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
    outline
}

/// The full contracts of `ids`, with the revision they belong to.
pub fn items(application: &Application, ids: &[String]) -> Result<serde_json::Value> {
    let mut contracts = serde_json::Map::new();
    for id in ids {
        let item = application
            .items
            .get(id)
            .with_context(|| format!("revision {} has no item {id}", application.revision))?;
        contracts.insert(id.clone(), serde_json::to_value(item)?);
    }
    Ok(serde_json::json!({"revision":application.revision,"items":contracts}))
}

pub fn confirm(task: &Task, revision: u64, approve: bool) -> Result<()> {
    let _control = task.control_lock()?;
    task.transaction(|tx| {
        let mut application = tx
            .state
            .application_proposal
            .take()
            .context("no baseline proposal awaiting decision")?;
        application.validate()?;
        ensure!(
            application.revision == revision,
            "baseline revision changed"
        );
        ensure!(
            application.resolved(),
            "resolve open interview/contract questions first"
        );
        if approve {
            ensure!(
                application.confirmed_revision == Some(revision),
                "confirm shared understanding before approving baseline"
            );
            let old = &tx.state.application;
            let global_change = old.revision > 0
                && (old.goal != application.goal || old.non_goals != application.non_goals);
            let mut affected = old
                .items
                .keys()
                .filter(|id| global_change || old.items.get(*id) != application.items.get(*id))
                .cloned()
                .collect::<BTreeSet<_>>();
            loop {
                let additional = application
                    .items
                    .iter()
                    .filter(|(id, item)| {
                        !affected.contains(*id)
                            && item
                                .dependencies
                                .iter()
                                .any(|dependency| affected.contains(dependency))
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                if additional.is_empty() {
                    break;
                }
                affected.extend(additional);
            }
            let orders = tx
                .state
                .orders
                .iter()
                .filter(|order| affected.contains(&order.step))
                .map(|order| order.id.clone())
                .collect::<BTreeSet<_>>();
            for order in &mut tx.state.orders {
                if orders.contains(&order.id) {
                    order.status = crate::state::OrderStatus::Closed;
                }
            }
            tx.state.queue.retain(|message| {
                !message
                    .order
                    .as_ref()
                    .is_some_and(|order| orders.contains(order))
            });
            tx.state
                .check_evidence
                .retain(|order, _| !orders.contains(order));
            for step in &mut tx.state.steps {
                if affected.contains(&step.id) {
                    step.set_status(crate::state::StepStatus::Rejected, crate::log::now());
                }
            }
            application.approved_revision = Some(revision);
            tx.state.application = application;
            tx.state.approved = true;
            tx.log(
                Kind::Approve,
                None,
                None,
                &format!(
                    "user approved exact baseline revision {revision}; affected contracts: {}",
                    affected.into_iter().collect::<Vec<_>>().join(", ")
                ),
            );
        } else {
            application.confirmed_revision = Some(revision);
            tx.state.application_proposal = Some(application);
            tx.log(
                Kind::Approve,
                None,
                None,
                "user confirmed planning understanding",
            );
        }
        Ok(())
    })
}
pub fn question_frontier(application: &Application) -> Vec<&Decision> {
    application
        .decisions
        .iter()
        .filter(|decision| {
            decision.answer.is_none()
                && decision.prerequisites.iter().all(|id| {
                    application
                        .decisions
                        .iter()
                        .any(|prerequisite| prerequisite.id == *id && prerequisite.answer.is_some())
                })
        })
        .collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    pub fn baseline() -> Application {
        let item = Contract {
            scope: "S01".into(),
            title: "Token service".into(),
            role: "backend-data".into(),
            repo: "app".into(),
            lane: "back".into(),
            allowed: vec!["backend/**".into()],
            reuse: vec!["ExistingRepository".into()],
            interfaces: vec!["issue() -> Token".into()],
            behavior: vec!["Issues token".into()],
            errors: vec!["Reject invalid account".into()],
            tests: vec!["Cannot access another tenant".into()],
            profiles: vec!["backend".into()],
            acceptance: vec!["Service contract passes".into()],
            standards: STANDARDS_VERSION.into(),
            ..Default::default()
        };
        Application {
            revision: 1,
            check_configuration:BTreeMap::from([("app".into(),serde_json::from_value(serde_json::json!({"environments":{"testing":{}},"checks":{"backend":{"command":"true","environment":"testing"}},"selected":["backend"]})).unwrap())]),
            goal: "Application".into(),
            scopes: BTreeMap::from([("S01".into(), "Accounts".into())]),
            items: BTreeMap::from([("S01-B01".into(), item)]),
            milestones: BTreeMap::from([(
                "M01".into(),
                Milestone {
                    version: "1.1.0".into(),
                    title: "Accounts ready".into(),
                    required: vec!["S01-B01".into()],
                    ..Default::default()
                },
            )]),
            ..Default::default()
        }
    }
    #[test]
    fn the_outline_keeps_ids_and_titles_without_contract_bodies() {
        let outline = outline(&baseline());
        assert_eq!(outline["items"]["S01-B01"]["title"], "Token service");
        assert_eq!(outline["items"]["S01-B01"]["lane"], "back");
        assert!(outline["items"]["S01-B01"].get("acceptance").is_none());
        assert_eq!(outline["milestones"]["M01"]["title"], "Accounts ready");
        assert_eq!(outline["revision"], 1);

        let read = items(&baseline(), &["S01-B01".into()]).unwrap();
        assert_eq!(
            read["items"]["S01-B01"]["acceptance"][0],
            "Service contract passes"
        );
        let error = items(&baseline(), &["S09".into()]).unwrap_err().to_string();
        assert!(error.contains("no item S09"), "{error}");
    }

    #[test]
    fn an_amendment_changes_only_what_it_names_on_the_next_revision() {
        let mut base = baseline();
        base.items
            .insert("S01-B02".into(), base.items["S01-B01"].clone());
        let mut added = base.items["S01-B01"].clone();
        added.title = "Token refresh".into();
        let mut renamed = base.items["S01-B01"].clone();
        renamed.title = "Token issue".into();
        let changes = Amendment {
            items: BTreeMap::from([
                ("S01-B03".into(), Some(added)),
                ("S01-B01".into(), Some(renamed)),
                ("S01-B02".into(), None),
            ]),
            ..Default::default()
        };
        let next = amended(&base, 3, changes);
        assert_eq!(next.revision, 4);
        assert_eq!(next.items["S01-B03"].title, "Token refresh");
        assert_eq!(next.items["S01-B01"].title, "Token issue");
        assert!(!next.items.contains_key("S01-B02"));
        assert_eq!(
            (&next.goal, &next.milestones),
            (&base.goal, &base.milestones)
        );
    }

    #[test]
    fn amendments_add_up_on_the_pending_proposal_and_are_checked() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        store(&task, baseline(), "first").unwrap();
        confirm(&task, 1, false).unwrap();
        confirm(&task, 1, true).unwrap();
        let goal = Amendment {
            goal: Some("Application with billing".into()),
            ..Default::default()
        };
        amend(&task, goal, "billing goal").unwrap();
        let non_goals = Amendment {
            non_goals: Some(vec!["No payments".into()]),
            ..Default::default()
        };
        amend(&task, non_goals, "no payments").unwrap();
        let proposal = task.state().unwrap().application_proposal.unwrap();
        assert_eq!(proposal.revision, 2);
        assert_eq!(
            proposal.goal, "Application with billing",
            "the first amendment stays"
        );
        assert_eq!(proposal.non_goals, ["No payments"]);
        assert!(proposal.approved_revision.is_none());

        let broken = Amendment {
            items: BTreeMap::from([("S01-B01".into(), Some(Contract::default()))]),
            ..Default::default()
        };
        assert!(amend(&task, broken, "empty contract").is_err());
        assert_eq!(
            task.state().unwrap().application_proposal.unwrap(),
            proposal
        );
    }
    #[test]
    fn contracts_reject_incomplete_escaping_and_cyclic_work() {
        let mut application = baseline();
        application.validate().unwrap();
        application.items.get_mut("S01-B01").unwrap().allowed = vec!["../other/**".into()];
        assert!(application.validate().is_err());
        application = baseline();
        application.items.get_mut("S01-B01").unwrap().dependencies = vec!["S01-B01".into()];
        assert!(application.validate().is_err());
        application = baseline();
        application
            .items
            .get_mut("S01-B01")
            .unwrap()
            .interfaces
            .clear();
        assert!(application.validate().is_err());
    }
    /// The baseline with its acceptance tests written by an item of their
    /// own, which the implementation depends on.
    fn with_protected_tests() -> Application {
        let mut application = baseline();
        let implementation = application.items.get_mut("S01-B01").unwrap();
        implementation.protected_tests = vec!["backend/tests/acceptance/**".into()];
        implementation.dependencies = vec!["S01-T01".into()];
        let tests = Contract {
            title: "Acceptance tests for the token service".into(),
            allowed: vec!["backend/tests/acceptance/**".into()],
            writes_tests: true,
            dependencies: vec![],
            protected_tests: vec![],
            ..implementation.clone()
        };
        application.items.insert("S01-T01".into(), tests);
        application
            .milestones
            .get_mut("M01")
            .unwrap()
            .required
            .push("S01-T01".into());
        application
    }

    #[test]
    fn protected_tests_come_from_an_item_that_writes_them_first() {
        with_protected_tests().validate().unwrap();

        let mut application = with_protected_tests();
        application
            .items
            .get_mut("S01-B01")
            .unwrap()
            .dependencies
            .clear();
        let error = application.validate().unwrap_err().to_string();
        assert!(error.contains("depend on the item that writes"), "{error}");

        let mut application = with_protected_tests();
        application.items.get_mut("S01-T01").unwrap().allowed = vec!["backend/src/**".into()];
        let error = application.validate().unwrap_err().to_string();
        assert!(
            error.contains("may not write its protected tests"),
            "{error}"
        );

        let mut application = with_protected_tests();
        application
            .items
            .get_mut("S01-B01")
            .unwrap()
            .protected_tests = vec!["../escape/**".into()];
        assert!(application.validate().is_err());
    }

    #[test]
    fn an_implementation_order_may_not_touch_its_protected_tests() {
        let application = with_protected_tests();
        let implementation = &application.items["S01-B01"];
        assert_eq!(
            implementation.forbidden_for_order(&["secrets/**".into()]),
            ["secrets/**", "backend/tests/acceptance/**"]
        );
        assert_eq!(
            implementation.forbidden_for_order(&["backend/tests/acceptance/**".into()]),
            ["backend/tests/acceptance/**"],
            "listed once"
        );
        let tests = &application.items["S01-T01"];
        assert!(
            tests.forbidden_for_order(&[]).is_empty(),
            "the item that writes the tests may change them"
        );
    }

    #[test]
    fn interview_only_asks_settled_frontier_and_approval_is_revision_bound() {
        let mut application = baseline();
        application.decisions = vec![
            Decision {
                id: "Q1".into(),
                round: 1,
                question: "Goal?".into(),
                recommendation: "Accounts".into(),
                prerequisites: vec![],
                answer: None,
            },
            Decision {
                id: "Q2".into(),
                round: 2,
                question: "Fields?".into(),
                recommendation: "Name".into(),
                prerequisites: vec!["Q1".into()],
                answer: None,
            },
        ];
        assert_eq!(question_frontier(&application)[0].id, "Q1");
        application.decisions[0].answer = Some("yes".into());
        assert_eq!(question_frontier(&application)[0].id, "Q2");
        application.confirmed_revision = Some(1);
        application.approved_revision = Some(1);
        assert!(!application.approved());
        application.decisions[1].answer = Some("yes".into());
        assert!(application.approved());
        application.revision += 1;
        assert!(!application.approved());
    }
    #[test]
    fn old_application_defaults_to_unapproved() {
        let application: Application = serde_json::from_str("{}").unwrap();
        assert!(!application.approved());
    }
}
