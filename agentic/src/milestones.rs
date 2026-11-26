//! Final integration evidence is bound to code trees, not step numbering.
use crate::state::{OrderStatus, State, StepStatus};
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

fn validate_item(state: &State, item: &str) -> Result<()> {
    ensure!(
        state
            .steps
            .iter()
            .any(|step| step.id == item && step.status == StepStatus::Accepted),
        "milestone item {item} is not accepted"
    );
    if let Some(journal) = state.integrations.get(item) {
        ensure!(
            journal.accepted
                && state.reviews.iter().any(|review| {
                    review.id == journal.review
                        && review.verdict == Some(crate::reviews::Verdict::Approved)
                        && review.candidate == journal.candidate
                }),
            "milestone has missing review evidence"
        );
        return Ok(());
    }

    // An accepted findings-only order never produces a merge journal. Prior
    // writing orders still require integration evidence, even after reassignment.
    let contract = state
        .application
        .items
        .get(item)
        .context("milestone contract missing")?;
    let mut orders = state.orders.iter().rev().filter(|order| order.step == item);
    let latest = orders
        .next()
        .context("milestone implementation lacks reviewed integration")?;
    ensure!(
        latest.delivered
            && latest.status == OrderStatus::Closed
            && state
                .agent(&latest.agent)
                .is_some_and(|agent| !agent.writes && agent.role == contract.role)
            && orders.all(|order| state.agent(&order.agent).is_some_and(|agent| !agent.writes)),
        "milestone implementation lacks reviewed integration"
    );
    Ok(())
}

pub fn ready(task: &Task, id: &str) -> Result<bool> {
    let state = task.state()?;
    let Some(evidence) = state.milestone_evidence.get(id) else {
        return Ok(false);
    };
    Ok(
        state.application.milestones.get(id) == Some(&evidence.milestone)
            && evidence
                .milestone
                .required
                .iter()
                .all(|item| validate_item(&state, item).is_ok())
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
/// The profiles a milestone runs, by repo: its items' own, and the check of
/// every interface they provide, so the lanes are tested against what they
/// agreed on once their work is together.
/// The profiles a milestone runs, by repo: its items' own, the checks of the
/// interfaces they provide, and each repo's system checks (the application
/// starts and its flows work).
fn profiles(
    application: &crate::execution::Application,
    milestone: &crate::execution::Milestone,
    checks: &BTreeMap<String, crate::check_profiles::RepositoryChecks>,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut selected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in &milestone.required {
        let contract = application
            .items
            .get(item)
            .context("milestone contract missing")?;
        let repo = selected.entry(contract.repo.clone()).or_default();
        repo.extend(contract.profiles.iter().cloned());
        repo.extend(
            contract
                .provides
                .iter()
                .filter_map(|interface| application.interfaces.get(interface))
                .filter_map(|interface| interface.check.clone()),
        );
    }
    for (repo, names) in selected.iter_mut() {
        if let Some(checks) = checks.get(repo) {
            names.extend(checks.system.iter().cloned());
        }
    }
    Ok(selected)
}

/// Where each check runs: in the image of every lane that defines it, so the
/// milestone covers the backend as well as the frontend; a check no lane
/// defines runs in the repo's own image. A lane's tools exist only in its
/// image (`with-google-fonts` in the frontend's), so a check never runs in
/// a lane that does not define it, nor again in the repo's image.
fn placements(
    definitions: &crate::check_profiles::RepositoryChecks,
    profiles: &BTreeSet<String>,
) -> BTreeMap<Option<String>, Vec<String>> {
    let mut placed: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
    for name in profiles {
        let lanes: Vec<_> = definitions
            .lanes
            .iter()
            .filter(|(_, own)| own.contains_key(name))
            .map(|(lane, _)| Some(lane.clone()))
            .collect();
        if lanes.is_empty() {
            placed.entry(None).or_default().push(name.clone());
        }
        for lane in lanes {
            placed.entry(lane).or_default().push(name.clone());
        }
    }
    placed
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
    for item in &milestone.required {
        validate_item(&state, item)?;
    }
    let config = task.config()?;
    let selected = profiles(&state.application, milestone, &config.check_profiles)?;
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
        for (lane, names) in placements(definitions, &profiles) {
            let checks = definitions.for_lane(lane.as_deref());
            for (name, profile, environment) in crate::check_profiles::named(&checks, names)? {
                let label = lane
                    .as_ref()
                    .map_or_else(|| name.clone(), |lane| format!("{lane}:{name}"));
                workspaces.push((worker.clone(), lane.clone(), label, profile, environment));
            }
        }
    }
    drop(_operation);
    let mut phases = Vec::new();
    for (index, (worker, lane, name, profile, environment)) in workspaces.into_iter().enumerate() {
        phases.push(crate::check_profiles::run_container_profile(
            task,
            crate::check_profiles::Container {
                agent: &worker,
                lane: lane.as_deref(),
                targets: &[],
            },
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
        for item in &milestone.required {
            validate_item(&tx.state, item)?;
        }
        tx.state
            .milestone_evidence
            .insert(id.into(), evidence.clone());
        Ok(())
    })?;
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_milestone_runs_its_items_profiles_and_their_interface_checks() {
        let mut application = crate::execution::tests::baseline();
        application
            .items
            .get_mut("S01-B01")
            .unwrap()
            .provides
            .push("notes-api".into());
        application.interfaces.insert(
            "notes-api".into(),
            crate::execution::Interface {
                version: 1,
                owner: "S01-B01".into(),
                kind: crate::execution::InterfaceKind::Http,
                spec: "GET /notes".into(),
                check: Some("contract".into()),
            },
        );
        let selected = profiles(
            &application,
            &application.milestones["M01"],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(
            selected["app"],
            BTreeSet::from(["backend".to_string(), "contract".to_string()])
        );
    }

    #[test]
    fn a_milestone_runs_its_repos_system_checks_and_other_repos_do_not() {
        let application = crate::execution::tests::baseline();
        let system = |names: &[&str]| crate::check_profiles::RepositoryChecks {
            system: names.iter().map(|name| name.to_string()).collect(),
            ..Default::default()
        };
        let checks = BTreeMap::from([
            ("app".to_string(), system(&["e2e"])),
            ("elsewhere".to_string(), system(&["other-e2e"])),
        ]);
        let selected = profiles(&application, &application.milestones["M01"], &checks).unwrap();
        assert_eq!(
            selected["app"],
            BTreeSet::from(["backend".to_string(), "e2e".to_string()])
        );
        assert!(!selected.contains_key("elsewhere"), "not in this milestone");
    }

    #[test]
    fn a_check_runs_only_in_the_images_of_the_lanes_that_define_it() {
        let profile = |command: &str| crate::check_profiles::Profile {
            command: command.into(),
            environment: "testing".into(),
            timeout_seconds: 60,
            requires: Vec::new(),
            test_evidence: false,
        };
        let lane = |names: &[&str]| {
            names
                .iter()
                .map(|name| (name.to_string(), profile(name)))
                .collect::<BTreeMap<_, _>>()
        };
        let definitions = crate::check_profiles::RepositoryChecks {
            checks: lane(&["build", "test", "docs"]),
            lanes: BTreeMap::from([
                ("back".to_string(), lane(&["test"])),
                ("front".to_string(), lane(&["build", "test"])),
            ]),
            ..Default::default()
        };
        let names =
            |names: &[&str]| -> Vec<String> { names.iter().map(|name| name.to_string()).collect() };
        let set = |list: &[&str]| -> BTreeSet<String> { names(list).into_iter().collect() };
        let placed = placements(&definitions, &set(&["build", "docs", "test"]));
        assert_eq!(
            placed,
            BTreeMap::from([
                (None, names(&["docs"])),
                (Some("back".to_string()), names(&["test"])),
                (Some("front".to_string()), names(&["build", "test"])),
            ]),
            "build only where its tools are; test in each lane, not again in the repo's image"
        );
        let plain = crate::check_profiles::RepositoryChecks {
            checks: lane(&["test"]),
            ..Default::default()
        };
        assert_eq!(
            placements(&plain, &set(&["test"])),
            BTreeMap::from([(None, names(&["test"]))]),
            "a repo without lanes runs in its own image"
        );
    }

    fn audit() -> State {
        serde_json::from_value(json!({
            "application": {"items": {"audit": {"role": "planner"}}},
            "steps": [{"id": "audit", "title": "Audit", "status": "accepted", "attempts": 1}],
            "agents": [{
                "id": "planner-1", "role": "planner", "agent": "codex", "repo": "notes",
                "session": "dev-notes", "pane": "%3", "writes": false, "live": false, "ready": true
            }],
            "orders": [{
                "id": "o1", "step": "audit", "agent": "planner-1", "text": "Audit",
                "allowed": [], "forbidden": [], "base": "base", "status": "closed", "delivered": true
            }]
        })).unwrap()
    }

    #[test]
    fn a_completed_read_only_audit_does_not_require_a_merge() {
        let mut state = audit();
        validate_item(&state, "audit").unwrap();
        state.steps[0].status = StepStatus::Reported;
        assert!(validate_item(&state, "audit").is_err());
        state.steps[0].status = StepStatus::Accepted;
        state.orders[0].delivered = false;
        assert!(validate_item(&state, "audit").is_err());
    }

    #[test]
    fn a_past_writer_still_requires_integration_after_read_only_reassignment() {
        let mut state = audit();
        let mut writer = state.agents[0].clone();
        writer.id = "executor-1".into();
        writer.role = "executor".into();
        writer.writes = true;
        let mut writing_order = state.orders[0].clone();
        writing_order.id = "o0".into();
        writing_order.agent = writer.id.clone();
        state.agents.push(writer);
        state.orders.insert(0, writing_order);
        let error = validate_item(&state, "audit").unwrap_err();
        assert!(error.to_string().contains("lacks reviewed integration"));
    }
}
