//! Independent, revision-bound review records. No verdict substitutes for tests.
use crate::{checks, execution::Workflow, log::Kind, state::StepStatus, task::Task};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approved,
    ChangesRequested,
    Blocked,
}
/// Why work had to be corrected: what the benchmark counts, so a change to
/// the team can be judged by the corrections it removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    RequirementAmbiguity,
    StalePlanInterface,
    ScopeOmission,
    EnvironmentMismatch,
    ImplementationDefect,
    IntegrationDefect,
    TestWeakening,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub file: String,
    pub line: u64,
    pub severity: String,
    pub blocking: bool,
    pub rationale: String,
    pub resolution: Option<String>,
    /// Required for a blocking finding.
    #[serde(default)]
    pub category: Option<Category>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub id: String,
    pub order: String,
    pub author: String,
    pub reviewer: String,
    pub baseline_revision: u64,
    pub order_revision: u64,
    pub base: String,
    pub source: String,
    pub target: String,
    pub evidence: String,
    #[serde(default)]
    pub candidate: crate::integration::Candidate,
    /// Hash of the passing candidate check evidence the review was asked on;
    /// empty when the repo configures no candidate check.
    #[serde(default)]
    pub candidate_evidence: String,
    pub verdict: Option<Verdict>,
    pub findings: Vec<Finding>,
    pub summary: Option<String>,
    /// When it was requested and submitted, in epoch seconds (0 / None for
    /// reviews from before they were kept).
    #[serde(default)]
    pub requested_at: u64,
    #[serde(default)]
    pub submitted_at: Option<u64>,
}
pub fn pending<'a>(state: &'a crate::state::State, reviewer: &str) -> Option<&'a Review> {
    state
        .reviews
        .iter()
        .find(|review| review.reviewer == reviewer && review.verdict.is_none())
}
/// What the checks found, readable by a reviewer who cannot run them: the
/// author's checks of the order and the candidate check of the exact merge,
/// each profile with its outcome, exit code, time and diagnostic.
pub fn check_results(state: &crate::state::State, review: &Review) -> serde_json::Value {
    let phases = |phases: &[crate::check_profiles::Phase]| {
        phases
            .iter()
            .map(|phase| serde_json::json!({"profile":phase.profile,"outcome":phase.outcome,"exit_code":phase.exit_code,"seconds":phase.seconds,"diagnostic":phase.diagnostic}))
            .collect::<Vec<_>>()
    };
    let order = state.check_evidence.get(&review.order).map(|evidence| {
        serde_json::json!({"head":evidence.head,"passed":evidence.passed(),"phases":phases(&evidence.phases)})
    });
    let candidate = state.candidate_evidence.get(&review.candidate.tree).map(|evidence| {
        serde_json::json!({"tree":evidence.tree,"passed":evidence.passed(),"phases":phases(&evidence.phases)})
    });
    serde_json::json!({"order":order,"candidate":candidate})
}

pub fn resumption(state: &crate::state::State, reviewer: &str) -> Option<String> {
    pending(state, reviewer).map(|review| format!("Resume independent review {} for order {}. Call status with input.view=brief and review_diff, inspect the immutable candidate through execute, then review_submit. Do not modify code.", review.id, review.order))
}
pub fn checkout(task: &Task, review: &Review) -> std::path::PathBuf {
    task.dir()
        .join("review-candidates")
        .join(format!("review-{}", review.id.trim_start_matches('r')))
}
fn evidence(task: &Task, order: &str) -> Result<String> {
    let state = task.state()?;
    let evidence = state
        .check_evidence
        .get(order)
        .context("passing authoritative check evidence required")?;
    ensure!(
        evidence.passed() && !evidence.phases.is_empty() && evidence.run.is_file(),
        "passing persisted evidence required"
    );
    let assignment = state
        .orders
        .iter()
        .find(|assignment| assignment.id == order)
        .context("unknown evidence order")?;
    let agent = state
        .agent(&assignment.agent)
        .context("unknown evidence author")?;
    let workplace = crate::lanes::workplace(task, agent)?;
    ensure!(
        evidence.order == order
            && evidence.base == assignment.base
            && evidence.revision == state.revision(order)
            && evidence.head == checks::head(&workplace.worktree)?,
        crate::failures::Stale("check evidence is stale".into())
    );
    let work = checks::inspect(
        &workplace.worktree,
        &assignment.base,
        &assignment.allowed,
        &assignment.forbidden,
    )?;
    ensure!(
        work.uncommitted.is_empty() && work.follows_the_rules(),
        "review source violates file policy"
    );
    crate::check_profiles::validate_configuration(task, &state, assignment, evidence)?;
    let stored: crate::check_profiles::Evidence =
        serde_json::from_slice(&std::fs::read(&evidence.run)?)?;
    ensure!(stored == *evidence, "persisted check artifact changed");
    crate::runner::content_hash(&serde_json::to_vec(evidence)?)
}
/// The reviews that sent a step's work back: each correction round counts
/// against the step's attempts, like another order does.
pub fn changes_requested(state: &crate::state::State, step: &str) -> u32 {
    let rounds = state
        .reviews
        .iter()
        .filter(|review| review.verdict == Some(Verdict::ChangesRequested))
        .filter(|review| {
            state
                .orders
                .iter()
                .any(|order| order.id == review.order && order.step == step)
        })
        .count();
    u32::try_from(rounds).unwrap_or(u32::MAX)
}

/// Before a reviewed step's review: its merge candidate passes the repo's
/// candidate profiles. Runs without the control lock (checks take minutes);
/// the review binds the result under it. A failing candidate fails the step,
/// for the leader to order a correction: the lanes break each other.
fn check_candidate_first(
    task: &Task,
    order_id: &str,
) -> Result<Option<crate::integration::CandidateEvidence>> {
    let state = task.state()?;
    if state.workflow != Workflow::Reviewed {
        return Ok(None);
    }
    let Some(order) = state.orders.iter().find(|order| order.id == order_id) else {
        return Ok(None);
    };
    let reported = state
        .steps
        .iter()
        .any(|step| step.id == order.step && step.status == StepStatus::Reported);
    let Some(lane) = state
        .agent(&order.agent)
        .and_then(|author| author.lane.clone())
    else {
        return Ok(None);
    };
    if !reported {
        return Ok(None);
    }
    let lane = crate::lanes::get(task, &lane)?;
    let candidate = crate::integration::candidate(task, &lane)?;
    let Some(evidence) = crate::integration::check_candidate(
        task,
        &lane.repo,
        Some(&lane.name),
        &candidate,
        !order.targets.is_empty(),
    )?
    else {
        return Ok(None);
    };
    if evidence.passed() {
        return Ok(Some(evidence));
    }
    let failed = failed_phases(&evidence.phases);
    let step = order.step.clone();
    let message = integration_failure(&step, &evidence.phases, !order.targets.is_empty());
    let environment_only = evidence
        .phases
        .iter()
        .filter(|phase| phase.outcome != crate::check_profiles::Outcome::Passed)
        .all(|phase| {
            matches!(
                phase.outcome,
                crate::check_profiles::Outcome::EnvironmentFailure
                    | crate::check_profiles::Outcome::RunnerFailure
            )
        });
    task.transaction(|tx| {
        if let Some(found) = tx.state.step_mut(&step) {
            found.set_status(StepStatus::Failed, crate::log::now());
        }
        crate::failures::record(
            &mut tx.state,
            &step,
            Some(&order.id),
            if environment_only {
                crate::failures::FailureKind::CheckEnvironment
            } else {
                crate::failures::FailureKind::IntegrationFailed
            },
            &failed.join("; "),
            crate::log::now(),
        );
        tx.state.say(crate::state::LEADER, message);
        tx.log(
            Kind::Check,
            None,
            Some(&step),
            &format!("candidate check failed: {}", failed.join("; ")),
        );
        Ok(())
    })?;
    anyhow::bail!(
        "the merge candidate of {step} fails its candidate check: {}",
        failed.join("; ")
    )
}

/// Each failed phase: its profile, outcome and diagnostic.
fn failed_phases(phases: &[crate::check_profiles::Phase]) -> Vec<String> {
    phases
        .iter()
        .filter(|phase| phase.outcome != crate::check_profiles::Outcome::Passed)
        .map(|phase| {
            format!(
                "{} ({:?}): {}",
                phase.profile, phase.outcome, phase.diagnostic
            )
        })
        .collect()
}

/// What the leader is told when a merge candidate fails: which profiles, why
/// they ran (every selected profile when the order's own checks ran only on
/// its targets) and the failing lines of each log, which neither it nor a
/// reviewer can open.
fn integration_failure(step: &str, phases: &[crate::check_profiles::Phase], full: bool) -> String {
    let mut message = format!(
        "INTEGRATION FAILED {step}: its work merged with the task's branch fails the candidate check: {}.",
        failed_phases(phases).join("; ")
    );
    if full {
        message.push_str(
            " Its order's checks ran only on its targets, so the candidate ran every selected profile on the whole repo: a failure may come from other work in the lane or the branch.",
        );
    }
    for phase in phases
        .iter()
        .filter(|phase| phase.outcome != crate::check_profiles::Outcome::Passed)
    {
        let excerpt = crate::check_profiles::failure_excerpt(&phase.log);
        if !excerpt.is_empty() {
            message.push_str(&format!("\n{} log: {excerpt}", phase.profile));
        }
    }
    message.push_str(
        "\nOrder a correction (an integration defect), or escalate when the failure is not this step's. ",
    );
    message.push_str(&crate::failures::guidance(step, None));
    message
}

pub fn request(task: &Task, order_id: &str, reviewer: &str) -> Result<Review> {
    let checked = check_candidate_first(task, order_id)?;
    let _control = task.control_lock()?;
    let state = task.state()?;
    let order = state
        .orders
        .iter()
        .find(|order| order.id == order_id)
        .context("unknown order")?;
    ensure!(
        state
            .steps
            .iter()
            .any(|step| step.id == order.step && step.status == StepStatus::Reported),
        "implementation must report passing checks first"
    );
    crate::orders::ensure_attempt_left(
        &state,
        crate::failures::work_failures(&state, &order.step),
        &order.step,
        task.config()?.max_attempts,
    )?;
    ensure!(
        order.agent != reviewer,
        "reviewer cannot review own implementation"
    );
    let reviewer_agent = state.agent(reviewer).context("unknown reviewer")?;
    ensure!(
        reviewer_agent.live && !reviewer_agent.writes && reviewer_agent.role == "code-reviewer",
        "assigned reviewer must be a live read-only code-reviewer"
    );
    let bound = bind(task, &state, order, checked.as_ref())?;
    let repo = bound.repo.clone();
    if state.workflow == Workflow::Reviewed {
        ensure!(
            !state
                .reviews
                .iter()
                .any(|review| review.reviewer == reviewer && review.verdict.is_none()),
            "reviewer already has an immutable pending assignment"
        );
        let identity = state
            .runner_identities
            .get(reviewer)
            .cloned()
            .context("reviewer identity missing")?;
        ensure!(
            !state.worker_turns.contains_key(reviewer),
            "reviewer is busy with another immutable assignment"
        );
        let directory = task.dir().join("review-candidates");
        std::fs::create_dir_all(&directory)?;
        let checkout = directory.join(format!("review-{}", state.next_id + 1));
        crate::runner::safe_git(
            &repo.worktree,
            &[
                "worktree",
                "add",
                "--detach",
                checkout.to_str().context("invalid review checkout path")?,
                &bound.review.candidate.commit,
            ],
        )?;
        crate::runner::prepare(task, reviewer, &checkout, true)?;
        task.transaction(|tx| {
            tx.state.runner_identities.insert(reviewer.into(), identity);
            Ok(())
        })?;
    }
    task.transaction(|tx| {
        tx.state.next_id += 1;
        let review = Review {
            id: format!("r{}", tx.state.next_id),
            reviewer: reviewer.into(),
            requested_at: crate::log::now(),
            ..bound.review
        };
        tx.state.reviews.push(review.clone());
        tx.state.say(reviewer,format!("Independently review {} for order {} using review_diff, inspect surrounding source through execute, then review_submit. Source {}, target {}, integration tree {}. Do not modify code.",review.id,review.order,review.source,review.target,review.candidate.tree));
        tx.log(
            Kind::Review,
            Some(reviewer),
            Some(&order.step),
            &format!("review {} requested", review.id),
        );
        Ok(review)
    })
}
/// What a review is bound to, taken once the candidate check passed: the
/// author's source, the task's target, the order's evidence and the exact
/// merge candidate. Shared by an independent review and a light acceptance.
struct Bound {
    repo: crate::task::Repo,
    review: Review,
}

fn bind(
    task: &Task,
    state: &crate::state::State,
    order: &crate::state::Order,
    checked: Option<&crate::integration::CandidateEvidence>,
) -> Result<Bound> {
    let author = state.agent(&order.agent).context("unknown author")?;
    let workplace = crate::lanes::workplace(task, author)?;
    let source = checks::head(&workplace.worktree)?;
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == author.repo)
        .context("unknown repo")?
        .clone();
    let target = checks::head(&repo.worktree)?;
    let lane = crate::lanes::get(
        task,
        author
            .lane
            .as_deref()
            .context("reviewed implementation requires an isolated lane")?,
    )?;
    let candidate = crate::integration::candidate(task, &lane)?;
    let candidate_evidence = match checked {
        Some(evidence) => {
            ensure!(
                evidence.matches(&candidate),
                "the branch moved during the candidate check: request the review again"
            );
            evidence.hash()?
        }
        None => String::new(),
    };
    let review = Review {
        id: "the requested review".into(),
        order: order.id.clone(),
        author: order.agent.clone(),
        baseline_revision: state.application.revision,
        order_revision: state.revision(&order.id),
        base: order.base.clone(),
        source,
        target,
        evidence: evidence(task, &order.id)?,
        candidate,
        candidate_evidence,
        ..Review::default()
    };
    crate::integration::require_candidate_evidence(task, &lane.repo, &review)?;
    Ok(Bound { repo, review })
}

/// A light item accepted on the leader's reading of its diff instead of an
/// independent review. It needs the item to be light now and a passing
/// candidate check; it is recorded as an approved review signed by the
/// leader, so the merge and every staleness rule treat it like any other.
pub fn accept_light(task: &Task, step: &str, reason: &str) -> Result<String> {
    ensure!(
        !reason.trim().is_empty(),
        "a light item needs accept_reason: what you read in the diff and why it is right"
    );
    let order_id = {
        let state = task.state()?;
        ensure!(
            state.workflow == Workflow::Reviewed,
            "light acceptance is for reviewed tasks"
        );
        light(task, &state, step)?;
        state
            .orders
            .iter()
            .rev()
            .find(|order| order.step == step)
            .map(|order| order.id.clone())
            .context("the step has no order")?
    };
    let checked = check_candidate_first(task, &order_id)?.context(
        "a light item skips the review only after a passing candidate check, and this \
         repo has none: request an independent review",
    )?;
    let _control = task.control_lock()?;
    let state = task.state()?;
    light(task, &state, step)?;
    let order = state
        .orders
        .iter()
        .find(|order| order.id == order_id)
        .context("unknown order")?;
    ensure!(
        state
            .steps
            .iter()
            .any(|found| found.id == step && found.status == StepStatus::Reported),
        "implementation must report passing checks first"
    );
    let bound = bind(task, &state, order, Some(&checked))?;
    task.transaction(|tx| {
        tx.state.next_id += 1;
        let now = crate::log::now();
        let review = Review {
            id: format!("r{}", tx.state.next_id),
            reviewer: crate::state::LEADER.into(),
            verdict: Some(Verdict::Approved),
            summary: Some(reason.trim().into()),
            requested_at: now,
            submitted_at: Some(now),
            ..bound.review
        };
        tx.log(
            Kind::Review,
            None,
            Some(step),
            &format!("light acceptance {}: {}", review.id, reason.trim()),
        );
        let id = review.id.clone();
        tx.state.reviews.push(review);
        Ok(id)
    })
}

/// The step's item is light now: a rise in rigor since (a failure, a new
/// sensitive path) takes the shortcut away.
fn light(task: &Task, state: &crate::state::State, step: &str) -> Result<()> {
    let routing = state
        .application
        .routing(step, crate::failures::work_failures(state, step))?
        .under(task.config()?.routing);
    ensure!(
        routing.rigor == crate::execution::Rigor::Light,
        "{step} is {:?}, not light: it needs an independent review{}",
        routing.rigor,
        if routing.why.is_empty() {
            String::new()
        } else {
            format!(" ({})", routing.why.join("; "))
        }
    );
    Ok(())
}

pub fn validate(task: &Task, review: &Review) -> Result<()> {
    let state = task.state()?;
    let order = state
        .orders
        .iter()
        .find(|order| order.id == review.order)
        .context("unknown review order")?;
    ensure!(
        review.author == order.agent && review.author != review.reviewer,
        "invalid review identities"
    );
    if review.reviewer == crate::state::LEADER {
        light(task, &state, &order.step)?;
    } else {
        let reviewer = state.agent(&review.reviewer).context("unknown reviewer")?;
        ensure!(
            !reviewer.writes && reviewer.role == "code-reviewer",
            "reviewer capability changed"
        );
    }
    ensure!(
        state.contract_bindings.get(&order.id) == state.application.items.get(&order.step)
            && state.revision(&order.id) == review.order_revision
            && order.base == review.base,
        "review revision is stale"
    );
    let author = state.agent(&order.agent).context("unknown author")?;
    let workplace = crate::lanes::workplace(task, author)?;
    ensure!(
        checks::head(&workplace.worktree)? == review.source,
        "review source changed"
    );
    ensure!(
        checks::inspect(
            &workplace.worktree,
            &review.base,
            &order.allowed,
            &order.forbidden
        )?
        .uncommitted
        .is_empty(),
        "review source is dirty"
    );
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == author.repo)
        .context("unknown repo")?;
    ensure!(
        crate::history::only_history_since(&repo.worktree, &review.target)?,
        crate::failures::Stale(
            "review target changed beyond the team's history: request a fresh review".into()
        )
    );
    ensure!(
        review.candidate.source == review.source
            && review.candidate.target == review.target
            && !review.candidate.commit.is_empty(),
        "integration candidate does not match review"
    );
    ensure!(
        checks::git(
            &repo.worktree,
            &[
                "rev-parse",
                &format!("{}^{{tree}}", review.candidate.commit)
            ]
        )?
        .trim()
            == review.candidate.tree,
        "candidate tree changed"
    );
    ensure!(
        evidence(task, &order.id)? == review.evidence,
        crate::failures::Stale("review evidence changed".into())
    );
    crate::integration::require_candidate_evidence(task, &repo.name, review)?;
    Ok(())
}
pub fn submit(
    task: &Task,
    pane: &str,
    id: &str,
    verdict: Verdict,
    findings: Vec<Finding>,
    summary: &str,
) -> Result<()> {
    let _control = task.control_lock()?;
    let state = task.state()?;
    let agent = state
        .agent_in_pane(pane)
        .context("review submission requires registered reviewer pane")?;
    let review = state
        .reviews
        .iter()
        .find(|review| review.id == id)
        .context("unknown review")?;
    ensure!(
        agent.id == review.reviewer && agent.live && !agent.writes,
        "foreign or inactive reviewer"
    );
    ensure!(
        review.verdict.is_none(),
        "review already submitted; request a fresh review for corrections"
    );
    ensure!(!summary.trim().is_empty(), "review summary required");
    for finding in &findings {
        ensure!(
            finding.line > 0
                && !finding.file.trim().is_empty()
                && !finding.rationale.trim().is_empty(),
            "actionable file/line/rationale required"
        );
        ensure!(
            !finding.blocking || finding.category.is_some(),
            "a blocking finding needs a category (requirement_ambiguity, stale_plan_interface, \
             scope_omission, environment_mismatch, implementation_defect, integration_defect \
             or test_weakening)"
        );
    }
    ensure!(
        verdict != Verdict::Approved
            || findings.iter().all(|finding| !finding.blocking
                || finding
                    .resolution
                    .as_ref()
                    .is_some_and(|reason| !reason.trim().is_empty())),
        "blocking findings remain"
    );
    validate(task, review)?;
    let step = state
        .orders
        .iter()
        .find(|order| order.id == review.order)
        .map(|order| order.step.clone())
        .context("unknown review order")?;
    // The leader may have ended its turn waiting for this review: the inbox
    // message is what starts its next one.
    let message = submitted_message(id, &agent.id, &step, &verdict, &findings, summary);
    let reviewer = agent.id.clone();
    task.transaction(|tx| {
        tx.state.say(crate::state::LEADER, message);
        let review = tx
            .state
            .reviews
            .iter_mut()
            .find(|review| review.id == id)
            .context("review disappeared")?;
        let now = crate::log::now();
        let failed = match verdict {
            Verdict::ChangesRequested => Some(crate::failures::FailureKind::ChangesRequested),
            Verdict::Blocked => Some(crate::failures::FailureKind::ReviewBlocked),
            Verdict::Approved => None,
        };
        let reviewed = review.order.clone();
        review.verdict = Some(verdict);
        review.findings = findings;
        review.summary = Some(summary.into());
        review.submitted_at = Some(now);
        let requested = review.requested_at;
        if let Some(kind) = failed {
            crate::failures::record(&mut tx.state, &step, Some(&reviewed), kind, summary, now);
        }
        if requested > 0 {
            tx.state
                .timing
                .record("review_wait", requested, now.max(requested))?;
        }
        tx.log(
            Kind::Review,
            Some(&reviewer),
            Some(&step),
            &format!("review {id} submitted"),
        );
        Ok(())
    })
}

/// What the leader reads when a review comes back: the verdict, the findings'
/// count, the reviewer's summary and the next move.
fn submitted_message(
    id: &str,
    reviewer: &str,
    step: &str,
    verdict: &Verdict,
    findings: &[Finding],
    summary: &str,
) -> String {
    const SUMMARY_CHARS: usize = 600;
    let blocking = findings.iter().filter(|finding| finding.blocking).count();
    let other = findings.len() - blocking;
    let mut summary: String = summary.trim().chars().take(SUMMARY_CHARS).collect();
    if summary.chars().count() == SUMMARY_CHARS {
        summary.push_str("… (the rest: application_workflow status, section reviews)");
    }
    let (outcome, next) = match verdict {
        Verdict::Approved => ("approved", format!("accept_step {step}")),
        Verdict::ChangesRequested => (
            "changes requested",
            "order the correction or answer the reviewer, then request a fresh review".into(),
        ),
        Verdict::Blocked => (
            "blocked",
            "resolve what blocks it or answer the reviewer, then request a fresh review".into(),
        ),
    };
    format!(
        "Reviewer {reviewer} submitted review {id} of step {step}: {outcome} \
         ({blocking} blocking, {other} other findings).\n{summary}\nNext: {next}."
    )
}
pub fn require_approval(task: &Task, step: &str) -> Result<()> {
    let state = task.state()?;
    if state.workflow == Workflow::Legacy {
        return Ok(());
    }
    let Some(order) = state.orders.iter().rev().find(|order| order.step == step) else {
        return Ok(());
    };
    let author = state.agent(&order.agent).context("unknown author")?;
    if !author.writes {
        return Ok(());
    }
    let review = state
        .reviews
        .iter()
        .rev()
        .find(|review| review.order == order.id)
        .context("independent code review required before merge")?;
    ensure!(
        review.verdict == Some(Verdict::Approved),
        "review is not approved"
    );
    validate(task, review)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The leader cannot open the candidate's logs: the message carries why
    // the whole suite ran and which tests failed.
    #[test]
    fn an_integration_failure_says_why_the_suite_ran_and_what_failed() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("phase-2.log");
        std::fs::write(
            &log,
            "  ✓ fine\n FAIL  src/hooks/useSystemMutations.test.tsx > keeps the mutation\nAssertionError: expected undefined\n",
        )
        .unwrap();
        let phase = |profile: &str, outcome| crate::check_profiles::Phase {
            profile: profile.into(),
            outcome,
            exit_code: Some(1),
            seconds: 1,
            diagnostic: "Isolated test command failed".into(),
            log: log.clone(),
        };
        let phases = [
            phase("lint", crate::check_profiles::Outcome::Passed),
            phase("test", crate::check_profiles::Outcome::TestFailure),
        ];

        let message = integration_failure("B3", &phases, true);
        assert!(message.starts_with("INTEGRATION FAILED B3"), "{message}");
        assert!(
            message.contains("test (TestFailure)") && !message.contains("lint ("),
            "{message}"
        );
        assert!(message.contains("ran only on its targets"), "{message}");
        assert!(
            message.contains("test log: FAIL  src/hooks/useSystemMutations.test.tsx")
                && message.contains("AssertionError"),
            "{message}"
        );
        assert!(
            !integration_failure("B3", &phases, false).contains("targets"),
            "an order checked everywhere ran the candidate's own profiles"
        );
    }

    #[test]
    fn a_submitted_review_tells_the_leader_what_comes_next() {
        let approved = submitted_message(
            "r312",
            "code-reviewer-6",
            "B1-2",
            &Verdict::Approved,
            &[],
            "All six files reviewed.",
        );
        assert!(
            approved.contains("code-reviewer-6 submitted review r312 of step B1-2"),
            "{approved}"
        );
        assert!(approved.contains("approved"), "{approved}");
        assert!(approved.contains("accept_step B1-2"), "{approved}");

        let finding = Finding {
            file: "web/package.json".into(),
            line: 1,
            severity: "high".into(),
            blocking: true,
            rationale: "no build evidence".into(),
            resolution: None,
            category: Some(Category::EnvironmentMismatch),
        };
        let changes = submitted_message(
            "r312",
            "code-reviewer-6",
            "B1-2",
            &Verdict::ChangesRequested,
            &[finding],
            &"x".repeat(2000),
        );
        assert!(changes.contains("changes requested"), "{changes}");
        assert!(changes.contains("1 blocking, 0 other"), "{changes}");
        assert!(changes.contains("fresh review"), "{changes}");
        assert!(!changes.contains("accept_step"), "{changes}");
        assert!(
            changes.len() < 1200,
            "the summary is cut: {}",
            changes.len()
        );
    }
    #[test]
    fn a_reviewer_reads_the_check_results_of_what_it_reviews() {
        use crate::check_profiles::{Evidence, Outcome, Phase};
        let phase = |profile: &str| Phase {
            profile: profile.into(),
            outcome: Outcome::Passed,
            exit_code: Some(0),
            seconds: 170,
            diagnostic: "ok".into(),
            log: "/x/log".into(),
        };
        let mut state = crate::state::State::default();
        let review = Review {
            order: "o161".into(),
            candidate: crate::integration::Candidate {
                tree: "t1".into(),
                ..Default::default()
            },
            ..Review::default()
        };
        state.check_evidence.insert(
            "o161".into(),
            Evidence {
                order: "o161".into(),
                revision: 0,
                base: "b".into(),
                head: "h".into(),
                configuration: "c".into(),
                run: "/x".into(),
                phases: vec![phase("lint"), phase("test")],
            },
        );
        let results = check_results(&state, &review);
        assert_eq!(results["order"]["passed"], true);
        assert_eq!(results["order"]["phases"][1]["profile"], "test");
        assert_eq!(results["order"]["phases"][1]["exit_code"], 0);
        assert!(
            results["order"]["phases"][1].get("log").is_none(),
            "a path it cannot open"
        );
        assert!(results["candidate"].is_null(), "no candidate check yet");
    }

    #[test]
    fn absent_review_blocks_before_acceptance_and_preserves_refs() {
        let root = tempfile::TempDir::new().unwrap();
        let (home, devws, task, repo) = crate::board::tests::started(&root);
        let team = crate::team::Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        team.write_plan("test plan").unwrap();
        team.approve().unwrap();
        let author = team.spawn_agent("executor", None).unwrap();
        let head = checks::head(repo.path()).unwrap();
        task.transaction(|tx| {
            tx.state.workflow = Workflow::Reviewed;
            tx.state.steps.push(crate::state::Step {
                id: "S01-B01".into(),
                title: "Implementation".into(),
                status: StepStatus::Reported,
                attempts: 1,
                spec: None,
                review_since: None,
            });
            tx.state.orders.push(crate::state::Order {
                read: Vec::new(),
                targets: Vec::new(),
                id: "o-review".into(),
                step: "S01-B01".into(),
                agent: author.id.clone(),
                text: "work".into(),
                allowed: vec![],
                forbidden: vec![],
                check: Some("true".into()),
                base: head.clone(),
                status: crate::state::OrderStatus::Closed,
                delivered: true,
                stalled: false,
                plan: None,
            });
            Ok(())
        })
        .unwrap();
        assert!(
            crate::orders::accept_step(&task, "S01-B01")
                .unwrap_err()
                .to_string()
                .contains("review")
        );
        assert_eq!(checks::head(repo.path()).unwrap(), head);
        assert_eq!(task.state().unwrap().steps[0].status, StepStatus::Reported);
        assert!(request(&task, "o-review", &author.id).is_err());
    }
    fn light_candidate(
        kind: crate::execution::WorkKind,
        allowed: &str,
    ) -> (tempfile::TempDir, Task) {
        let root = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&root);
        let head = checks::head(repo.path()).unwrap();
        task.transaction(|tx| {
            tx.state.workflow = Workflow::Reviewed;
            let mut application = crate::execution::tests::baseline();
            let item = application.items.get_mut("S01-B01").unwrap();
            item.kind = kind;
            item.allowed = vec![allowed.into()];
            tx.state.application = application;
            tx.state.steps.push(crate::state::Step {
                id: "S01-B01".into(),
                title: "Implementation".into(),
                status: StepStatus::Reported,
                attempts: 1,
                spec: None,
                review_since: None,
            });
            tx.state.orders.push(crate::state::Order {
                read: Vec::new(),
                targets: Vec::new(),
                id: "o1".into(),
                step: "S01-B01".into(),
                agent: "executor-1".into(),
                text: "work".into(),
                allowed: vec![],
                forbidden: vec![],
                check: None,
                base: head,
                status: crate::state::OrderStatus::Closed,
                delivered: true,
                stalled: false,
                plan: None,
            });
            Ok(())
        })
        .unwrap();
        (root, task)
    }

    #[test]
    fn a_light_acceptance_needs_a_reason_a_light_item_and_a_candidate_check() {
        use crate::execution::WorkKind;
        let (_root, task) = light_candidate(WorkKind::Change, "README.md");
        let error = accept_light(&task, "S01-B01", "  ")
            .unwrap_err()
            .to_string();
        assert!(error.contains("accept_reason"), "{error}");

        let error = accept_light(&task, "S01-B01", "one-line port fix, matches the server")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("only after a passing candidate check"),
            "no candidate check ran: {error}"
        );

        let (_root, task) = light_candidate(WorkKind::Feature, "src/notes.py");
        let error = accept_light(&task, "S01-B01", "looks right")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("S01-B01 is Standard, not light: it needs an independent review"),
            "{error}"
        );
        let (_root, task) = light_candidate(WorkKind::Change, "api/auth/session.py");
        let error = accept_light(&task, "S01-B01", "looks right")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("(sensitive path api/auth/session.py)"),
            "{error}"
        );
        assert!(task.state().unwrap().reviews.is_empty(), "nothing recorded");
    }

    #[test]
    fn a_leader_signed_review_goes_stale_once_the_item_is_no_longer_light() {
        let (_root, task) = light_candidate(crate::execution::WorkKind::Change, "README.md");
        task.transaction(|tx| {
            for (at, order) in [(10, "o-first"), (20, "o-second")] {
                crate::failures::record(
                    &mut tx.state,
                    "S01-B01",
                    Some(order),
                    crate::failures::FailureKind::CheckFailed,
                    "tests fail",
                    at,
                );
            }
            Ok(())
        })
        .unwrap();
        let review = Review {
            id: "r1".into(),
            order: "o1".into(),
            author: "executor-1".into(),
            reviewer: crate::state::LEADER.into(),
            verdict: Some(Verdict::Approved),
            ..Review::default()
        };
        let error = validate(&task, &review).unwrap_err().to_string();
        assert!(
            error.contains("is Strict, not light") && error.contains("2 work failures"),
            "{error}"
        );
    }

    #[test]
    fn review_rounds_asking_for_changes_count_against_the_steps_attempts() {
        let root = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&root);
        let head = checks::head(repo.path()).unwrap();
        task.transaction(|tx| {
            tx.state.workflow = Workflow::Reviewed;
            tx.state.steps.push(crate::state::Step {
                id: "S01-B01".into(),
                title: "Implementation".into(),
                status: StepStatus::Reported,
                attempts: 1,
                spec: None,
                review_since: None,
            });
            tx.state.orders.push(crate::state::Order {
                read: Vec::new(),
                targets: Vec::new(),
                id: "o1".into(),
                step: "S01-B01".into(),
                agent: "executor-1".into(),
                text: "work".into(),
                allowed: vec![],
                forbidden: vec![],
                check: None,
                base: head.clone(),
                status: crate::state::OrderStatus::Closed,
                delivered: true,
                stalled: false,
                plan: None,
            });
            for (number, verdict) in [
                Verdict::ChangesRequested,
                Verdict::ChangesRequested,
                Verdict::Blocked,
                Verdict::ChangesRequested,
            ]
            .into_iter()
            .enumerate()
            {
                tx.state.reviews.push(Review {
                    id: format!("r{number}"),
                    order: "o1".into(),
                    verdict: Some(verdict.clone()),
                    ..Review::default()
                });
                // Each round of changes ended an order of its own.
                if verdict == Verdict::ChangesRequested {
                    crate::failures::record(
                        &mut tx.state,
                        "S01-B01",
                        Some(&format!("o-round-{number}")),
                        crate::failures::FailureKind::ChangesRequested,
                        "changes requested",
                        number as u64,
                    );
                }
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(changes_requested(&task.state().unwrap(), "S01-B01"), 3);
        assert_eq!(changes_requested(&task.state().unwrap(), "other"), 0);

        // Three rounds of changes use up a limit of 3, whatever the default.
        task.transaction(|tx| {
            tx.state.attempt_limits.insert("S01-B01".into(), 3);
            Ok(())
        })
        .unwrap();
        let error = request(&task, "o1", "reviewer-1").unwrap_err().to_string();
        assert!(error.contains("3 attempts"), "{error}");

        crate::orders::allow_attempts(&task, "S01-B01", 4).unwrap();
        let error = request(&task, "o1", "reviewer-1").unwrap_err().to_string();
        assert!(
            !error.contains("attempts"),
            "the user's limit lets the next review through: {error}"
        );
    }

    #[test]
    fn findings_from_before_categories_load_and_blocking_ones_need_one() {
        let old: Finding = serde_json::from_value(serde_json::json!({
            "file": "a.rs", "line": 1, "severity": "high", "blocking": false,
            "rationale": "why", "resolution": null
        }))
        .unwrap();
        assert_eq!(old.category, None);
        let categorized: Finding = serde_json::from_value(serde_json::json!({
            "file": "a.rs", "line": 1, "severity": "high", "blocking": true,
            "rationale": "why", "resolution": null, "category": "test_weakening"
        }))
        .unwrap();
        assert_eq!(categorized.category, Some(Category::TestWeakening));
        let review: Review = serde_json::from_value(serde_json::json!({
            "id": "r1", "order": "o1", "author": "a", "reviewer": "b",
            "baseline_revision": 1, "order_revision": 0, "base": "x", "source": "y",
            "target": "z", "evidence": "e", "verdict": null, "findings": [], "summary": null
        }))
        .unwrap();
        assert_eq!((review.requested_at, review.submitted_at), (0, None));
    }

    #[test]
    fn a_blocking_finding_without_a_category_is_refused() {
        let root = tempfile::TempDir::new().unwrap();
        let (home, devws, task, _repo) = crate::board::tests::started(&root);
        let team = crate::team::Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let reviewer = team.spawn_agent("planner", None).unwrap();
        task.transaction(|tx| {
            tx.state.reviews.push(Review {
                id: "r1".into(),
                reviewer: reviewer.id.clone(),
                ..Review::default()
            });
            Ok(())
        })
        .unwrap();
        let finding = Finding {
            file: "a.rs".into(),
            line: 3,
            severity: "high".into(),
            blocking: true,
            rationale: "the assertion was removed".into(),
            resolution: None,
            category: None,
        };
        let error = submit(
            &task,
            &reviewer.pane,
            "r1",
            Verdict::ChangesRequested,
            vec![finding],
            "weakened test",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("needs a category"), "{error}");
    }

    #[test]
    fn foreign_reviewer_cannot_submit_or_forge_an_approval() {
        let root = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&root);
        assert!(
            submit(
                &task,
                "%not-registered",
                "r1",
                Verdict::Approved,
                vec![],
                "looks good"
            )
            .is_err()
        );
        assert!(task.state().unwrap().reviews.is_empty());
    }
}
