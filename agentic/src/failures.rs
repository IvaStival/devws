//! Why each step failed, so the leader diagnoses before it orders again and
//! attempts count only work that failed: not re-checks, resets, stale
//! evidence or the environment, which devAgentics or other work caused.

use crate::state::{Order, State, StepStatus};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// The longest summary a failure keeps.
const SUMMARY: usize = 500;
/// The failures `task_status` shows per open step.
const SHOWN: usize = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub at: u64,
    /// The order it ended; none for a revision reset.
    #[serde(default)]
    pub order: Option<String>,
    pub kind: FailureKind,
    pub summary: String,
    /// The leader's diagnosis, given with the order that followed it.
    #[serde(default)]
    pub diagnosis: Option<Diagnosis>,
    /// The order that followed it: it is settled.
    #[serde(default)]
    pub resolved_by: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// Its tests or the file rules failed: the work is wrong.
    CheckFailed,
    /// A reviewer asked for changes.
    ChangesRequested,
    /// A check ran out of time: slow work, or a slow environment.
    CheckTimeout,
    /// Its merge with the task's branch failed: its work, or other work.
    IntegrationFailed,
    /// A reviewer could not review it.
    ReviewBlocked,
    /// The agent could not go on.
    AgentBlocked,
    /// The leader sent it back.
    LeaderRejected,
    /// It failed before devAgentics kept failures.
    Unrecorded,
    /// Only the environment or the runner failed.
    CheckEnvironment,
    /// Its evidence went stale: the branch or the checks moved.
    StaleEvidence,
    /// A revision changed its contract.
    RevisionReset,
    /// Its agent was stopped with the order open.
    AgentStopped,
}

/// Whether a failure counts as an attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    /// Always: the work failed.
    Work,
    /// When the leader's diagnosis names a work cause.
    Ambiguous,
    /// Never: devAgentics, other work or the environment caused it.
    NotWork,
}

impl FailureKind {
    pub fn weight(self) -> Weight {
        match self {
            FailureKind::CheckFailed | FailureKind::ChangesRequested => Weight::Work,
            FailureKind::CheckTimeout
            | FailureKind::IntegrationFailed
            | FailureKind::ReviewBlocked
            | FailureKind::AgentBlocked
            | FailureKind::LeaderRejected
            | FailureKind::Unrecorded => Weight::Ambiguous,
            FailureKind::CheckEnvironment
            | FailureKind::StaleEvidence
            | FailureKind::RevisionReset
            | FailureKind::AgentStopped => Weight::NotWork,
        }
    }
}

/// The root cause the leader found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    ImplementationDefect,
    TestDefect,
    PlanDefect,
    MissingContext,
    IntegrationConflict,
    OtherWork,
    Environment,
    Devagentics,
}

impl Cause {
    /// The step's own work was at fault: it counts as an attempt.
    pub fn is_work(self) -> bool {
        !matches!(
            self,
            Cause::OtherWork | Cause::Environment | Cause::Devagentics
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnosis {
    pub cause: Cause,
    /// What shows it: log lines, file:line, a diff.
    pub evidence: String,
    /// What the next order does differently because of it.
    pub change: String,
    /// After a second failure at the same gate: the one assumption every
    /// attempt shared, and what tested it. Two fixes that failed the same
    /// way usually share a wrong premise, not two bad fixes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub premise: Option<String>,
}

/// How an order goes at its step: when a step fails twice for the same cause,
/// the next order must differ in one of these.
#[derive(Clone, Debug, PartialEq)]
pub struct Approach {
    pub agent: String,
    pub plan: Option<String>,
    pub contract: Option<String>,
    pub allowed: Vec<String>,
    pub read: Vec<String>,
    pub targets: Vec<String>,
}

/// The approach of an order already given.
pub fn approach_of(state: &State, order: &Order) -> Result<Approach> {
    Ok(Approach {
        agent: order.agent.clone(),
        plan: order.plan.clone(),
        contract: contract_hash(state.contract_bindings.get(&order.id))?,
        allowed: order.allowed.clone(),
        read: order.read.clone(),
        targets: order.targets.clone(),
    })
}

/// A contract's fingerprint, to tell an amended one apart.
pub fn contract_hash(contract: Option<&crate::execution::Contract>) -> Result<Option<String>> {
    contract
        .map(|contract| crate::runner::content_hash(&serde_json::to_vec(contract)?))
        .transpose()
}

/// A stale-evidence refusal: the leader re-runs the checks, which costs no
/// attempt.
#[derive(Debug)]
pub struct Stale(pub String);

impl std::fmt::Display for Stale {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Stale {}

/// When `error` is a stale-evidence refusal, records it on the step's
/// latest order (or `order`): the re-check it needs costs no attempt and no
/// diagnosis. Returns the error unchanged.
pub fn note_stale(
    task: &crate::task::Task,
    step: Option<&str>,
    order: Option<&str>,
    error: anyhow::Error,
) -> anyhow::Error {
    if !error
        .chain()
        .any(|cause| cause.downcast_ref::<Stale>().is_some())
    {
        return error;
    }
    let message = format!("{error:#}");
    let _ = task.transaction(|tx| {
        let found = tx.state.orders.iter().rev().find(|found| {
            order.is_some_and(|order| found.id == order)
                || step.is_some_and(|step| found.step == step)
        });
        let Some((step, order)) = found.map(|found| (found.step.clone(), found.id.clone())) else {
            return Ok(());
        };
        record(
            &mut tx.state,
            &step,
            Some(&order),
            FailureKind::StaleEvidence,
            &message,
            crate::log::now(),
        );
        Ok(())
    });
    error
}

/// Records why `step` failed. An order fails once: a rejection after a
/// failed check or review keeps the first, so nothing counts twice.
pub fn record(
    state: &mut State,
    step: &str,
    order: Option<&str>,
    kind: FailureKind,
    summary: &str,
    now: u64,
) {
    let failures = state.step_failures.entry(step.to_string()).or_default();
    if let Some(order) = order
        && failures
            .iter()
            .any(|failure| failure.order.as_deref() == Some(order))
    {
        return;
    }
    failures.push(Failure {
        at: now,
        order: order.map(str::to_string),
        kind,
        summary: summary.chars().take(SUMMARY).collect(),
        diagnosis: None,
        resolved_by: None,
    });
}

/// Whether a failure counts as an attempt.
pub fn counts(failure: &Failure) -> bool {
    match failure.kind.weight() {
        Weight::Work => true,
        // Until it is diagnosed, the leader has not said whose fault it was.
        Weight::Ambiguous => failure
            .diagnosis
            .as_ref()
            .is_some_and(|diagnosis| diagnosis.cause.is_work()),
        Weight::NotWork => false,
    }
}

/// The attempts a step used: its failures that were the work's.
pub fn work_failures(state: &State, step: &str) -> u32 {
    state.step_failures.get(step).map_or(0, |failures| {
        failures.iter().filter(|failure| counts(failure)).count() as u32
    })
}

/// A step that failed before failures were kept: it needs a diagnosis once.
fn unrecorded(state: &State, step: &str) -> bool {
    !state.step_failures.contains_key(step)
        && state.steps.iter().any(|found| {
            found.id == step
                && matches!(
                    found.status,
                    StepStatus::Failed
                        | StepStatus::Blocked
                        | StepStatus::Rejected
                        | StepStatus::Exhausted
                )
        })
}

/// Whether the next order of `step` must say why it failed and what changes.
pub fn needs_diagnosis(state: &State, step: &str) -> bool {
    unrecorded(state, step)
        || state.step_failures.get(step).is_some_and(|failures| {
            failures.iter().any(|failure| {
                failure.resolved_by.is_none() && failure.kind.weight() != Weight::NotWork
            })
        })
}

/// Settles the failures `new_order` follows. A failure that was the work's,
/// or might have been, needs a diagnosis; the same cause again with the same
/// approach is refused (only the user's waiver lets it through once).
pub fn resolve(
    state: &mut State,
    step: &str,
    diagnosis: Option<Diagnosis>,
    approach: &Approach,
    new_order: &str,
    now: u64,
) -> Result<()> {
    if unrecorded(state, step) {
        record(
            state,
            step,
            None,
            FailureKind::Unrecorded,
            "it failed before devAgentics kept why",
            now,
        );
    }
    let failures = state.step_failures.get(step).cloned().unwrap_or_default();
    let pending: Vec<usize> = (0..failures.len())
        .filter(|&index| failures[index].resolved_by.is_none())
        .collect();
    let diagnosable = pending
        .iter()
        .copied()
        .rfind(|&index| failures[index].kind.weight() != Weight::NotWork);
    if let Some(index) = diagnosable {
        let failure = &failures[index];
        let Some(diagnosis) = &diagnosis else {
            bail!(
                "step {step} failed ({:?}{}): {}. Find the cause before ordering it again, then give send_order a diagnosis {{cause, evidence, change}}. {}",
                failure.kind,
                failure
                    .order
                    .as_deref()
                    .map(|order| format!(", order {order}"))
                    .unwrap_or_default(),
                failure.summary,
                guidance(step, failure.order.as_deref())
            );
        };
        let evidence = diagnosis.evidence.trim().chars().count();
        let change = diagnosis.change.trim().chars().count();
        anyhow::ensure!(
            (20..=1000).contains(&evidence),
            "the diagnosis evidence must say what shows the cause (20 to 1000 characters)"
        );
        anyhow::ensure!(
            (10..=1000).contains(&change),
            "the diagnosis change must say what this order does differently (10 to 1000 characters)"
        );
        let same_gate = failures[..=index]
            .iter()
            .filter(|earlier| earlier.kind == failure.kind)
            .count();
        let premise = diagnosis
            .premise
            .as_deref()
            .map_or(0, |premise| premise.trim().chars().count());
        anyhow::ensure!(
            same_gate < 2 || (20..=1000).contains(&premise),
            "step {step} failed at the same gate ({:?}) {same_gate} times: before another fix, \
             give diagnosis.premise, the one assumption every attempt shared and what you ran \
             to test it (20 to 1000 characters)",
            failure.kind
        );
        let previous = failures[..index]
            .iter()
            .rev()
            .find_map(|earlier| earlier.diagnosis.as_ref());
        let failed_approach = failure
            .order
            .as_deref()
            .and_then(|order| state.orders.iter().find(|found| found.id == order))
            .map(|order| approach_of(state, order))
            .transpose()?;
        let same = previous.is_some_and(|previous| previous.cause == diagnosis.cause)
            && failed_approach.as_ref() == Some(approach);
        if same && !state.diagnosis_waivers.remove(step) {
            bail!(
                "step {step} failed again for {:?} after an order that already targeted it ({}). The approach is the same: agent {}, the same contract, plan, files and targets. Change it (another agent, an amended contract or plan, other read or targets, or split the step), or close_step it, or escalate.",
                diagnosis.cause,
                previous
                    .map(|previous| previous.change.as_str())
                    .unwrap_or_default(),
                approach.agent
            );
        }
    }
    let failures = state.step_failures.entry(step.to_string()).or_default();
    if let Some(index) = diagnosable {
        failures[index].diagnosis = diagnosis;
    }
    for index in pending {
        failures[index].resolved_by = Some(new_order.to_string());
    }
    Ok(())
}

/// Where the leader looks before ordering a failed step again.
pub fn guidance(step: &str, order: Option<&str>) -> String {
    let at = order.map(|order| format!(" {order}")).unwrap_or_default();
    format!(
        "Find the cause of {step} before ordering it again: read_check_log{at}, git_diff at={}, read_file or search_code where it points, the review findings. Then send_order with a diagnosis {{cause, evidence, change}}; a re-check after stale evidence or the environment needs none and costs no attempt.",
        order.unwrap_or("<order>")
    )
}

/// The step's failures as `task_status` shows them: what counts, whether the
/// next order needs a diagnosis, and the newest few.
pub fn status_json(state: &State, step: &str) -> serde_json::Value {
    let failures = state
        .step_failures
        .get(step)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let newest: Vec<serde_json::Value> = failures
        .iter()
        .rev()
        .take(SHOWN)
        .map(|failure| {
            serde_json::json!({
                "at": failure.at,
                "order": failure.order,
                "kind": failure.kind,
                "counts": counts(failure),
                "summary": failure.summary.chars().take(160).collect::<String>(),
                "cause": failure.diagnosis.as_ref().map(|diagnosis| diagnosis.cause),
            })
        })
        .collect();
    serde_json::json!({
        "work_failures": work_failures(state, step),
        "needs_diagnosis": needs_diagnosis(state, step),
        "failures": newest,
    })
}

/// What the leader's diagnoses of an accepted step taught, as knowledge for
/// its lane: the next orders there carry it, so the next agent does not make
/// the same mistake. One note per cause; devAgentics writes them, no agent
/// turn.
pub fn lessons(state: &State, step: &str) -> Vec<crate::memory::Note> {
    let Some(lane) = state
        .application
        .items
        .get(step)
        .map(|item| item.lane.clone())
    else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    let mut notes = Vec::new();
    for failure in state.step_failures.get(step).into_iter().flatten() {
        let Some(diagnosis) = &failure.diagnosis else {
            continue;
        };
        if !diagnosis.cause.is_work() || seen.contains(&diagnosis.cause) {
            continue;
        }
        seen.push(diagnosis.cause);
        notes.push(crate::memory::Note {
            kind: crate::memory::NoteKind::Knowledge,
            title: format!("{} in {step}", cause_name(diagnosis.cause)),
            body: format!(
                "cause: {}\nevidence: {}\nfix: {}\nfrom: step {step}{}, accepted",
                cause_name(diagnosis.cause),
                diagnosis.evidence.trim(),
                diagnosis.change.trim(),
                failure
                    .order
                    .as_ref()
                    .map_or(String::new(), |order| format!(", order {order}")),
            ),
            author: crate::state::LEADER.into(),
            step: Some(step.into()),
            area: Some(lane.clone()),
        });
    }
    notes
}

/// The causes of `step` that other items of its lane were also diagnosed
/// with: a lesson the team keeps relearning belongs in a check or a
/// standard, not in another note.
pub fn repeated(state: &State, step: &str) -> Vec<String> {
    let Some(lane) = state.application.items.get(step).map(|item| &item.lane) else {
        return Vec::new();
    };
    let causes = |id: &str| -> Vec<Cause> {
        state
            .step_failures
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|failure| failure.diagnosis.as_ref().map(|diagnosis| diagnosis.cause))
            .filter(|cause| cause.is_work())
            .collect()
    };
    let mut lines = Vec::new();
    let mut own = causes(step);
    own.dedup();
    for cause in own {
        let mut steps: Vec<&str> = state
            .application
            .items
            .iter()
            .filter(|(id, item)| item.lane == *lane && causes(id).contains(&cause))
            .map(|(id, _)| id.as_str())
            .collect();
        steps.sort_unstable();
        if steps.len() >= 2 {
            lines.push(format!(
                "{} repeated in lane {lane} ({}): make it impossible instead of noting it again. \
                 Fix it at the highest level that works: the design, then a type, then a check \
                 whose error names the fix, then a test; a standard or doc only for a judgment call.",
                cause_name(cause),
                steps.join(", ")
            ));
        }
    }
    lines
}

fn cause_name(cause: Cause) -> String {
    serde_json::to_value(cause)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{OrderStatus, Step};

    fn diagnosis(cause: Cause) -> Diagnosis {
        Diagnosis {
            cause,
            evidence: "useSystemMutations.test.tsx:757 expected 'mutation remains'".into(),
            change: "keep the unrelated mutation's cache entry".into(),
            premise: Some("the cache key names the mutation: ran the key test, it does".into()),
        }
    }

    fn diagnosed(steps: &[(&str, &str, Cause)]) -> State {
        let mut state = State::default();
        for (step, lane, cause) in steps {
            state.application.items.insert(
                (*step).into(),
                crate::execution::Contract {
                    lane: (*lane).into(),
                    ..Default::default()
                },
            );
            state
                .step_failures
                .entry((*step).into())
                .or_default()
                .push(Failure {
                    at: 1,
                    order: Some(format!("o-{step}")),
                    kind: FailureKind::CheckFailed,
                    summary: "tests fail".into(),
                    diagnosis: Some(diagnosis(*cause)),
                    resolved_by: Some("o-next".into()),
                });
        }
        state
    }

    #[test]
    fn an_accepted_steps_diagnoses_become_knowledge_for_its_lane() {
        let mut state = diagnosed(&[("B3", "back", Cause::MissingContext)]);
        state.step_failures.get_mut("B3").unwrap().push(Failure {
            at: 2,
            order: Some("o-env".into()),
            kind: FailureKind::CheckEnvironment,
            summary: "docker down".into(),
            diagnosis: Some(diagnosis(Cause::Environment)),
            resolved_by: None,
        });
        let notes = lessons(&state, "B3");
        assert_eq!(notes.len(), 1, "the environment taught the code nothing");
        let note = &notes[0];
        assert_eq!(note.kind, crate::memory::NoteKind::Knowledge);
        assert_eq!(note.area.as_deref(), Some("back"));
        assert_eq!(note.title, "missing_context in B3");
        assert!(
            note.body
                .contains("evidence: useSystemMutations.test.tsx:757")
                && note
                    .body
                    .contains("fix: keep the unrelated mutation's cache entry")
                && note.body.contains("from: step B3, order o-B3"),
            "{}",
            note.body
        );
        assert!(lessons(&diagnosed(&[]), "B3").is_empty());
    }

    #[test]
    fn a_cause_two_items_of_a_lane_share_asks_for_a_structural_fix() {
        let state = diagnosed(&[
            ("B3", "back", Cause::MissingContext),
            ("B5", "back", Cause::MissingContext),
            ("F1", "front", Cause::MissingContext),
            ("B6", "back", Cause::ImplementationDefect),
        ]);
        let lines = repeated(&state, "B5");
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("missing_context repeated in lane back (B3, B5)"),
            "another lane's does not count: {}",
            lines[0]
        );
        assert!(lines[0].contains("then a check whose error names the fix"));
        assert!(repeated(&state, "B6").is_empty(), "once is no pattern");
    }

    fn order(id: &str, agent: &str) -> Order {
        Order {
            read: Vec::new(),
            targets: Vec::new(),
            id: id.into(),
            step: "B3".into(),
            agent: agent.into(),
            text: String::new(),
            allowed: vec!["web/**".into()],
            forbidden: Vec::new(),
            check: None,
            base: "abc".into(),
            status: OrderStatus::Closed,
            delivered: true,
            stalled: false,
            plan: None,
        }
    }

    fn approach(agent: &str) -> Approach {
        Approach {
            agent: agent.into(),
            plan: None,
            contract: None,
            allowed: vec!["web/**".into()],
            read: Vec::new(),
            targets: Vec::new(),
        }
    }

    fn state() -> State {
        let mut state = State::default();
        state.steps.push(Step {
            id: "B3".into(),
            title: "Menu".into(),
            status: StepStatus::Failed,
            attempts: 1,
            spec: None,
            review_since: None,
        });
        state.orders.push(order("o1", "hammer-28"));
        state
    }

    #[test]
    fn only_the_works_own_failures_count() {
        let mut state = state();
        record(
            &mut state,
            "B3",
            Some("o1"),
            FailureKind::CheckFailed,
            "1 test failed",
            1,
        );
        record(
            &mut state,
            "B3",
            Some("o1"),
            FailureKind::LeaderRejected,
            "same order",
            2,
        );
        record(
            &mut state,
            "B3",
            Some("o2"),
            FailureKind::CheckEnvironment,
            "docker down",
            3,
        );
        record(
            &mut state,
            "B3",
            Some("o3"),
            FailureKind::StaleEvidence,
            "branch moved",
            4,
        );
        record(
            &mut state,
            "B3",
            None,
            FailureKind::RevisionReset,
            "contract changed",
            5,
        );
        record(
            &mut state,
            "B3",
            Some("o4"),
            FailureKind::IntegrationFailed,
            "other lane",
            6,
        );
        assert_eq!(state.step_failures["B3"].len(), 5, "one failure per order");
        assert_eq!(
            work_failures(&state, "B3"),
            1,
            "an undiagnosed ambiguous one waits"
        );

        state.step_failures.get_mut("B3").unwrap()[4].diagnosis = Some(diagnosis(Cause::OtherWork));
        assert_eq!(work_failures(&state, "B3"), 1);
        state.step_failures.get_mut("B3").unwrap()[4].diagnosis =
            Some(diagnosis(Cause::IntegrationConflict));
        assert_eq!(work_failures(&state, "B3"), 2);
    }

    #[test]
    fn a_work_failure_needs_a_diagnosis_and_a_not_work_one_none() {
        let mut state = state();
        record(
            &mut state,
            "B3",
            Some("o1"),
            FailureKind::StaleEvidence,
            "branch moved",
            1,
        );
        assert!(!needs_diagnosis(&state, "B3"));
        resolve(&mut state, "B3", None, &approach("hammer-28"), "o2", 2).unwrap();
        assert_eq!(
            state.step_failures["B3"][0].resolved_by.as_deref(),
            Some("o2")
        );

        state.orders.push(order("o2", "hammer-28"));
        record(
            &mut state,
            "B3",
            Some("o2"),
            FailureKind::CheckFailed,
            "1 test failed",
            3,
        );
        assert!(needs_diagnosis(&state, "B3"));
        let error = resolve(&mut state, "B3", None, &approach("hammer-28"), "o3", 4)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("diagnosis") && error.contains("read_check_log o2"),
            "{error}"
        );
        let mut thin = diagnosis(Cause::ImplementationDefect);
        thin.evidence = "tests".into();
        assert!(
            resolve(
                &mut state,
                "B3",
                Some(thin),
                &approach("hammer-28"),
                "o3",
                4
            )
            .is_err()
        );

        resolve(
            &mut state,
            "B3",
            Some(diagnosis(Cause::ImplementationDefect)),
            &approach("hammer-28"),
            "o3",
            4,
        )
        .unwrap();
        assert!(!needs_diagnosis(&state, "B3"));
        assert_eq!(
            state.step_failures["B3"][1]
                .diagnosis
                .as_ref()
                .unwrap()
                .cause,
            Cause::ImplementationDefect
        );
    }

    #[test]
    fn a_second_failure_at_the_same_gate_needs_the_premise_first() {
        let mut state = state();
        record(
            &mut state,
            "B3",
            Some("o1"),
            FailureKind::CheckFailed,
            "1 test failed",
            1,
        );
        let without = || Diagnosis {
            premise: None,
            ..diagnosis(Cause::ImplementationDefect)
        };
        resolve(
            &mut state,
            "B3",
            Some(without()),
            &approach("hammer-28"),
            "o2",
            2,
        )
        .expect("one failure needs no premise");
        state.orders.push(order("o2", "hammer-28"));
        record(
            &mut state,
            "B3",
            Some("o2"),
            FailureKind::CheckFailed,
            "same test",
            3,
        );
        let error = resolve(
            &mut state,
            "B3",
            Some(Diagnosis {
                cause: Cause::TestDefect,
                ..without()
            }),
            &approach("hammer-29"),
            "o3",
            4,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("failed at the same gate (CheckFailed) 2 times")
                && error.contains("diagnosis.premise"),
            "{error}"
        );
        resolve(
            &mut state,
            "B3",
            Some(diagnosis(Cause::TestDefect)),
            &approach("hammer-29"),
            "o3",
            4,
        )
        .expect("with the premise written and tested, the next fix may go");
    }

    #[test]
    fn the_same_cause_twice_needs_another_approach() {
        let mut state = state();
        record(
            &mut state,
            "B3",
            Some("o1"),
            FailureKind::CheckFailed,
            "1 test failed",
            1,
        );
        let cause = || Some(diagnosis(Cause::ImplementationDefect));
        resolve(&mut state, "B3", cause(), &approach("hammer-28"), "o2", 2).unwrap();
        state.orders.push(order("o2", "hammer-28"));
        record(
            &mut state,
            "B3",
            Some("o2"),
            FailureKind::CheckFailed,
            "same test",
            3,
        );

        let error = resolve(&mut state, "B3", cause(), &approach("hammer-28"), "o3", 4)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("failed again") && error.contains("close_step"),
            "{error}"
        );
        resolve(
            &mut state,
            "B3",
            Some(diagnosis(Cause::TestDefect)),
            &approach("hammer-28"),
            "o3",
            4,
        )
        .expect("another cause is another approach to the problem");

        state.orders.push(order("o3", "hammer-28"));
        record(
            &mut state,
            "B3",
            Some("o3"),
            FailureKind::CheckFailed,
            "again",
            5,
        );
        resolve(
            &mut state,
            "B3",
            Some(diagnosis(Cause::TestDefect)),
            &approach("hammer-31"),
            "o4",
            6,
        )
        .expect("another agent is another approach");

        state.orders.push(order("o4", "hammer-31"));
        record(
            &mut state,
            "B3",
            Some("o4"),
            FailureKind::CheckFailed,
            "again",
            7,
        );
        assert!(
            resolve(
                &mut state,
                "B3",
                Some(diagnosis(Cause::TestDefect)),
                &approach("hammer-31"),
                "o5",
                8
            )
            .is_err()
        );
        state.diagnosis_waivers.insert("B3".into());
        resolve(
            &mut state,
            "B3",
            Some(diagnosis(Cause::TestDefect)),
            &approach("hammer-31"),
            "o5",
            8,
        )
        .expect("the user's waiver lets it through once");
        assert!(state.diagnosis_waivers.is_empty());
    }

    #[test]
    fn a_step_that_failed_before_failures_were_kept_is_diagnosed_once() {
        let mut state = state();
        assert!(needs_diagnosis(&state, "B3"));
        assert!(resolve(&mut state, "B3", None, &approach("hammer-28"), "o2", 1).is_err());
        resolve(
            &mut state,
            "B3",
            Some(diagnosis(Cause::Devagentics)),
            &approach("hammer-28"),
            "o2",
            1,
        )
        .unwrap();
        assert_eq!(state.step_failures["B3"][0].kind, FailureKind::Unrecorded);
        assert_eq!(
            work_failures(&state, "B3"),
            0,
            "devAgentics' fault costs nothing"
        );
        assert!(!needs_diagnosis(&state, "B3"));
    }

    #[test]
    fn the_status_shows_the_newest_failures() {
        let mut state = state();
        for (index, order) in ["o1", "o2", "o3", "o4"].iter().enumerate() {
            record(
                &mut state,
                "B3",
                Some(order),
                FailureKind::CheckFailed,
                order,
                index as u64,
            );
        }
        let status = status_json(&state, "B3");
        assert_eq!(status["work_failures"], 4);
        assert_eq!(status["needs_diagnosis"], true);
        assert_eq!(status["failures"].as_array().unwrap().len(), 3);
        assert_eq!(status["failures"][0]["order"], "o4");
    }
}
