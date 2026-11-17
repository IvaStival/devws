//! Exact merge candidates and durable intent; approvals never follow moving refs.
use crate::{
    checks,
    execution::Workflow,
    reviews::{Review, Verdict},
    state::StepStatus,
    task::Task,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub source: String,
    pub target: String,
    pub tree: String,
    pub commit: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Journal {
    pub step: String,
    pub review: String,
    pub repo: String,
    pub candidate: Candidate,
    pub merged: bool,
    pub accepted: bool,
}

pub fn candidate(task: &Task, lane: &crate::task::Lane) -> Result<Candidate> {
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == lane.repo)
        .context("unknown integration repo")?;
    ensure!(
        crate::runner::safe_git(&repo.worktree, &["symbolic-ref", "--short", "HEAD"])?
            == config.branch,
        "task integration worktree is on a different branch"
    );
    let source = checks::head(&lane.worktree)?;
    let target = checks::head(&repo.worktree)?;
    let tree = if source == target {
        crate::runner::safe_git(
            &repo.worktree,
            &["rev-parse", &format!("{source}^{{tree}}")],
        )?
    } else {
        let result = crate::runner::safe_git(
            &repo.worktree,
            &["merge-tree", "--write-tree", &target, &source],
        )?;
        result
            .lines()
            .next()
            .context("missing merge candidate tree")?
            .into()
    };
    ensure!(
        tree.chars().all(|character| character.is_ascii_hexdigit()),
        "invalid integration tree"
    );
    let commit = if source == target {
        target.clone()
    } else {
        crate::runner::safe_git(
            &repo.worktree,
            &[
                "commit-tree",
                &tree,
                "-p",
                &target,
                "-p",
                &source,
                "-m",
                "Merge independently reviewed implementation",
            ],
        )?
    };
    Ok(Candidate {
        source,
        target,
        tree,
        commit,
    })
}
pub fn merge(task: &Task, lane: &crate::task::Lane, step: &str, review: &Review) -> Result<()> {
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == lane.repo)
        .context("unknown repo")?;
    let candidate = &review.candidate;
    ensure!(
        !candidate.commit.is_empty(),
        "review has no integration candidate"
    );
    ensure!(
        crate::runner::safe_git(&repo.worktree, &["symbolic-ref", "--short", "HEAD"])?
            == config.branch,
        "integration branch changed"
    );
    ensure!(
        crate::history::only_history_since(&repo.worktree, &candidate.target)?
            && checks::head(&lane.worktree)? == candidate.source,
        "review source or target is stale"
    );
    ensure!(
        checks::inspect(&repo.worktree, "HEAD", &[], &[])?
            .uncommitted
            .is_empty(),
        "integration worktree has uncommitted edits"
    );
    crate::reviews::validate(task, review)?;
    task.transaction(|tx| {
        tx.state.integrations.insert(
            step.into(),
            Journal {
                step: step.into(),
                review: review.id.clone(),
                repo: repo.name.clone(),
                candidate: candidate.clone(),
                merged: false,
                accepted: false,
            },
        );
        Ok(())
    })?;
    // History recorded since the review sits beside the reviewed code, so the
    // exact candidate is merged next to it rather than fast-forwarded.
    if checks::head(&repo.worktree)? == candidate.target {
        crate::runner::safe_git(
            &repo.worktree,
            &["merge", "--ff-only", "--no-edit", &candidate.commit],
        )?;
    } else {
        crate::runner::safe_git(
            &repo.worktree,
            &[
                "merge",
                "--no-ff",
                "-m",
                "Merge independently reviewed implementation beside the team's history",
                &candidate.commit,
            ],
        )?;
    }
    task.transaction(|tx| {
        tx.state
            .integrations
            .get_mut(step)
            .context("integration intent missing")?
            .merged = true;
        Ok(())
    })
}
/// Reconcile only the exact approved candidate, optionally followed by metadata-only history.
pub fn recover(task: &Task, step: &str) -> Result<bool> {
    let state = task.state()?;
    if state.workflow == Workflow::Legacy {
        return Ok(false);
    }
    let Some(journal) = state.integrations.get(step) else {
        return Ok(false);
    };
    if journal.accepted {
        return Ok(true);
    }
    let review = state
        .reviews
        .iter()
        .find(|review| review.id == journal.review)
        .context("integration review missing")?;
    ensure!(
        review.verdict == Some(Verdict::Approved) && review.candidate == journal.candidate,
        "integration intent lacks an exact approved review"
    );
    require_candidate_evidence(task, &journal.repo, review)?;
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == journal.repo)
        .context("integration repo missing")?;
    let merged = crate::runner::safe_git(
        &repo.worktree,
        &[
            "merge-base",
            "--is-ancestor",
            &journal.candidate.commit,
            "HEAD",
        ],
    )
    .is_ok();
    if !merged && crate::history::only_history_since(&repo.worktree, &journal.candidate.target)? {
        return Ok(false);
    }
    ensure!(
        merged,
        "integration recovery found unrelated branch advancement; inspect before recovery"
    );
    let changed = crate::runner::safe_git(
        &repo.worktree,
        &[
            "diff",
            "--name-only",
            &format!("{}..HEAD", journal.candidate.commit),
        ],
    )?;
    ensure!(
        changed
            .lines()
            .all(|path| path.starts_with(".devagentics/history/")),
        "integration recovery found unreviewed code after candidate"
    );
    task.transaction(|tx| {
        let found = tx.state.step_mut(step).context("integrated step missing")?;
        found.set_status(StepStatus::Accepted, crate::log::now());
        let journal = tx
            .state
            .integrations
            .get_mut(step)
            .context("integration journal missing")?;
        journal.merged = true;
        journal.accepted = true;
        tx.log(
            crate::log::Kind::Review,
            None,
            Some(step),
            "recovered exact approved integration candidate",
        );
        Ok(())
    })?;
    Ok(true)
}

/// The candidate profiles' results on one merge candidate: the lane's work
/// merged with the task's branch, before anyone reviews it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CandidateEvidence {
    pub repo: String,
    pub source: String,
    pub target: String,
    pub tree: String,
    pub configuration: String,
    pub phases: Vec<crate::check_profiles::Phase>,
}

impl CandidateEvidence {
    pub fn passed(&self) -> bool {
        !self.phases.is_empty()
            && self
                .phases
                .iter()
                .all(|phase| phase.outcome == crate::check_profiles::Outcome::Passed)
    }

    pub(crate) fn matches(&self, candidate: &Candidate) -> bool {
        self.source == candidate.source
            && self.target == candidate.target
            && self.tree == candidate.tree
    }

    pub fn hash(&self) -> Result<String> {
        crate::runner::content_hash(&serde_json::to_vec(self)?)
    }
}

/// The candidate profiles of `repo` and what their results depend on: the
/// profile definitions and the runner they ran in.
fn candidate_configuration(task: &Task, repo: &str) -> Result<(Vec<String>, String)> {
    let checks = task
        .config()?
        .check_profiles
        .get(repo)
        .cloned()
        .unwrap_or_default();
    let signature = serde_json::to_string(&(&checks, task.state()?.runner))?;
    Ok((checks.candidate, signature))
}

/// Runs the repo's candidate profiles on `candidate` in a disposable runner
/// workspace of its own (never an agent's identity), without holding the
/// control lock. None when the repo configures no candidate check. Passing
/// evidence for the same candidate and configuration is reused.
pub fn check_candidate(
    task: &Task,
    repo: &str,
    candidate: &Candidate,
) -> Result<Option<CandidateEvidence>> {
    let (profiles, configuration) = candidate_configuration(task, repo)?;
    if profiles.is_empty() || task.state()?.workflow == Workflow::Legacy {
        return Ok(None);
    }
    if let Some(found) = task.state()?.candidate_evidence.get(&candidate.tree)
        && found.matches(candidate)
        && found.configuration == configuration
        && found.passed()
    {
        return Ok(Some(found.clone()));
    }
    let config = task.config()?;
    let worktree = &config
        .repos
        .iter()
        .find(|found| found.name == repo)
        .context("unknown candidate repo")?
        .worktree;
    let definitions = crate::check_profiles::named(&config.check_profiles[repo], profiles)?;
    let short = &candidate.tree[..candidate.tree.len().min(12)];
    let directory = task.dir().join("candidate-checks").join(format!(
        "{short}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    std::fs::create_dir_all(&directory)?;
    let checkout = directory.join("source");
    crate::runner::safe_git(
        worktree,
        &[
            "worktree",
            "add",
            "--detach",
            checkout
                .to_str()
                .context("invalid candidate checkout path")?,
            &candidate.commit,
        ],
    )?;
    let worker = format!("candidate-{short}");
    let started = crate::log::now();
    let phases = run_candidate_phases(task, &worker, &checkout, &directory, definitions);
    // The snapshot holds its own copy; the checkout only had to outlive the runs.
    crate::runner::safe_git(
        worktree,
        &[
            "worktree",
            "remove",
            "--force",
            checkout
                .to_str()
                .context("invalid candidate checkout path")?,
        ],
    )?;
    let evidence = CandidateEvidence {
        repo: repo.into(),
        source: candidate.source.clone(),
        target: candidate.target.clone(),
        tree: candidate.tree.clone(),
        configuration,
        phases: phases?,
    };
    std::fs::write(
        directory.join("evidence.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    task.transaction(|tx| {
        tx.state
            .timing
            .record("candidate_checks", started, crate::log::now().max(started))?;
        tx.state
            .candidate_evidence
            .insert(evidence.tree.clone(), evidence.clone());
        Ok(())
    })?;
    Ok(Some(evidence))
}

fn run_candidate_phases(
    task: &Task,
    worker: &str,
    checkout: &std::path::Path,
    directory: &std::path::Path,
    definitions: Vec<(
        String,
        crate::check_profiles::Profile,
        crate::check_profiles::Environment,
    )>,
) -> Result<Vec<crate::check_profiles::Phase>> {
    crate::runner::prepare(task, worker, checkout, false)?;
    definitions
        .into_iter()
        .enumerate()
        .map(|(index, (name, profile, environment))| {
            crate::check_profiles::run_container_profile(
                task,
                worker,
                &name,
                &profile,
                &environment,
                directory,
                index,
            )
        })
        .collect()
}

/// A review may lead to a merge only with passing, current candidate
/// evidence for its exact candidate, when the repo configures candidate
/// profiles: the same rule for the merge, the acceptance and recovery.
pub fn require_candidate_evidence(task: &Task, repo: &str, review: &Review) -> Result<()> {
    let (profiles, configuration) = candidate_configuration(task, repo)?;
    // Candidate checks run in the reviewed runner; a legacy task has none.
    if profiles.is_empty() || task.state()?.workflow == Workflow::Legacy {
        return Ok(());
    }
    ensure!(
        !review.candidate_evidence.is_empty(),
        "review {} has no candidate check evidence: request a fresh review",
        review.id
    );
    let state = task.state()?;
    let evidence = state
        .candidate_evidence
        .get(&review.candidate.tree)
        .context("candidate check evidence is missing")?;
    ensure!(
        evidence.matches(&review.candidate),
        "candidate check evidence is for another candidate"
    );
    ensure!(evidence.passed(), "the candidate check did not pass");
    ensure!(
        evidence.configuration == configuration,
        "candidate check configuration changed: request a fresh review"
    );
    ensure!(
        evidence.hash()? == review.candidate_evidence,
        "candidate check evidence changed"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_profiles::{Outcome, Phase, RepositoryChecks};

    fn phase(outcome: Outcome) -> Phase {
        Phase {
            profile: "smoke".into(),
            outcome,
            exit_code: Some(0),
            seconds: 1,
            diagnostic: String::new(),
            log: std::path::PathBuf::new(),
        }
    }

    /// A reviewed task's repo with one candidate profile.
    fn with_candidate_profile(task: &Task, repo: &str) {
        task.transaction(|tx| {
            tx.state.workflow = Workflow::Reviewed;
            Ok(())
        })
        .unwrap();
        let checks: RepositoryChecks = serde_json::from_value(serde_json::json!({
            "environments": {"testing": {}},
            "checks": {"smoke": {"command": "true", "environment": "testing"}},
            "candidate": ["smoke"]
        }))
        .unwrap();
        crate::check_profiles::configure(task, repo, checks).unwrap();
    }

    fn candidate() -> Candidate {
        Candidate {
            source: "s".into(),
            target: "t".into(),
            tree: "abc".into(),
            commit: "c".into(),
        }
    }

    /// Records candidate evidence for `candidate()` with `outcome`, and a
    /// review bound to it.
    fn checked(task: &Task, repo: &str, outcome: Outcome) -> Review {
        let (_, configuration) = candidate_configuration(task, repo).unwrap();
        let evidence = CandidateEvidence {
            repo: repo.into(),
            source: "s".into(),
            target: "t".into(),
            tree: "abc".into(),
            configuration,
            phases: vec![phase(outcome)],
        };
        let hash = evidence.hash().unwrap();
        task.transaction(|tx| {
            tx.state.candidate_evidence.insert("abc".into(), evidence);
            Ok(())
        })
        .unwrap();
        Review {
            id: "r1".into(),
            candidate: candidate(),
            candidate_evidence: hash,
            ..Review::default()
        }
    }

    #[test]
    fn without_candidate_profiles_no_candidate_evidence_is_needed() {
        let root = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&root);
        let repo = task.config().unwrap().repos[0].name.clone();
        require_candidate_evidence(&task, &repo, &Review::default()).unwrap();
        assert_eq!(check_candidate(&task, &repo, &candidate()).unwrap(), None);

        with_candidate_profile(&task, &repo);
        task.transaction(|tx| {
            tx.state.workflow = Workflow::Legacy;
            Ok(())
        })
        .unwrap();
        require_candidate_evidence(&task, &repo, &Review::default()).unwrap();
        assert_eq!(
            check_candidate(&task, &repo, &candidate()).unwrap(),
            None,
            "a legacy task has no runner to check candidates in"
        );
    }

    #[test]
    fn a_merge_needs_passing_current_evidence_for_its_exact_candidate() {
        let root = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&root);
        let repo = task.config().unwrap().repos[0].name.clone();
        with_candidate_profile(&task, &repo);

        let error = require_candidate_evidence(&task, &repo, &Review::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("no candidate check evidence"), "{error}");

        let review = checked(&task, &repo, Outcome::Passed);
        require_candidate_evidence(&task, &repo, &review).unwrap();
        assert_eq!(
            check_candidate(&task, &repo, &candidate()).unwrap(),
            Some(task.state().unwrap().candidate_evidence["abc"].clone()),
            "passing evidence for the same candidate is reused, not rerun"
        );

        let other = Review {
            candidate: Candidate {
                target: "moved".into(),
                ..candidate()
            },
            ..review.clone()
        };
        let error = require_candidate_evidence(&task, &repo, &other)
            .unwrap_err()
            .to_string();
        assert!(error.contains("another candidate"), "{error}");

        let forged = Review {
            candidate_evidence: "0".repeat(64),
            ..review.clone()
        };
        assert!(require_candidate_evidence(&task, &repo, &forged).is_err());

        let failed = checked(&task, &repo, Outcome::TestFailure);
        let error = require_candidate_evidence(&task, &repo, &failed)
            .unwrap_err()
            .to_string();
        assert!(error.contains("did not pass"), "{error}");

        let review = checked(&task, &repo, Outcome::Passed);
        let mut config = task.config().unwrap();
        config
            .check_profiles
            .get_mut(&repo)
            .unwrap()
            .checks
            .get_mut("smoke")
            .unwrap()
            .command = "false".into();
        task.write_config(&config).unwrap();
        let error = require_candidate_evidence(&task, &repo, &review)
            .unwrap_err()
            .to_string();
        assert!(error.contains("configuration changed"), "{error}");
    }
}
