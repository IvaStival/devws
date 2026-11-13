//! What the leader reads to know where the team stands: `task_status`, and
//! `devagentics task status`. The truth about the team lives here, not in the
//! leader's memory.

use crate::devws::{Devws, PaneState};
use crate::home::Home;
use crate::roles::Roles;
use crate::state::{LEADER, OrderStatus};
use crate::task::Task;
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// How many log entries the summary ends with.
const RECENT: usize = 20;

pub fn summary(task: &Task, devws: &dyn Devws, home: &Home) -> Result<Value> {
    let config = task.config()?;
    let state = task.state()?;
    let first_worktree = config.repos.first().map(|repo| repo.worktree.clone());
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
                "sandbox": if crate::agent_args::Kind::parse(&effective.agent) == Some(crate::agent_args::Kind::Codex) {
                    effective.sandbox.as_deref().unwrap_or(if role.writes { "workspace-write" } else { "read-only" })
                } else {
                    "provider-managed"
                },
                "approval_mode": effective.approvals.as_deref().unwrap_or(match crate::agent_args::Kind::parse(&effective.agent) {
                    Some(crate::agent_args::Kind::Claude) if role.writes => "acceptEdits",
                    Some(crate::agent_args::Kind::Claude) => "default",
                    Some(crate::agent_args::Kind::Codex) => "on-request",
                    None => "provider-default",
                }),
                "description": role.description,
                "startup_candidates": if role.name == "leader" { None } else {
                    devws.agent_kind(&role.agent).ok().map(|kind| role.worker_candidates(kind))
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
        "recovery": state.recovery,
        "amendments": state.amendments,
        "permissions": state.permissions,
        "task": {
            "id": config.id, "branch": config.branch, "workspace": config.workspace,
            "approved": state.approved, "paused": state.paused, "max_agents": config.max_agents,
        },
        "plan": { "path": plan_path, "written": plan_path.is_file() },
        "repos": config.repos.iter().map(|repo| json!({ "name": repo.name, "worktree": repo.worktree, "check": repo.check })).collect::<Vec<_>>(),
        "lanes": config.lanes.iter().map(|lane| json!({ "name": lane.name, "repo": lane.repo, "files": lane.globs, "branch": lane.branch, "worktree": lane.worktree })).collect::<Vec<_>>(),
        "roles": roles,
        "agents": agents,
        "steps": state.steps.iter().map(|step| json!({ "id": step.id, "version": versions.get(&step.id), "title": step.title, "status": step.status, "attempts": step.attempts })).collect::<Vec<_>>(),
        // What a new plan part is named; its steps are next_version.1, .2…
        "next_version": crate::specs::next_version(&state.steps),
        "execution_plans": state.plans.iter().map(|plan| json!({ "step": plan.step, "author": plan.author })).collect::<Vec<_>>(),
        "open_questions": state.questions.iter().filter(|question| question.answer.is_none()).map(|question| json!({ "id": question.id, "from": question.from, "to": question.to, "text": question.text })).collect::<Vec<_>>(),
        "waiting_for_you": state.queue.iter().filter(|message| message.to == LEADER).count()
            + state.permissions.iter().filter(|request| request.status == crate::state::PermissionStatus::Pending).count(),
        "pending_permissions": state.permissions.iter().filter(|request| request.status == crate::state::PermissionStatus::Pending).count(),
        "limits": limits,
        "leader": crate::leader::status(home, task, devws)?,
        "recent_log": recent,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_args::Kind as Program;
    use crate::devws::Limits;
    use crate::devws::fake::FakeDevws;
    use crate::home::Home;
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
        assert_eq!(summary["repos"][0]["check"], Value::Null);
        assert_eq!(
            summary["leader"],
            json!({
                "program": "claude", "other": "codex (gpt-6.1-sol)",
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
