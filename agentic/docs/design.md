# devAgentics design

## Context
devws runs one agent per workspace: one folder, one branch. The goal is
parallel teams: a planner writes the plan, a leader (the tech lead) starts
custom agents with different jobs, possibly across several repos, and steers
them. The user wants this as a new project, devAgentics, so devws stays as
it is. Settled in a grilling session (03-11-2026).

## Design

**Repo.** `agentic/` in devws (it began as `~/Projects/devAgentics`), written in Rust; it was on GitHub as the private
repo `IvaStival/devAgentics`. It uses devws as its screen and workspace layer
through a few stable `devws` commands and doesn't copy devws.

**Flow.**
1. The planner (strongest model) writes `plan.md`. It is a step, not a
   standing agent: the leader can call it back to re-plan.
2. The user approves the plan (gate 1).
3. The leader (a Claude Code or Codex session) starts and stops every team
   member. Executors and helpers (tester, reviewer, any role) act only on
   its orders.
4. When every step is done, the leader opens one draft PR per repo, each
   linking the others. The user reviews them (gate 2). In between, the leader
   escalates ("needs you") only for what it can't decide.

**Tasks.** One branch name across every repo the task touches, with a
worktree per repo. At most one agent writes in a worktree at a time; a lock
enforces it. The sidebar groups a task's workspaces under the task's name.
Task state lives in `~/.devagentics/tasks/<id>/`: the plan, the log, order
queues, locks and which agent is in which pane.

**Roles.** Files that set the CLI (claude, codex, opencode), model, effort,
prompt, sandbox and approvals, and whether the role writes or only reads.
devAgentics ships defaults; a project's `.devws/roles/` adds its own or
overrides them.

**Leader tools.** An MCP server in devAgentics: spawn, send an order, status,
`task_status`, read the log, open PRs.

**Orders.** One at a time per agent, with the files it may change and the
files it may not. They wait in a queue per agent and are delivered when its
devws state is idle or done, or through Claude's Stop hook. Every order ends
with done or blocked plus a report. Agents talk only to the leader.

**Acceptance.** devAgentics checks the diff against the order's file rules,
runs the step's checks itself, and then the leader reviews the diff. After
two failures the step goes to a stronger model (v2).

**Log.** devAgentics writes `log.jsonl`: orders, reports, check results,
escalations. The user watches it in a pane.

**Memory.** The task folder holds the exact state: step status, queues,
locks. ai-memory holds the team's knowledge: decisions, gotchas and handoffs,
which recovering leaders and agents query. After a compaction or a restart,
the leader rebuilds from `task_status` plus its last ai-memory handoff.
ai-memory never answers exact questions (step status, locks, queues).
Decisions about one repo go into that repo's project; decisions about the
whole task go into a dedicated ai-memory project, `devagentics-tasks`.

**Limits.** Per task: at most N agents at once (3 by default), an OpenRouter
spending cap, and no new agents on an account at 85% of its week or five-hour
usage. Workers try compatible alternatives when that account or their model is
unavailable; project-local agent management needs no separate approval. See
[worker startup fallback](../../docs/architecture/devagentics-execution.md#worker-availability-and-startup-fallback).
Reaching a limit pauses the task and kills nothing.

**Screen.** The task's window holds the leader, the log pane and the plan in
the editor. Each executor is an agent pane in its worktree's devws workspace.

## Phases
- **v1:** tasks across repos, roles, the MCP tools, queues and the log,
  Claude and Codex executors, file rules, checks plus the leader's review,
  draft PRs, limits, ai-memory handoffs.
- **v2:** OpenCode as a devws agent kind (its plugin events need checking
  first), executors on OpenRouter models (DeepSeek…), the stronger-model
  rule, the task's branches run together in jungleforge with end-to-end
  checks.
- **v3:** stock test and review helper roles.

## devws side (separate plan in devws)
Stable `devws` commands for devAgentics: open a worktree workspace, add an
agent pane with a role, read an agent's state, deliver text when it is idle.
The sidebar groups workspaces by task. In v2, OpenCode as an agent kind.

## Verification (v1, end to end)
A two-repo task (a test API repo and a test web repo): the planner writes a
plan, the user approves it, the leader starts one Claude and one Codex
executor in separate worktrees, a step that breaks its file rules is
rejected, the checks gate each step, and the result is two linked draft PRs.
Killing the leader mid-task and starting a new one resumes the task from
`task_status`.

## Reliable execution state

The [execution architecture](../../docs/architecture/devagentics-execution.md) describes order revisions, operation locks, isolated check evidence and durable interruption recovery.
