//! Project-local planning API shared by CLI and leader MCP tools.
use crate::{
    execution::{self, Workflow},
    performance, reviews,
    task::Task,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

/// Each baseline item: its stage, whether it is assigned, and whether it is
/// ready to start or what blocks it.
pub fn ready_work(task: &Task) -> Result<Vec<Value>> {
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
                    Some(
                        crate::state::StepStatus::Accepted
                            | crate::state::StepStatus::Reported
                            | crate::state::StepStatus::Closed
                    )
                ),
                "completed, closed or awaiting independent review"
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
                reusable || state.live_agents(true) < config.max_agents,
                "writer capacity occupied; reuse an idle writer or stop one before adding a writer"
            );
            Ok(())
        })()
        .err()
        .map(|error| error.to_string());
        let routing = state
            .application
            .routing(id, crate::failures::work_failures(&state, id))?
            .under(config.routing);
        items.push(json!({"id":id,"scope":item.scope,"role":item.role,"lane":item.lane,"kind":routing.kind,"rigor":routing.rigor,"required":routing.rigor.required(),"rigor_why":routing.why,"stage":stage,"assigned":assigned,"reusable_worker":reusable,"ready":reason.is_none(),"blocked":reason}));
    }
    Ok(items)
}

pub fn status(task: &Task) -> Result<Value> {
    let state = task.state()?;
    let config = task.config()?;
    let items = ready_work(task)?;
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
    // Who approves the pending proposal: the user for a change to what the
    // work is for or what judges it, the leader for the rest.
    let proposal_approval = state.application_proposal.as_ref().map(|proposal| {
        let why = execution::user_approval_needed(&state.application, proposal);
        json!({"needs_user": why.is_some(), "why": why.unwrap_or_else(|| "it changes only how steps are carried out, adds steps or adds checks: confirm nothing, approve it yourself and tell the user in one line of your next summary what changed and why".into())})
    });
    Ok(
        json!({"workflow":state.workflow,"proposal_approval":proposal_approval,"candidate_checks":candidate_checks,"application":state.application,"proposal":state.application_proposal,"interview_frontier":execution::question_frontier(state.application_proposal.as_ref().unwrap_or(&state.application)),"ready_work":items,"reviews":state.reviews,"performance":performance::update(task)?,"capacity":{"limit":config.max_agents,"writers":state.live_agents(true),"read_only":state.live_agents(false)},"runner":{"configured":state.runner.is_some(),"available":state.runner.is_some(),"adapter":if state.codex_team {"claude_mcp_only or opencode_mcp_only or codex_sandbox: the user put the whole team on Codex (task codex-team); Codex workers write only their snapshot, without network"} else {"claude_mcp_only or opencode_mcp_only"},"worker_programs":if state.codex_team {vec!["claude","opencode","codex"]} else {vec!["claude","opencode"]},"reviewer_adapter":if state.codex_reviewers || state.codex_team {"codex_read_only_sandbox: cannot write; its shell can read host files"} else {"claude_mcp_only or opencode_mcp_only"},"network":"none","live_project_mounts":false,"configuration":state.runner}}),
    )
}
/// The status small enough to come back in one call: the baseline and the
/// proposal as outlines, and how to read the rest.
pub fn compact(task: &Task) -> Result<Value> {
    let mut status = status(task)?;
    let state = task.state()?;
    status["application"] = execution::outline(&state.application);
    if let Some(proposal) = &state.application_proposal {
        status["proposal"] = execution::outline(proposal);
    }
    slim_growing_parts(&mut status, &state);
    status["read_more"] = json!(format!(
        "status with section, one part in full: {}; section application or proposal with items [ids]: those items' full contracts",
        SECTIONS.join(", ")
    ));
    Ok(status)
}

/// The parts of the status a section names.
const SECTIONS: [&str; 9] = [
    "application",
    "proposal",
    "reviews",
    "ready_work",
    "runner",
    "candidate_checks",
    "interview_frontier",
    "performance",
    "capacity",
];

/// Reviews and accepted items only grow as the team works: keep each review's
/// outcome and the work still to do; their sections have the rest.
fn slim_growing_parts(status: &mut Value, state: &crate::state::State) {
    status["reviews"] = json!(state.reviews.iter().map(|review| json!({"id":review.id,"order":review.order,"author":review.author,"reviewer":review.reviewer,"verdict":review.verdict,"findings":review.findings.len(),"submitted_at":review.submitted_at})).collect::<Vec<_>>());
    let ready_work = status["ready_work"].as_array().cloned().unwrap_or_default();
    let (accepted, open): (Vec<_>, Vec<_>) = ready_work
        .into_iter()
        .partition(|item| item["stage"] == "accepted");
    status["ready_work"] = json!(open);
    status["accepted_items"] = json!(accepted.len());
}

/// The status inside `task_status`, which carries much else: of the baseline
/// and the proposal only their revisions and size.
pub fn brief(task: &Task) -> Result<Value> {
    let mut status = status(task)?;
    let state = task.state()?;
    let size = |application: &execution::Application| json!({"revision":application.revision,"confirmed_revision":application.confirmed_revision,"approved_revision":application.approved_revision,"items":application.items.len()});
    status["application"] = size(&state.application);
    if let Some(proposal) = &state.application_proposal {
        status["proposal"] = size(proposal);
    }
    slim_growing_parts(&mut status, &state);
    status["read_more"] = json!(
        "application_workflow status: the baseline's outline; section (reviews, ready_work, runner, …) for the full part, and items for contracts"
    );
    Ok(status)
}

/// One part of the status. The baseline and the proposal are outlines, or
/// the full contracts of `items`.
fn section(task: &Task, section: &str, input: &Value) -> Result<Value> {
    let state = task.state()?;
    let application = match section {
        "application" => Some(&state.application),
        "proposal" => Some(
            state
                .application_proposal
                .as_ref()
                .context("no baseline proposal awaiting decision")?,
        ),
        _ => None,
    };
    if let Some(application) = application {
        return match input.get("items") {
            Some(ids) => execution::items(
                application,
                &serde_json::from_value::<Vec<String>>(ids.clone())?,
            ),
            None => Ok(execution::outline(application)),
        };
    }
    let status = status(task)?;
    let part = status.get(section).with_context(|| {
        format!(
            "status has no section {section}; its sections are {}",
            SECTIONS.join(", ")
        )
    })?;
    Ok(json!({ section: part }))
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
        "status" => match input["section"].as_str() {
            Some(name) => section(task, name, input),
            None => compact(task),
        },
        "proposal_example" if input["variant"] == "light" => execution::light_proposal_example(),
        "proposal_example" => execution::proposal_example(),
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
        "leader_inbox" => {
            // The messages handled last time go with the next read: one call.
            let acknowledged = match input.get("ack") {
                Some(ids) => acknowledge(task, serde_json::from_value(ids.clone())?)?,
                None => Vec::new(),
            };
            let mut inbox = leader_inbox(task)?;
            if !acknowledged.is_empty() {
                inbox["acknowledged"] = json!(acknowledged);
            }
            Ok(inbox)
        }
        "inbox_ack" => {
            let ids = acknowledge(task, serde_json::from_value(input["ids"].clone())?)?;
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
                json!({"configured":true,"adapter":if task.state()?.codex_team {"claude_mcp_only or opencode_mcp_only or codex_sandbox"} else {"claude_mcp_only or opencode_mcp_only"},"network":"none","live_project_mounts":false}),
            )
        }
        "image_build" => Ok(serde_json::to_value(crate::images::build(
            task,
            &serde_json::from_value(input.clone())?,
        )?)?),
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
            let affected = execution::store_input(task, input)?;
            Ok(
                json!({"affected":affected,"next":"Planner interview/confirmation, then explicit baseline approval"}),
            )
        }
        "amend" => {
            let changes =
                serde_json::from_value(input.get("changes").context("changes required")?.clone())?;
            let affected = execution::amend(
                task,
                changes,
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
        "review_request" => {
            let order = input["order"].as_str().context("order required")?;
            let review = reviews::request(
                task,
                order,
                input["reviewer"].as_str().context("reviewer required")?,
            )
            .map_err(|error| crate::failures::note_stale(task, None, Some(order), error))?;
            Ok(serde_json::to_value(review)?)
        }
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
/// The leader's messages, and the ready items nobody has started. Only
/// agents send messages; ready work is shown here, read when the leader reads
/// its inbox, and never wakes it on its own.
/// Takes the leader's handled messages out of its inbox.
fn acknowledge(task: &Task, ids: Vec<u64>) -> Result<Vec<u64>> {
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
    Ok(ids)
}

/// One line per live agent: what the leader needs between turns without a
/// status call. "hammer-17 hammer · lane front · claude · B3 · working".
fn roster(state: &crate::state::State) -> Vec<String> {
    state
        .agents
        .iter()
        .filter(|agent| agent.live)
        .map(|agent| {
            let order = state
                .open_order_of(&agent.id)
                .map(|order| order.step.as_str());
            let doing = if agent.lost {
                "lost"
            } else if state.worker_turns.contains_key(&agent.id) {
                "working"
            } else if !agent.ready {
                "starting"
            } else if order.is_some() {
                "has its order"
            } else {
                "idle"
            };
            let lane = agent
                .lane
                .as_deref()
                .map(|lane| format!(" · lane {lane}"))
                .unwrap_or_default();
            let step = order.map(|step| format!(" · {step}")).unwrap_or_default();
            format!(
                "{} {}{lane} · {}{step} · {doing}",
                agent.id, agent.role, agent.agent
            )
        })
        .collect()
}

pub(crate) fn leader_inbox(task: &Task) -> Result<Value> {
    let state = task.state()?;
    let team = roster(&state);
    let messages: Vec<_> = state
        .queue
        .into_iter()
        .filter(|message| message.to == crate::state::LEADER)
        .collect();
    let ready_not_started: Vec<Value> = ready_work(task)?
        .into_iter()
        .filter(|item| item["ready"] == true && item["assigned"] == false)
        .map(|item| json!({"id": item["id"], "lane": item["lane"], "rigor": item["rigor"]}))
        .collect();
    Ok(
        json!({"messages": messages, "team": team, "ready_not_started": ready_not_started, "next": "handle the messages, then read again with ack [their ids]"}),
    )
}

pub fn block_unverified_runner(task: &Task) -> Result<()> {
    crate::runner::available(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::tests::baseline;
    #[test]
    fn status_comes_in_outlines_and_sections_small_enough_to_read() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        crate::execution::store(&task, baseline(), "first").unwrap();
        let read = |input: Value| call(&task, "status", &input).unwrap();

        let whole = read(json!({}));
        assert_eq!(
            whole["proposal"]["items"]["S01-B01"]["title"],
            "Token service"
        );
        assert!(
            whole["proposal"]["items"]["S01-B01"]
                .get("acceptance")
                .is_none()
        );
        assert!(whole["read_more"].as_str().unwrap().contains("items"));
        let brief = brief(&task).unwrap();
        assert_eq!(
            brief["proposal"]["items"], 1,
            "only its size inside task_status"
        );
        assert_eq!(brief["proposal"]["revision"], 1);

        let runner = read(json!({"section":"runner"}));
        assert_eq!(
            runner.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["runner"]
        );
        let contracts = read(json!({"section":"proposal","items":["S01-B01"]}));
        assert_eq!(
            contracts["items"]["S01-B01"]["acceptance"][0],
            "Service contract passes"
        );
        let error = call(&task, "status", &json!({"section":"nothing"}))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("its sections are application, proposal, reviews, ready_work, runner"),
            "a wrong section names the right ones: {error}"
        );
        let whole = call(&task, "status", &json!({})).unwrap();
        for section in SECTIONS
            .into_iter()
            .filter(|section| *section != "proposal")
        {
            assert!(whole.get(section).is_some(), "the status has {section}");
        }
        assert!(
            call(
                &task,
                "status",
                &json!({"section":"proposal","items":["S9"]})
            )
            .is_err()
        );
    }

    #[test]
    fn the_leader_asks_for_the_light_example_by_variant() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let light = call(&task, "proposal_example", &json!({"variant": "light"})).unwrap();
        assert_eq!(
            light["application"]["items"]["readme-port"]["kind"],
            "change"
        );
        let full = call(&task, "proposal_example", &json!({})).unwrap();
        assert!(full["application"]["items"].get("status-command").is_some());
    }

    #[test]
    fn ready_work_shows_each_items_rigor_and_what_it_needs() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        task.transaction(|tx| {
            let mut application = baseline();
            let item = application.items.get_mut("S01-B01").unwrap();
            item.kind = crate::execution::WorkKind::Change;
            item.allowed = vec!["README.md".into()];
            tx.state.application = application;
            Ok(())
        })
        .unwrap();
        let item = ready_work(&task).unwrap().remove(0);
        assert_eq!(item["kind"], "change");
        assert_eq!(item["rigor"], "light");
        assert_eq!(
            item["required"],
            json!(["candidate_check", "accept_reason"])
        );

        task.transaction(|tx| {
            tx.state
                .application
                .items
                .get_mut("S01-B01")
                .unwrap()
                .allowed = vec!["api/auth/**".into()];
            Ok(())
        })
        .unwrap();
        let item = ready_work(&task).unwrap().remove(0);
        assert_eq!(item["rigor"], "strict");
        assert_eq!(item["rigor_why"], json!(["sensitive path api/auth/**"]));
    }

    #[test]
    fn the_inbox_shows_ready_work_beside_the_messages() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        task.transaction(|tx| {
            tx.state
                .say(crate::state::LEADER, "hammer-1 reports".into());
            Ok(())
        })
        .unwrap();
        let inbox = call(&task, "leader_inbox", &json!({})).unwrap();
        assert_eq!(inbox["messages"][0]["text"], "hammer-1 reports");
        assert!(inbox["ready_not_started"].is_array());
    }

    #[test]
    fn one_inbox_read_acknowledges_the_last_and_shows_the_team() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        task.transaction(|tx| {
            tx.state.say(crate::state::LEADER, "first".into());
            tx.state.agents.push(crate::state::Agent {
                id: "hammer-17".into(),
                role: "hammer".into(),
                agent: "claude".into(),
                repo: "app".into(),
                session: String::new(),
                pane: "%9".into(),
                writes: true,
                group: None,
                lane: Some("front".into()),
                live: true,
                unavailable: false,
                ready: true,
                lost: false,
                usage: None,
            });
            Ok(())
        })
        .unwrap();
        let inbox = call(&task, "leader_inbox", &json!({})).unwrap();
        assert_eq!(
            inbox["team"],
            json!(["hammer-17 hammer · lane front · claude · idle"])
        );
        let first = inbox["messages"][0]["id"].clone();
        task.transaction(|tx| {
            tx.state.say(crate::state::LEADER, "second".into());
            Ok(())
        })
        .unwrap();
        let next = call(&task, "leader_inbox", &json!({"ack": [first]})).unwrap();
        assert_eq!(next["acknowledged"], json!([first]));
        let texts: Vec<_> = next["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["text"].clone())
            .collect();
        assert_eq!(
            texts,
            [json!("second")],
            "the handled one is gone in the same call"
        );
        assert!(
            call(&task, "leader_inbox", &json!({"ack": [first]})).is_err(),
            "not twice"
        );
    }

    #[test]
    fn the_brief_keeps_review_outcomes_and_leaves_their_text_to_the_section() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        crate::execution::store(&task, baseline(), "first").unwrap();
        task.transaction(|tx| {
            tx.state.reviews.push(crate::reviews::Review {
                id: "r1".into(),
                order: "o1".into(),
                summary: Some("a long reviewed account ".repeat(100)),
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
        let brief = brief(&task).unwrap();
        assert_eq!(brief["reviews"][0]["id"], "r1");
        assert!(brief["reviews"][0].get("summary").is_none());
        assert_eq!(brief["accepted_items"], 0);
        let reviews = call(&task, "status", &json!({"section":"reviews"})).unwrap();
        assert!(
            reviews["reviews"][0]["summary"]
                .as_str()
                .unwrap()
                .starts_with("a long")
        );
    }

    #[test]
    fn the_leader_amends_the_baseline_with_only_the_changes() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        crate::execution::store(&task, baseline(), "first").unwrap();
        crate::execution::confirm(&task, 1, false).unwrap();
        crate::execution::confirm(&task, 1, true).unwrap();
        let mut item = baseline().items["S01-B01"].clone();
        item.title = "Token refresh".into();
        let result = call(
            &task,
            "amend",
            &json!({"reason":"refresh tokens","changes":{
                "items":{"S01-B02":item},
                "milestones":{"M01":{"version":"1.1.0","title":"Accounts ready","required":["S01-B01","S01-B02"]}}
            }}),
        )
        .unwrap();
        assert_eq!(result["affected"], json!(["S01-B02"]));
        let proposal = task.state().unwrap().application_proposal.unwrap();
        assert_eq!(proposal.revision, 2);
        assert_eq!(proposal.items.len(), 2);
        assert!(
            call(&task, "amend", &json!({"changes":{}})).is_err(),
            "a reason is required"
        );
    }

    #[test]
    fn status_says_codex_may_plan_and_write_when_the_team_is_on_codex() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let runner = |task: &Task| status(task).unwrap()["runner"].clone();
        assert_eq!(
            runner(&task)["adapter"],
            "claude_mcp_only or opencode_mcp_only"
        );
        assert_eq!(
            runner(&task)["worker_programs"],
            serde_json::json!(["claude", "opencode"])
        );
        task.transaction(|tx| {
            tx.state.codex_team = true;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            runner(&task)["worker_programs"],
            serde_json::json!(["claude", "opencode", "codex"]),
            "the user put the whole team on Codex: planners, writers and reviewers"
        );
        assert!(
            runner(&task)["adapter"].as_str().unwrap().contains("codex"),
            "{}",
            runner(&task)
        );
    }

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
    fn the_leader_approves_a_check_only_revision_but_not_a_plan_change() {
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        crate::execution::store(&task, baseline(), "first").unwrap();
        crate::execution::confirm(&task, 1, false).unwrap();
        crate::execution::confirm(&task, 1, true).unwrap();

        let mut checks_only = task.state().unwrap().application;
        checks_only.revision = 2;
        let app = checks_only.check_configuration.get_mut("app").unwrap();
        let lint = app.checks["backend"].clone();
        app.checks.insert("pint".into(), lint);
        crate::execution::store(&task, checks_only, "back lane checks").unwrap();
        let status = call(&task, "status", &json!({})).unwrap();
        assert_eq!(
            status["proposal_approval"]["needs_user"], false,
            "{}",
            status["proposal_approval"]
        );
        crate::execution::confirm(&task, 2, true).expect("no confirmation round for checks");
        assert!(
            task.state().unwrap().application.approved(),
            "a revision the leader approved lets orders go out"
        );
        assert!(
            task.log_lines()
                .unwrap()
                .iter()
                .any(|line| line.contains("the leader approved revision 2"))
        );

        let mut plan_change = task.state().unwrap().application;
        plan_change.revision = 3;
        plan_change.goal = "Something else".into();
        crate::execution::store(&task, plan_change, "new goal").unwrap();
        let status = call(&task, "status", &json!({})).unwrap();
        assert_eq!(status["proposal_approval"]["needs_user"], true);
        assert!(
            status["proposal_approval"]["why"]
                .as_str()
                .unwrap()
                .contains("changes the plan")
        );
        assert!(
            crate::execution::confirm(&task, 3, true).is_err(),
            "the user confirms and approves a plan change"
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
    // A revision resets only what it changed: the work that depends on it
    // keeps its status and the leader is asked to check it; a removed item
    // closes.
    #[test]
    fn a_revision_resets_only_the_items_it_changed() {
        use crate::state::StepStatus;
        let directory = tempfile::TempDir::new().unwrap();
        let (_home, _devws, task, _repo) = crate::board::tests::started(&directory);
        let mut proposal = baseline();
        for (id, dependencies) in [("S01-B02", vec!["S01-B01"]), ("S01-B03", vec![])] {
            let mut item = proposal.items["S01-B01"].clone();
            item.dependencies = dependencies.into_iter().map(str::to_string).collect();
            proposal.items.insert(id.into(), item);
            for milestone in proposal.milestones.values_mut() {
                milestone.required.push(id.into());
            }
        }
        crate::execution::store(&task, proposal, "three items").unwrap();
        crate::execution::confirm(&task, 1, false).unwrap();
        crate::execution::confirm(&task, 1, true).unwrap();
        task.transaction(|tx| {
            for (id, status) in [
                ("S01-B01", StepStatus::Reported),
                ("S01-B02", StepStatus::Accepted),
                ("S01-B03", StepStatus::Failed),
            ] {
                tx.state.steps.push(crate::state::Step {
                    id: id.into(),
                    title: id.into(),
                    status,
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
        changed.items.remove("S01-B03");
        for milestone in changed.milestones.values_mut() {
            milestone.required.retain(|item| item != "S01-B03");
        }
        crate::execution::store(&task, changed, "one change, one removal").unwrap();
        crate::execution::confirm(&task, 2, false).unwrap();
        crate::execution::confirm(&task, 2, true).unwrap();

        let state = task.state().unwrap();
        let status = |id: &str| {
            state
                .steps
                .iter()
                .find(|step| step.id == id)
                .unwrap()
                .status
        };
        assert_eq!(status("S01-B01"), StepStatus::Rejected);
        assert_eq!(
            status("S01-B02"),
            StepStatus::Accepted,
            "its dependent keeps its status"
        );
        assert_eq!(
            status("S01-B03"),
            StepStatus::Closed,
            "a removed item closes"
        );
        assert_eq!(
            state.step_failures["S01-B01"][0].kind,
            crate::failures::FailureKind::RevisionReset
        );
        assert_eq!(crate::failures::work_failures(&state, "S01-B01"), 0);
        let told = state
            .queue
            .iter()
            .find(|message| {
                message.to == crate::state::LEADER && message.text.contains("Revision 2")
            })
            .expect("the leader hears what the revision reset");
        assert!(
            told.text.contains("S01-B01") && told.text.contains("S01-B02"),
            "{}",
            told.text
        );
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
        let roles = crate::roles::Roles::load(&home, None).unwrap();
        let infra = roles.get("infra").expect("an infra senior is provisioned");
        assert!(!infra.writes, "infra changes no repo file");
        assert!(infra.instructions.contains("image_build"));
        std::fs::write(&path, "customized").unwrap();
        crate::roles::provision_specialists(&home).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "customized");
    }
}
