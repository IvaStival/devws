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
    let Some(evidence) = crate::integration::check_candidate(task, &lane.repo, &candidate)? else {
        return Ok(None);
    };
    if evidence.passed() {
        return Ok(Some(evidence));
    }
    let failed: Vec<String> = evidence
        .phases
        .iter()
        .filter(|phase| phase.outcome != crate::check_profiles::Outcome::Passed)
        .map(|phase| {
            format!(
                "{} ({:?}): {}",
                phase.profile, phase.outcome, phase.diagnostic
            )
        })
        .collect();
    let step = order.step.clone();
    task.transaction(|tx| {
        if let Some(found) = tx.state.step_mut(&step) {
            found.set_status(StepStatus::Failed, crate::log::now());
        }
        tx.state.say(
            crate::state::LEADER,
            format!(
                "INTEGRATION FAILED {step}: its work merged with the task's branch fails the \
                 candidate check: {}. Order a correction (an integration defect) or escalate.",
                failed.join("; ")
            ),
        );
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
    let candidate_evidence = match &checked {
        Some(evidence) => {
            ensure!(
                evidence.matches(&candidate),
                "the branch moved during the candidate check: request the review again"
            );
            evidence.hash()?
        }
        None => String::new(),
    };
    crate::integration::require_candidate_evidence(
        task,
        &lane.repo,
        &Review {
            id: "the requested review".into(),
            candidate: candidate.clone(),
            candidate_evidence: candidate_evidence.clone(),
            ..Review::default()
        },
    )?;
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
            candidate_evidence,
            verdict: None,
            findings: vec![],
            summary: None,
            requested_at: crate::log::now(),
            submitted_at: None,
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
    task.transaction(|tx| {
        let review = tx
            .state
            .reviews
            .iter_mut()
            .find(|review| review.id == id)
            .context("review disappeared")?;
        let now = crate::log::now();
        review.verdict = Some(verdict);
        review.findings = findings;
        review.summary = Some(summary.into());
        review.submitted_at = Some(now);
        let requested = review.requested_at;
        if requested > 0 {
            tx.state
                .timing
                .record("review_wait", requested, now.max(requested))?;
        }
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
        assert!(results["order"]["phases"][1].get("log").is_none(), "a path it cannot open");
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

        // Three rounds of changes use up a limit of 3, whatever the default.
        crate::orders::allow_attempts(&task, "S01-B01", 3).unwrap();
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
