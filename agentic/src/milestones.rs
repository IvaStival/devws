//! Final integration evidence is bound to code trees, not step numbering.
use crate::{checks, task::Task};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub baseline: u64,
    pub version: String,
    pub milestone: crate::execution::Milestone,
    pub contracts: BTreeMap<String, crate::execution::Contract>,
    pub code: BTreeMap<String, String>,
    pub configuration: String,
    pub phases: Vec<crate::check_profiles::Phase>,
}
pub fn code(task: &Task) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for repo in task.config()?.repos {
        let entries = checks::git(&repo.worktree, &["ls-tree", "-r", "HEAD"])?;
        let entries = entries
            .lines()
            .filter(|line| {
                !line
                    .split_once('\t')
                    .is_some_and(|(_, path)| path.starts_with(".devagentics/history/"))
            })
            .collect::<Vec<_>>()
            .join("\n");
        result.insert(repo.name, crate::runner::content_hash(entries.as_bytes())?);
    }
    Ok(result)
}
fn configuration(task: &Task) -> Result<String> {
    let config = task.config()?;
    Ok(serde_json::to_string(&(
        config.check_profiles,
        task.state()?.runner,
    ))?)
}
pub fn ready(task: &Task, id: &str) -> Result<bool> {
    let state = task.state()?;
    let Some(evidence) = state.milestone_evidence.get(id) else {
        return Ok(false);
    };
    Ok(
        state.application.milestones.get(id) == Some(&evidence.milestone)
            && evidence
                .contracts
                .iter()
                .all(|(id, contract)| state.application.items.get(id) == Some(contract))
            && evidence.code == code(task)?
            && evidence.configuration == configuration(task)?
            && !evidence.phases.is_empty()
            && evidence
                .phases
                .iter()
                .all(|phase| phase.outcome == crate::check_profiles::Outcome::Passed),
    )
}
pub fn validate(task: &Task, id: &str) -> Result<Evidence> {
    let _operation = task.control_lock()?;
    let state = task.state()?;
    ensure!(state.application.approved(), "baseline must be approved");
    let milestone = state
        .application
        .milestones
        .get(id)
        .context("unknown milestone")?;
    let mut selected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in &milestone.required {
        ensure!(
            state
                .steps
                .iter()
                .any(|step| step.id == *item && step.status == crate::state::StepStatus::Accepted),
            "milestone item {item} is not accepted"
        );
        let journal = state
            .integrations
            .get(item)
            .context("milestone implementation lacks reviewed integration")?;
        ensure!(
            journal.accepted
                && state
                    .reviews
                    .iter()
                    .any(|review| review.id == journal.review
                        && review.verdict == Some(crate::reviews::Verdict::Approved)
                        && review.candidate == journal.candidate),
            "milestone has missing review evidence"
        );
        let contract = state
            .application
            .items
            .get(item)
            .context("milestone contract missing")?;
        selected
            .entry(contract.repo.clone())
            .or_default()
            .extend(contract.profiles.iter().cloned());
    }
    for dependency in &milestone.dependencies {
        ensure!(
            state
                .milestone_evidence
                .get(dependency)
                .is_some_and(|evidence| evidence
                    .phases
                    .iter()
                    .all(|phase| phase.outcome == crate::check_profiles::Outcome::Passed)),
            "milestone dependency has not passed integration validation"
        );
    }
    let captured = code(task)?;
    let signature = configuration(task)?;
    let config = task.config()?;
    let directory = task.dir().join("milestone-checks").join(format!(
        "{id}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    std::fs::create_dir_all(&directory)?;
    // No mutable task/merge operation runs while the final snapshot is captured.
    let mut workspaces = Vec::new();
    for (repo_name, profiles) in selected {
        let repo = config
            .repos
            .iter()
            .find(|repo| repo.name == repo_name)
            .context("unknown milestone repo")?;
        let generation = directory
            .file_name()
            .context("missing validation generation")?
            .to_string_lossy();
        let worker = format!("milestone-{generation}-{repo_name}");
        crate::runner::prepare(task, &worker, &repo.worktree, false)?;
        let definitions = config
            .check_profiles
            .get(&repo_name)
            .context("final named checks are missing")?;
        for name in profiles {
            let profile = definitions
                .checks
                .get(&name)
                .context("final profile missing")?
                .clone();
            let environment = definitions
                .environments
                .get(&profile.environment)
                .context("final environment missing")?
                .clone();
            workspaces.push((worker.clone(), name, profile, environment));
        }
    }
    drop(_operation);
    let mut phases = Vec::new();
    for (index, (worker, name, profile, environment)) in workspaces.into_iter().enumerate() {
        phases.push(crate::check_profiles::run_container_profile(
            task,
            &worker,
            &name,
            &profile,
            &environment,
            &directory,
            index,
        )?);
    }
    let evidence = Evidence {
        baseline: state.application.revision,
        version: milestone.version.clone(),
        milestone: milestone.clone(),
        contracts: milestone
            .required
            .iter()
            .map(|id| (id.clone(), state.application.items[id].clone()))
            .collect(),
        code: captured,
        configuration: signature,
        phases,
    };
    let _control = task.control_lock()?;
    ensure!(
        code(task)? == evidence.code
            && configuration(task)? == evidence.configuration
            && task.state()?.application.revision == evidence.baseline,
        "final integration inputs changed; rerun"
    );
    std::fs::write(
        directory.join("evidence.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    task.transaction(|tx| {
        tx.state
            .milestone_evidence
            .insert(id.into(), evidence.clone());
        Ok(())
    })?;
    Ok(evidence)
}
