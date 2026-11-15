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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub file: String,
    pub line: u64,
    pub severity: String,
    pub blocking: bool,
    pub rationale: String,
    pub resolution: Option<String>,
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
    pub verdict: Option<Verdict>,
    pub findings: Vec<Finding>,
    pub summary: Option<String>,
}
pub fn pending<'a>(state: &'a crate::state::State, reviewer: &str) -> Option<&'a Review> {
    state
        .reviews
        .iter()
        .find(|review| review.reviewer == reviewer && review.verdict.is_none())
}
pub fn resumption(state: &crate::state::State, reviewer: &str) -> Option<String> {
    pending(state, reviewer).map(|review| format!("Resume independent review {} for order {}. Call status and review_diff, inspect the immutable candidate through execute, then review_submit. Do not modify code.", review.id, review.order))
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
        "check evidence is stale"
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

pub fn request(task: &Task, order_id: &str, reviewer: &str) -> Result<Review> {
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
        changes_requested(&state, &order.step),
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
    let author = state.agent(&order.agent).context("unknown author")?;
    let workplace = crate::lanes::workplace(task, author)?;
    let source = checks::head(&workplace.worktree)?;
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == author.repo)
        .context("unknown repo")?;
    let target = checks::head(&repo.worktree)?;
    let evidence = evidence(task, order_id)?;
    let lane = crate::lanes::get(
        task,
        author
            .lane
            .as_deref()
            .context("reviewed implementation requires an isolated lane")?,
    )?;
    let candidate = crate::integration::candidate(task, &lane)?;
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
                &candidate.commit,
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
            order: order_id.into(),
            author: order.agent.clone(),
            reviewer: reviewer.into(),
            baseline_revision: state.application.revision,
            order_revision: state.revision(order_id),
            base: order.base.clone(),
            source,
            target,
            evidence,
            candidate,
            verdict: None,
            findings: vec![],
            summary: None,
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
    let reviewer = state.agent(&review.reviewer).context("unknown reviewer")?;
    ensure!(
        !reviewer.writes && reviewer.role == "code-reviewer",
        "reviewer capability changed"
    );
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
        checks::head(&repo.worktree)? == review.target,
        "review target changed"
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
        "review evidence changed"
    );
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
    task.transaction(|tx| {
        let review = tx
            .state
            .reviews
            .iter_mut()
            .find(|review| review.id == id)
            .context("review disappeared")?;
        review.verdict = Some(verdict);
        review.findings = findings;
        review.summary = Some(summary.into());
        tx.log(
            Kind::Review,
            Some(&agent.id),
            None,
            &format!("review {id} submitted"),
        );
        Ok(())
    })
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
                    verdict: Some(verdict),
                    ..Review::default()
                });
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(changes_requested(&task.state().unwrap(), "S01-B01"), 3);
        assert_eq!(changes_requested(&task.state().unwrap(), "other"), 0);

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
