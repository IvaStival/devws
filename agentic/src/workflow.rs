//! Project-local planning API shared by CLI and leader MCP tools.
use crate::{
    execution::{self, Application, Workflow},
    performance, reviews,
    task::Task,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

pub fn status(task: &Task) -> Result<Value> {
    let state = task.state()?;
    let config = task.config()?;
    let mut items = Vec::new();
    let home = crate::home::Home::at(
        task.dir()
            .parent()
            .and_then(std::path::Path::parent)
            .context("task home missing")?
            .to_path_buf(),
    );
    let live = state.agents.iter().filter(|agent| agent.live).count();
    for (id, item) in &state.application.items {
        let stage = state
            .steps
            .iter()
            .find(|step| step.id == *id)
            .map(|step| step.status);
        let assigned = state
            .orders
            .iter()
            .any(|order| order.step == *id && order.status == crate::state::OrderStatus::Open);
        let reusable = state.agents.iter().any(|agent| {
            agent.live
                && !agent.lost
                && agent.role == item.role
                && agent.lane.as_deref() == Some(&item.lane)
                && state.open_order_of(&agent.id).is_none()
                && !state.worker_turns.contains_key(&agent.id)
        });
        let reason = (|| -> Result<()> {
            state.application.ready(id, &state)?;
            ensure!(!state.paused, "task paused");
            ensure!(state.runner.is_some(), "isolated runner not configured");
            ensure!(!assigned, "assignment already in progress");
            ensure!(
                !matches!(
                    stage,
                    Some(crate::state::StepStatus::Accepted | crate::state::StepStatus::Reported)
                ),
                "completed or awaiting independent review"
            );
            let repo = config
                .repos
                .iter()
                .find(|repo| repo.name == item.repo)
                .context("repository not configured")?;
            let roles = crate::roles::Roles::load(&home, Some(&repo.worktree))?;
            ensure!(
                roles.get(&item.role).is_some_and(|role| role.writes),
                "implementation specialist role missing or read-only"
            );
            ensure!(
                config
                    .lanes
                    .iter()
                    .any(|lane| lane.name == item.lane && lane.repo == item.repo),
                "implementation lane not configured"
            );
            let profiles = config
                .check_profiles
                .get(&item.repo)
                .context("named checks not configured")?;
            ensure!(
                item.profiles
                    .iter()
                    .all(|name| profiles.checks.contains_key(name)),
                "required named profile missing"
            );
            ensure!(
                reusable || live < config.max_agents,
                "agent capacity occupied; reuse/release idle staffing before adding a writer"
            );
            Ok(())
        })()
        .err()
        .map(|error| error.to_string());
        items.push(json!({"id":id,"scope":item.scope,"role":item.role,"lane":item.lane,"stage":stage,"assigned":assigned,"reusable_worker":reusable,"ready":reason.is_none(),"blocked":reason}));
    }
    // A repo without candidate profiles merges reviewed work untested in
    // combination until the milestone check: say so.
    let candidate_checks: serde_json::Map<String, Value> = config
        .repos
        .iter()
        .map(|repo| {
            let profiles = config
                .check_profiles
                .get(&repo.name)
                .map(|checks| checks.candidate.clone())
                .unwrap_or_default();
            let warning = profiles.is_empty().then(|| {
                "no candidate check: merges are first tested together at the milestone".to_string()
            });
            (
                repo.name.clone(),
                json!({"profiles":profiles,"warning":warning}),
            )
        })
        .collect();
    Ok(
        json!({"workflow":state.workflow,"candidate_checks":candidate_checks,"application":state.application,"proposal":state.application_proposal,"interview_frontier":execution::question_frontier(state.application_proposal.as_ref().unwrap_or(&state.application)),"ready_work":items,"reviews":state.reviews,"performance":performance::update(task)?,"capacity":{"limit":config.max_agents,"live":state.agents.iter().filter(|agent|agent.live).count()},"runner":{"configured":state.runner.is_some(),"available":state.runner.is_some(),"adapter":"claude_mcp_only","network":"none","live_project_mounts":false,"configuration":state.runner}}),
    )
}
pub fn adoption(task: &Task) -> Result<Value> {
    let state = task.state()?;
    let config = task.config()?;
    let direct = state
        .agents
        .iter()
        .filter(|agent| agent.writes && agent.live && agent.lane.is_none())
        .map(|agent| agent.id.clone())
        .collect::<Vec<_>>();
    Ok(
        json!({"current":state.workflow,"paused":state.paused,"direct_branch_writers":direct,"open_orders":state.orders.iter().filter(|order|order.status==crate::state::OrderStatus::Open).map(|order|&order.id).collect::<Vec<_>>(),"managed_storage":task.dir(),"repositories":config.repos.iter().map(|repo|&repo.worktree).collect::<Vec<_>>(),"blockers": if state.runner.is_none(){vec!["Configure and verify an isolated local image first"]}else if !state.paused{vec!["Pause task before adoption"]}else if state.agents.iter().any(|worker|worker.live){vec!["Stop unscoped legacy agents before adoption"]}else if state.orders.iter().any(|order|order.status==crate::state::OrderStatus::Open){vec!["Resolve open legacy orders before adoption"]}else{vec![]},"can_apply":state.runner.is_some() && state.paused && !state.agents.iter().any(|worker|worker.live) && !state.orders.iter().any(|order|order.status==crate::state::OrderStatus::Open)}),
    )
}
/// Caller must be the leader/user. Worker tools never expose this catalog.
pub fn call(task: &Task, action: &str, input: &Value) -> Result<Value> {
    match action {
        "status" => status(task),
        "runner_images" => crate::runner::images(),
        "project_overview" => crate::runner::overview(task),
        "preflight_retry" => {
            let item = input["item"].as_str().context("item required")?;
            task.transaction(|tx| {
                tx.state.environment_preflight.remove(item);
                Ok(())
            })?;
            Ok(json!({"retry_enabled":item}))
        }
        "leader_inbox" => Ok(json!(
            task.state()?
                .queue
                .into_iter()
                .filter(|message| message.to == crate::state::LEADER)
                .collect::<Vec<_>>()
        )),
        "inbox_ack" => {
            let ids = serde_json::from_value::<Vec<u64>>(input["ids"].clone())?;
            task.transaction(|tx| {
                ensure!(
                    ids.iter().all(|id| tx
                        .state
                        .queue
                        .iter()
                        .any(|message| message.id == *id && message.to == crate::state::LEADER)),
                    "cannot acknowledge foreign or already consumed notifications"
                );
                tx.state.queue.retain(|message| !ids.contains(&message.id));
                Ok(())
            })?;
            Ok(json!({"acknowledged":ids}))
        }
        "preview_adoption" => adoption(task),
        "provision_roles" => {
            ensure!(
                input["approved_storage"].as_bool() == Some(true),
                "explicit approval for the task home role-storage target is required"
            );
            let home = crate::home::Home::at(
                task.dir()
                    .parent()
                    .and_then(std::path::Path::parent)
                    .context("task home missing")?
                    .to_path_buf(),
            );
            crate::roles::provision_specialists(&home)?;
            Ok(json!({"installed":true,"target":home.roles_dir()}))
        }
        "runner_configure" => {
            crate::runner::configure(
                task,
                serde_json::from_value(input["configuration"].clone())?,
            )?;
            Ok(
                json!({"configured":true,"adapter":"claude_mcp_only","network":"none","live_project_mounts":false}),
            )
        }
        "recover_turn" => {
            let agent = input["agent"].as_str().context("agent required")?;
            ensure!(
                input["resend_confirmed"].as_bool() == Some(true),
                "inspect the uncertain turn and explicitly confirm resend first"
            );
            let _control = task.control_lock()?;
            task.transaction(|tx| {
                ensure!(
                    !tx.state
                        .agents
                        .iter()
                        .any(|worker| worker.id == agent && worker.live && !worker.lost),
                    "stop the failed worker before recovery"
                );
                let message = tx
                    .state
                    .worker_turns
                    .remove(agent)
                    .context("no uncertain turn")?;
                tx.state.queue.push(message);
                Ok(())
            })?;
            Ok(json!({"queued":true}))
        }
        "adopt" => {
            let configured = task
                .state()?
                .runner
                .context("configure the isolated runner before adoption")?;
            crate::runner::probe(&configured)?;
            let _control = task.control_lock()?;
            task.transaction(|tx|{ensure!(tx.state.paused,"pause task before explicit adoption");ensure!(!tx.state.agents.iter().any(|worker|worker.live),"stop legacy agents before adopting scoped workers");ensure!(!tx.state.orders.iter().any(|order|order.status==crate::state::OrderStatus::Open),"resolve active legacy orders before adoption");tx.state.workflow=Workflow::Reviewed;tx.state.approved=tx.state.application.approved();tx.log(crate::log::Kind::Review,None,None,"user adopted reviewed execution for future work; historical steps retain legacy provenance");Ok(())})?;
            Ok(json!({"adopted":true}))
        }
        "answer" => {
            let id = input["question"].as_str().context("question required")?;
            let answer = input["answer"]
                .as_str()
                .filter(|answer| !answer.trim().is_empty())
                .context("nonempty answer required")?;
            let _control = task.control_lock()?;
            task.transaction(|tx| {
                let application = tx
                    .state
                    .application_proposal
                    .as_mut()
                    .context("no planning proposal")?;
                ensure!(
                    execution::question_frontier(application)
                        .iter()
                        .any(|decision| decision.id == id),
                    "question is already answered or prerequisites are unresolved"
                );
                let decision = application
                    .decisions
                    .iter_mut()
                    .find(|decision| decision.id == id)
                    .context("unknown question")?;
                decision.answer = Some(answer.into());
                application.confirmed_revision = None;
                application.approved_revision = None;
                tx.log(
                    crate::log::Kind::Answer,
                    None,
                    None,
                    &format!("user answered planning question {id}"),
                );
                Ok(())
            })?;
            Ok(json!({"recorded":id}))
        }
        "propose" => {
            let application: Application = serde_json::from_value(
                input
                    .get("application")
                    .context("application required")?
                    .clone(),
            )?;
            let affected = execution::store(
                task,
                application,
                input["reason"].as_str().context("reason required")?,
            )?;
            Ok(
                json!({"affected":affected,"next":"Planner interview/confirmation, then explicit baseline approval"}),
            )
        }
        "confirm" | "approve" => {
            execution::confirm(
                task,
                input["revision"]
                    .as_u64()
                    .context("revision must be an integer")?,
                action == "approve",
            )?;
            Ok(json!({"recorded":action}))
        }
        "review_request" => Ok(serde_json::to_value(reviews::request(
            task,
            input["order"].as_str().context("order required")?,
            input["reviewer"].as_str().context("reviewer required")?,
        )?)?),
        "validate_milestone" => Ok(serde_json::to_value(crate::milestones::validate(
            task,
            input["milestone"].as_str().context("milestone required")?,
        )?)?),
        "milestones" => milestones(task),
        _ => bail!("unknown workflow action: {action}"),
    }
}
pub fn milestones(task: &Task) -> Result<Value> {
    let state = task.state()?;
    let mut result = Vec::new();
    for (id, milestone) in &state.application.milestones {
        let accepted = milestone.required.iter().all(|item| {
            state
                .steps
                .iter()
                .any(|step| step.id == *item && step.status == crate::state::StepStatus::Accepted)
        });
        let status = if milestone.release_record.is_some() {
            "released"
        } else if accepted && crate::milestones::ready(task, id)? {
            "ready"
        } else if accepted {
            "integration_validation_required"
        } else if milestone
            .required
            .iter()
            .any(|item| state.steps.iter().any(|step| step.id == *item))
        {
            "in_progress"
        } else {
            "planned"
        };
        result.push(json!({"id":id,"version":milestone.version,"title":milestone.title,"status":status,"required":milestone.required,"dependencies":milestone.dependencies}));
    }
    Ok(json!(result))
}
pub fn block_unverified_runner(task: &Task) -> Result<()> {
    crate::runner::available(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::tests::baseline;
    #[test]
    fn status_warns_about_a_repo_without_a_candidate_check() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let repo = task.config().unwrap().repos[0].name.clone();
        let warning =
            |task: &Task| status(task).unwrap()["candidate_checks"][&repo]["warning"].clone();
        assert!(
            warning(&task)
                .as_str()
                .unwrap()
                .contains("no candidate check")
        );

        let checks = serde_json::from_value(serde_json::json!({
            "environments": {"testing": {}},
            "checks": {"smoke": {"command": "true", "environment": "testing"}},
            "candidate": ["smoke"]
        }))
        .unwrap();
        crate::check_profiles::configure(&task, &repo, checks).unwrap();
        assert!(warning(&task).is_null());
        assert_eq!(
            status(&task).unwrap()["candidate_checks"][&repo]["profiles"][0],
            "smoke"
        );
    }
    #[test]
    fn approved_baseline_prevents_silent_check_weakening() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let repo = task.config().unwrap().repos[0].name.clone();
        let mut proposal = baseline();
        let definitions = proposal.check_configuration.remove("app").unwrap();
        proposal.items.get_mut("S01-B01").unwrap().repo = repo.clone();
        proposal
            .check_configuration
            .insert(repo.clone(), definitions.clone());
        crate::check_profiles::configure(&task, &repo, definitions.clone()).unwrap();
        crate::execution::store(&task, proposal, "complete checks").unwrap();
        crate::execution::confirm(&task, 1, false).unwrap();
        crate::execution::confirm(&task, 1, true).unwrap();
        task.transaction(|tx| {
            tx.state.workflow = Workflow::Reviewed;
            Ok(())
        })
        .unwrap();
        let mut changed = definitions;
        changed.checks.get_mut("backend").unwrap().command = "false".into();
        assert!(crate::check_profiles::configure(&task, &repo, changed).is_err());
        assert_eq!(
            task.config().unwrap().check_profiles[&repo].checks["backend"].command,
            "true"
        );
    }
    #[test]
    fn material_amendments_preserve_unrelated_accepted_work_and_stable_ids() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let mut proposal = baseline();
        proposal
            .scopes
            .insert("S02".into(), "Independent scope".into());
        let mut item = proposal.items["S01-B01"].clone();
        item.scope = "S02".into();
        proposal.items.insert("S02-B01".into(), item);
        proposal.milestones.insert(
            "M02".into(),
            crate::execution::Milestone {
                version: "1.2.0".into(),
                title: "Second milestone".into(),
                required: vec!["S02-B01".into()],
                ..Default::default()
            },
        );
        crate::execution::store(&task, proposal, "whole app").unwrap();
        crate::execution::confirm(&task, 1, false).unwrap();
        crate::execution::confirm(&task, 1, true).unwrap();
        task.transaction(|tx| {
            for id in ["S01-B01", "S02-B01"] {
                tx.state.steps.push(crate::state::Step {
                    id: id.into(),
                    title: id.into(),
                    status: crate::state::StepStatus::Accepted,
                    attempts: 1,
                    spec: None,
                    review_since: None,
                });
            }
            Ok(())
        })
        .unwrap();
        let mut changed = task.state().unwrap().application;
        changed.revision = 2;
        changed
            .items
            .get_mut("S01-B01")
            .unwrap()
            .behavior
            .push("New requirement".into());
        crate::execution::store(&task, changed, "material scope change").unwrap();
        crate::execution::confirm(&task, 2, false).unwrap();
        crate::execution::confirm(&task, 2, true).unwrap();
        let state = task.state().unwrap();
        assert_eq!(state.steps[0].status, crate::state::StepStatus::Rejected);
        assert_eq!(state.steps[1].status, crate::state::StepStatus::Accepted);
        assert_eq!(state.application.milestones["M02"].version, "1.2.0");
    }
    #[test]
    fn coordinator_inbox_acknowledgement_cannot_consume_worker_messages() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        task.transaction(|tx| {
            tx.state.say(crate::state::LEADER, "report".into());
            tx.state.say("worker-1", "private assignment".into());
            Ok(())
        })
        .unwrap();
        let state = task.state().unwrap();
        let leader = state.queue[0].id;
        let worker = state.queue[1].id;
        assert!(call(&task, "inbox_ack", &json!({"ids":[worker]})).is_err());
        call(&task, "inbox_ack", &json!({"ids":[leader]})).unwrap();
        assert_eq!(task.state().unwrap().queue[0].to, "worker-1");
    }

    #[test]
    fn proposal_requires_exact_confirmation_then_approval_and_keeps_old_baseline() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&dir);
        let proposal = baseline();
        execution::store(&task, proposal, "initial app").unwrap();
        assert_eq!(task.state().unwrap().application.revision, 0);
        assert!(execution::confirm(&task, 1, true).is_err());
        assert!(execution::confirm(&task, 2, false).is_err());
        execution::confirm(&task, 1, false).unwrap();
        execution::confirm(&task, 1, true).unwrap();
        assert!(task.state().unwrap().application.approved());
        let mut changed = task.state().unwrap().application;
        changed.revision = 2;
        changed.goal = "Additional requirements".into();
        execution::store(&task, changed, "material product change").unwrap();
        assert!(task.state().unwrap().application.approved());
        assert_eq!(task.state().unwrap().application.revision, 1);
        assert!(
            task.state()
                .unwrap()
                .application_proposal
                .unwrap()
                .approved_revision
                .is_none()
        );
        assert!(execution::confirm(&task, 1, true).is_err());
    }
    #[test]
    fn unverified_adoption_and_launch_make_no_configuration_or_branch_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, repo) = crate::board::tests::started(&dir);
        let head = crate::checks::head(repo.path()).unwrap();
        let before = task.state().unwrap();
        let config = task.config().unwrap();
        assert_eq!(adoption(&task).unwrap()["can_apply"], false);
        assert!(call(&task, "adopt", &json!({})).is_err());
        assert_eq!(task.state().unwrap(), before);
        assert_eq!(task.config().unwrap(), config);
        task.transaction(|tx| {
            tx.state.workflow = Workflow::Reviewed;
            Ok(())
        })
        .unwrap();
        assert!(block_unverified_runner(&task).is_err());
        assert_eq!(crate::checks::head(repo.path()).unwrap(), head);
    }
    #[test]
    fn released_status_cannot_be_forged_in_a_plan_submission() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&dir);
        let mut proposal = baseline();
        proposal.milestones.get_mut("M01").unwrap().release_record = Some("trust me".into());
        assert!(execution::store(&task, proposal, "release claim").is_err());
    }
    #[test]
    fn portable_specialist_roles_never_overwrite_existing_customizations() {
        let root = tempfile::TempDir::new().unwrap();
        let home = crate::home::Home::at(root.path().into());
        crate::roles::provision_specialists(&home).unwrap();
        let path = home.roles_dir().join("backend-data.md");
        let original = std::fs::read_to_string(&path).unwrap();
        assert!(original.contains("SOLID") && original.contains("database"));
        std::fs::write(&path, "customized").unwrap();
        crate::roles::provision_specialists(&home).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "customized");
    }
}
