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
        checks::head(&repo.worktree)? == candidate.target
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
    crate::runner::safe_git(
        &repo.worktree,
        &["merge", "--ff-only", "--no-edit", &candidate.commit],
    )?;
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
    let config = task.config()?;
    let repo = config
        .repos
        .iter()
        .find(|repo| repo.name == journal.repo)
        .context("integration repo missing")?;
    let head = checks::head(&repo.worktree)?;
    if head == journal.candidate.target {
        return Ok(false);
    }
    ensure!(
        crate::runner::safe_git(
            &repo.worktree,
            &[
                "merge-base",
                "--is-ancestor",
                &journal.candidate.commit,
                "HEAD"
            ]
        )
        .is_ok(),
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
