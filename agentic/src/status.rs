//! What the leader reads to know where the team stands: `task_status`, and
//! `devagentics task status`. The truth about the team lives here, not in the
//! leader's memory.

use crate::devws::{Devws, PaneState};
use crate::home::Home;
use crate::roles::Roles;
use crate::state::{LEADER, OrderStatus};
use crate::task::Task;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// How many log entries the summary ends with.
const RECENT: usize = 20;
const PROGRESS_LIMIT: usize = 20;
const ORDER_PHASE_LIMIT: usize = 8;
const DIAGNOSTIC_LIMIT: usize = 180;

/// A state-only snapshot for routine progress checks between full reads.
pub fn progress(task: &Task) -> Result<Value> {
    let state = task.state()?;
    let mut active: Vec<_> = state
        .steps
        .iter()
        .filter(|step| {
            !matches!(
                step.status,
                crate::state::StepStatus::Accepted | crate::state::StepStatus::Closed
            )
        })
        .collect();
    active.sort_by_key(|step| (!step.status.waits_for_leader(), step.review_since));
    let agents: Vec<_> = state.agents.iter().filter(|agent| agent.live).collect();
    let config = task.config()?;
    Ok(json!({
        "task": config.id,
        "paused": state.paused,
        "approved": state.approved,
        "workflow": state.workflow,
        "application_revision": state.application.revision,
        "proposal_revision": state.application_proposal.as_ref().map(|proposal| proposal.revision),
        "pending": {
            "inbox": state.queue.iter().filter(|message| message.to == LEADER).count(),
            "permissions": state.permissions.iter().filter(|request| request.status == crate::state::PermissionStatus::Pending).count(),
            "questions": state.questions.iter().filter(|question| question.answer.is_none()).count(),
            "recovery": state.recovery.len(),
            "usage_limit_approval": state.usage_limit_approval,
        },
        "step_count": state.steps.len(),
        "active_step_count": active.len(),
        "active_steps": active.iter().take(PROGRESS_LIMIT).map(|step| json!({
            "id": step.id,
            "status": step.status,
            "attempts": step.attempts,
        })).collect::<Vec<_>>(),
        "live_agent_count": agents.len(),
        "agents": agents.iter().take(PROGRESS_LIMIT).map(|agent| json!({
            "id": agent.id,
            "role": agent.role,
            "lane": agent.lane,
            "lost": agent.lost,
            "unavailable": agent.unavailable,
            "working": state.worker_turns.contains_key(&agent.id),
            "order": state.open_order_of(&agent.id).map(|order| json!({
                "id": order.id,
                "step": order.step,
                "delivered": order.delivered,
            })),
        })).collect::<Vec<_>>(),
    }))
}

/// A single assignment and its recorded check evidence, including closed orders.
pub fn order(task: &Task, id: &str) -> Result<Value> {
    let state = task.state()?;
    let order = state
        .orders
        .iter()
        .find(|order| order.id == id)
        .with_context(|| {
            format!("unknown order {id}; use task_status view=progress for open order IDs")
        })?;
    let revision = state.revision(id);
    let evidence = state.check_evidence.get(id).map(|evidence| {
        let phases = evidence.phases.iter().take(ORDER_PHASE_LIMIT).map(|phase| json!({
            "profile": phase.profile,
            "outcome": phase.outcome,
            "exit_code": phase.exit_code,
            "diagnostic": phase.diagnostic.chars().take(DIAGNOSTIC_LIMIT).collect::<String>(),
            "diagnostic_truncated": phase.diagnostic.chars().nth(DIAGNOSTIC_LIMIT).is_some(),
        })).collect::<Vec<_>>();
        json!({
            "revision": evidence.revision,
            "matches_order_revision": evidence.revision == revision,
            "head": evidence.head,
            "passed": evidence.passed(),
            "phase_count": evidence.phases.len(),
            "phases": phases,
        })
    });
    Ok(json!({
        "id": order.id,
        "step": order.step,
        "agent": order.agent,
        "status": order.status,
        "delivered": order.delivered,
        "base": order.base,
        "plan": order.plan,
        "revision": revision,
        "profiles": state.order_profiles.get(id),
        "check_evidence": evidence,
    }))
}

pub fn summary(task: &Task, devws: &dyn Devws, home: &Home) -> Result<Value> {
    let config = task.config()?;
    let state = task.state()?;
    let first_worktree = config.repos.first().map(|repo| repo.worktree.clone());
    let opencode =
        crate::accounts::opencode_for_task(crate::opencode::OpencodeSettings::load(home)?, &state);
    let roles: Vec<Value> = Roles::load(home, first_worktree.as_deref())?
        .all()
        .iter()
        .map(|role| {
            let fallback = role.on_fallback();
            let effective = if role.name == "leader"
                && fallback.as_ref().is_some_and(|fallback| {
                    state.leader_agent.as_deref() == Some(fallback.agent.as_str())
                })
            {
                fallback.as_ref().unwrap_or(role)
            } else {
                role
            };
            json!({
                "name": role.name, "agent": effective.agent, "model": effective.model,
                "effort": effective.effort, "writes": role.writes, "group": role.group.name(),
                "repo_access": if role.writes { "order_scoped_write" } else { "read_only" },
                "permission_ceiling": if role.group == crate::roles::Group::Floor { "order_scoped_write" } else { "read_only" },
                "sandbox": crate::providers::sandbox_label(effective),
                "approval_mode": if state.workflow==crate::execution::Workflow::Reviewed {"scoped_mcp_only".to_string()} else {crate::providers::approvals_label(effective)},
                "description": role.description,
                "startup_candidates": if role.name == "leader" { None } else {
                    devws.agent_kind(&role.agent).ok().map(|kind| {
                        crate::worker::LaunchState::new(role, kind, &opencode).candidates
                    })
                },
            })
        })
        .collect();
    let mut agents = Vec::new();
    for agent in &state.agents {
        let pane_state = if agent.unavailable {
            "unavailable"
        } else if agent.live && !agent.lost {
            match devws.pane_state(&agent.pane)? {
                PaneState::Working => "working",
                PaneState::Waiting => "waiting",
                PaneState::Done => "done",
                PaneState::Idle => "idle",
                PaneState::Gone => "gone",
            }
        } else {
            "gone"
        };
        let order = state
            .orders
            .iter()
            .find(|order| order.agent == agent.id && order.status == OrderStatus::Open)
            .map(|order| json!({ "id": order.id, "step": order.step, "delivered": order.delivered, "base": order.base, "plan": order.plan, "revision": state.revision(&order.id), "profiles": state.order_profiles.get(&order.id), "check_evidence": state.check_evidence.get(&order.id).map(crate::check_profiles::Evidence::response) }));
        agents.push(json!({
            "agent": agent.agent,
            "selected_program": state.worker_launches.get(&agent.id).and_then(|launch| launch.selected.as_ref()),
            "fallback_reasons": state.worker_launches.get(&agent.id).map(|launch| &launch.failures),
            "unavailable": agent.unavailable,
            "id": agent.id, "role": agent.role, "repo": agent.repo, "pane": agent.pane,
            "live": agent.live, "state": pane_state, "ready": agent.ready, "lost": agent.lost,
            "writes": agent.writes, "group": agent.group().name(), "lane": agent.lane, "order": order,
        }));
    }

    let mut limits = serde_json::Map::new();
    let accounts: BTreeSet<_> = state
        .agents
        .iter()
        .filter(|agent| agent.live)
        .map(|agent| agent.agent.as_str())
        .collect();
    for account in accounts {
        let used = devws.limits(account)?;
        limits.insert(
            account.to_string(),
            json!({ "five_hour_percent": used.five_hour, "week_percent": used.week }),
        );
    }

    let log = task.log_lines()?;
    let recent: Vec<Value> = log[log.len().saturating_sub(RECENT)..]
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let plan_path = task.dir().join("plan.md");
    let versions = crate::specs::step_versions(&state.steps);

    Ok(json!({
        "reviewed_workflow": crate::workflow::brief(task)?,
        "recovery": state.recovery,
        "amendments": state.amendments,
        "permissions": state.permissions,
        "usage_limit_approval": {
            "status": state.usage_limit_approval,
            "blocked_accounts": state.usage_limit_blocks,
            "next_action": match state.usage_limit_approval {
                crate::state::UsageApproval::Pending => "Ask the user whether workers may exceed percentage limits for this task; record the explicit decision with set_usage_limit_approval.",
                crate::state::UsageApproval::Approved => "Resume existing blocked workers with resume_task; do not create duplicate replacements.",
                crate::state::UsageApproval::Denied => "No override. Do not ask again unless the user requests reconsideration.",
                crate::state::UsageApproval::NotRequested => "Use normal worker selection.",
            },
        },
        "task": {
            "id": config.id, "branch": config.branch, "workspace": config.workspace,
            "approved": state.approved, "paused": state.paused, "max_agents": config.max_agents,
        },
        "plan": { "path": plan_path, "written": plan_path.is_file() },
        "repos": config.repos.iter().map(|repo| json!({ "name": repo.name, "worktree": repo.worktree, "check": repo.check })).collect::<Vec<_>>(),
        "lanes": config.lanes.iter().map(|lane| json!({ "name": lane.name, "repo": lane.repo, "files": lane.globs, "branch": lane.branch, "worktree": lane.worktree })).collect::<Vec<_>>(),
        "roles": roles,
        "agents": agents,
        "steps": state.steps.iter().map(|step| json!({ "id": step.id, "version": if state.workflow==crate::execution::Workflow::Reviewed {None}else{versions.get(&step.id)}, "title": step.title, "status": step.status, "orders": step.attempts, "attempt_limit": crate::orders::attempt_limit(&state, &step.id, config.max_attempts), "failures": (!matches!(step.status, crate::state::StepStatus::Accepted | crate::state::StepStatus::Closed)).then(|| crate::failures::status_json(&state, &step.id)) })).collect::<Vec<_>>(),
        // What a new plan part is named; its steps are next_version.1, .2…
        "next_version": if state.workflow==crate::execution::Workflow::Reviewed {None}else{Some(crate::specs::next_version(&state.steps))},
        "execution_plans": state.plans.iter().map(|plan| json!({ "step": plan.step, "author": plan.author })).collect::<Vec<_>>(),
        "open_questions": state.questions.iter().filter(|question| question.answer.is_none()).map(|question| json!({ "id": question.id, "from": question.from, "to": question.to, "text": question.text })).collect::<Vec<_>>(),
        "waiting_for_you": usize::from(state.usage_limit_approval == crate::state::UsageApproval::Pending) + state.queue.iter().filter(|message| message.to == LEADER).count()
            + state.permissions.iter().filter(|request| request.status == crate::state::PermissionStatus::Pending).count(),
        "pending_permissions": state.permissions.iter().filter(|request| request.status == crate::state::PermissionStatus::Pending).count(),
        "limits": limits,
        // The user's switches: new agents start only on accounts that are on.
        "accounts": devws.accounts()?.into_iter().map(|(name, kind)| json!({
            "name": name,
            "kind": kind.name(),
            "enabled": crate::accounts::is_enabled(&state, &name),
            // Each group's own switch, held off when `enabled` is off: the
            // leader leads only on an account with `leader` on.
            "leader": crate::accounts::is_enabled_for(&state, &name, crate::roles::Group::Cto),
            "senior": crate::accounts::is_enabled_for(&state, &name, crate::roles::Group::Senior),
            "floor": crate::accounts::is_enabled_for(&state, &name, crate::roles::Group::Floor),
            // The model the user picked for the group's agents on it; null: each role's own.
            "leader_model": crate::accounts::group_model(&state, &name, crate::roles::Group::Cto),
            "senior_model": crate::accounts::group_model(&state, &name, crate::roles::Group::Senior),
            "floor_model": crate::accounts::group_model(&state, &name, crate::roles::Group::Floor),
            // A provider refused a worker on it: no new agents until then.
            "held": state.account_holds.get(&name).filter(|hold| hold.until > crate::log::now()).map(|hold| json!({"until": hold.until, "reason": hold.reason})),
        })).collect::<Vec<_>>(),
        "leader": crate::leader::status(home, task, devws)?,
        "recent_log": recent,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devws::Limits;
    use crate::devws::fake::FakeDevws;
    use crate::home::Home;
    use crate::providers::Kind as Program;
    use crate::start::start_task;
    use crate::team::Team;
    use tempfile::TempDir;

    #[test]
    fn status_reports_the_actual_fallback_instead_of_the_preferred_role() {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        let devws = FakeDevws::new(&dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_kind("codex", Program::Codex);
        start_task(&home, &devws, repo.path(), "d", None).unwrap();
        let task = Task::open(&home, "main").unwrap();
        devws.set_limits(
            "claude",
            Limits {
                week: Some(100),
                five_hour: None,
            },
        );
        Team {
            task: &task,
            home: &home,
            devws: &devws,
        }
        .spawn_agent("planner", None)
        .unwrap();
        let status = summary(&task, &devws, &home).unwrap();
        assert_eq!(status["agents"][0]["agent"], "codex");
        assert_eq!(
            status["agents"][0]["selected_program"]["model"],
            "gpt-6.1-sol"
        );
        assert!(
            status["agents"][0]["fallback_reasons"][0]
                .as_str()
                .unwrap()
                .contains("week")
        );
    }

    #[test]
    fn the_summary_says_where_the_team_stands() {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().join("home"));
        let repo = crate::checks::tests::repo();
        crate::checks::tests::run(repo.path(), &["checkout", "-q", "-b", "feat/x"]);
        let devws = FakeDevws::new(&dir.path().join("w"))
            .with_kind("claude", Program::Claude)
            .with_limits(
                "claude",
                Limits {
                    five_hour: Some(40),
                    week: Some(7),
                },
            );
        start_task(&home, &devws, repo.path(), "d", None).unwrap();
        let task = Task::open(&home, "feat-x").unwrap();
        let team = Team {
            task: &task,
            home: &home,
            devws: &devws,
        };
        let planner = team.spawn_agent("planner", None).unwrap();
        devws.set_state(&planner.pane, PaneState::Working);
        task.transaction(|tx| {
            tx.state.say(LEADER, "executor-1 reports".into());
            Ok(())
        })
        .unwrap();

        let summary = summary(&task, &devws, &home).unwrap();
        assert_eq!(summary["task"]["id"], "feat-x");
        assert_eq!(summary["task"]["approved"], false);
        assert_eq!(summary["plan"]["written"], false);
        assert_eq!(summary["roles"][0]["name"], "executor");
        assert_eq!(summary["roles"][0]["writes"], true);
        assert_eq!(summary["roles"][0]["repo_access"], "order_scoped_write");
        assert_eq!(
            summary["roles"][0]["permission_ceiling"],
            "order_scoped_write"
        );
        assert_eq!(summary["roles"][0]["approval_mode"], "auto");
        assert_eq!(summary["agents"][0]["id"], "planner-1");
        assert_eq!(summary["agents"][0]["state"], "working");
        assert_eq!(summary["agents"][0]["order"], Value::Null);
        assert_eq!(summary["waiting_for_you"], 1);
        assert_eq!(summary["pending_permissions"], 0);
        assert_eq!(summary["limits"]["claude"]["five_hour_percent"], 40);
        assert!(
            summary["accounts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|account| account["enabled"] == true
                    && account["leader"] == true
                    && account["senior"] == true
                    && account["floor"] == true
                    && account["name"].is_string()),
            "{}",
            summary["accounts"]
        );
        assert_eq!(summary["repos"][0]["check"], Value::Null);
        assert_eq!(
            summary["leader"],
            json!({
                "program": "claude (claude-opus-5-5)", "other": "codex (gpt-6.1-sol)",
                "other_limits": { "five_hour_percent": null, "week_percent": null },
                "other_run_out": false, "switch_pending": null,
            })
        );
        let kinds: Vec<_> = summary["recent_log"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["kind"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(kinds, ["start", "spawn"]);

        // A stopped agent still shows, as gone.
        team.stop_agent("planner-1").unwrap();
        let after = super::summary(&task, &devws, &home).unwrap();
        assert_eq!(
            (
                after["agents"][0]["live"].clone(),
                after["agents"][0]["state"].clone()
            ),
            (json!(false), json!("gone"))
        );
        assert!(
            after["limits"].as_object().unwrap().is_empty(),
            "only live agents' accounts are asked"
        );
    }
}
