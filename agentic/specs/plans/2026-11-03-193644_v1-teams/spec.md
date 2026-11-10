# Spec: devAgentics v1, agent teams across repos

- **Date:** 03-11-2026
- **Branch:** feat/v1-teams
- **Status:** Specified

## Context

The design (`docs/design.md`) was settled in a grilling session. v1 is a
working team on the user's Claude and Codex subscriptions:
- a planner writes the plan;
- the user approves it;
- a leader starts executors in each repo's worktree and hands each one order
  at a time;
- devAgentics checks every step, then the leader reviews it;
- the result is linked draft PRs.

devws is the screen and workspace layer, driven only through its commands
(`devws open`, `devws agent spawn|run|state|send|mark|stop|limits`, `devws
worktree ensure`; devws's specs `2026-11-03-193644_devagentics-interface` and
`2026-11-03-211923_devws-agentic`). The user starts a team with `devws
agentic`, which makes the worktree, calls `devagentics task start` and opens
the workspace: the leader, the user talks to, and a log pane running
`devagentics watch`. The leader opens the other agents as it needs them.

## Requirements

### Tasks
1. `devagentics task start <folder> [--max-agents N]` is what `devws agentic`
   calls. `<folder>` is the task's first worktree, which devws made. It:
   - takes the branch from the folder (`git symbolic-ref`); the task id is the
     branch as a slug (`feat/login` → `feat-login`). A task already started
     for that folder is reused; another folder with the same slug gets `-2`;
   - creates `~/.devagentics/tasks/<id>/` (`DEVAGENTICS_HOME` overrides it)
     holding `task.toml` (repos, worktrees, checks, limits, and the workspace's
     session, `dev-<folder name>`), `state.json` (agents, orders, steps,
     locks), `log.jsonl` and `plan.md`;
   - writes the leader's arguments file (`leader.args`: its role as the
     agent's program takes it, the MCP server of `devagentics mcp <id>`, its
     first message) and prints three lines: the task id, the leader's agent
     and that file's path.
   It opens nothing: devws opens the workspace, with the leader in it and
   `devagentics watch <id>` in its log pane.
2. `devagentics task approve <id>` is the user's gate 1, a command only the
   user runs (not a leader tool). It refuses when `plan.md` is empty. The plan
   comes from the leader asking a planner (a role) through `spawn_agent`; the
   planner stores it with `devagentics plan write` (the plan on stdin). Until
   the task is approved, `spawn_agent` refuses roles that write.
3. `devagentics task status <id>` prints the same summary as the `task_status`
   tool. `task pause|resume|cancel <id>`:
   - **pause** stops new deliveries and spawns, but kills nothing;
   - **resume** respawns the team agents whose panels are gone (a restart) as
     new conversations, with their open orders sent again, and restarts the
     dispatcher if it stopped. The leader comes back with `devws agentic
     <folder>`, which reuses the task (`task start`);
   - **cancel** stops every team agent and keeps the worktrees.
4. `devagentics task list` lists the tasks with their state.

### Roles
5. A role is a Markdown file with a TOML front matter block:
   - `name`, `agent` (a `DEVWS_AGENTS` name such as `claude` or `codex_work`),
     `model`, `effort`, `writes` (true/false), `sandbox` and `approvals`;
   - the body is the role's instructions, sent as the agent's first message
     along with its order context.
6. The defaults (`planner`, `leader`, `executor`) ship in the binary. Files in
   `<repo>/.devws/roles/` and `~/.devagentics/roles/` add roles or override
   them by name, in that order of precedence. `devagentics role list` shows
   where each one comes from.
7. Each role is turned into CLI arguments by a small adapter per kind (claude,
   codex) and passed through devws's `--agent-args` file. The leader also gets
   the MCP server config. What each kind supports (model, effort, sandbox,
   MCP, initial prompt) is checked against the real CLIs when this is planned.

### Leader tools (MCP, stdio, `devagentics mcp <id>`)
8. `task_status`: the plan's path, repos and checks, agents (role, worktree,
   state, current order), steps and their states, waiting messages, limits,
   and the last 20 log entries.
9. `add_repo(repo, check?)`: another repo for the task, as a worktree on the
   task's branch (`devws worktree ensure`), with its check command.
   `spawn_agent(role, repo?)` starts an agent in a repo's worktree (the task's
   first repo by default):
   - in the first repo it is a panel of the leader's workspace
     (`devws agent spawn … --place team --label <role> --agent-args <file>`);
   - in another repo, that repo's own workspace opens with the agent as its
     main agent (`devws open --no-attach --task <id> …`).
   Its pane is marked `@devws_team 1` and `@devagentics_agent <id>`. It
   refuses:
   - a writing role in a worktree that already has a writer (the lock);
   - a writing role before the task is approved;
   - more than `max_agents` live team members (default 3; the leader and
     planner do not count);
   - an account past 85% of its 5-hour limit (`devws agent limits`).
10. `send_order(agent, step, text, allowed, forbidden?, check?)`: queues one
    order (step id and title, the instructions, the globs it may change and
    must not change, and a check command that defaults to the repo's). It
    records the base commit. An agent with an order still open refuses a
    second one.
11. `accept_step(step)` / `reject_step(step, reason)`: the leader's review.
    Rejecting re-opens the step, and the leader re-orders.
12. `stop_agent(agent)`, which also releases its lock; `set_check(repo,
    command)`; `read_log(limit, kinds?)`; `escalate(question)`, which logs the
    question and marks the leader pane `waiting` with a notification
    (`devws agent mark`).
13. `open_prs()`: when every step is accepted, pushes each worktree's branch,
    opens a draft PR per repo with `gh pr create --draft`, then edits each
    PR's body to link the others.

### Orders, reports and acceptance
14. An executor reports with `devagentics report done|blocked "<summary>"`
    (identified by `$TMUX_PANE`). Every role's instructions say so.
15. On `done`, devAgentics:
    - requires a clean worktree with the work committed;
    - checks every file changed since the base commit against `allowed` and
      `forbidden`;
    - runs the check command in the worktree, with a timeout and its output
      saved under the task folder.
    The result goes to the leader as a message ("step 3: rules ok, check
    passed, 4 files +120 −8, review with `git diff <base>..<head>`"). The
    step waits for `accept_step`.
16. On `blocked`, the summary goes to the leader. An agent whose state turns
    `done` with an open order and no report for 2 minutes is reported to the
    leader as "stopped without reporting".

### Delivery and the log
17. `devagentics watch <id>` is the task's dispatcher and the log pane. Every
    second it delivers each agent's oldest waiting message with
    `devws agent send` once that pane is `done` or `idle`; on busy it retries
    later. It notices panes that are gone, and it shows the log as it grows.
    One `watch` per task (a lock file); state is shared through file-locked,
    atomic writes, because the MCP server and `report` write too.
18. `log.jsonl` is append-only and written by devAgentics only:
    - `order`, `delivered`, `report`, `check`, `review`, `spawn`, `stop`,
      `escalate`, `limit`, `pr`;
    - each entry has a time, the agent and the step.

### Memory and recovery
19. The leader's role tells it to:
    - call `task_status` first and after every compaction;
    - write an ai-memory handoff after each accepted step;
    - record decisions about one repo in that repo's ai-memory project, and
      decisions about the whole task in `devagentics-tasks`.
    devAgentics itself never reads ai-memory for state.
20. A new leader (`task resume`, or the pane restarted) continues from
    `task_status` alone.

## Constraints

- Not in v1:
  - OpenCode and OpenRouter executors;
  - the two-failures → stronger-model rule;
  - jungleforge end-to-end runs;
  - stock tester and reviewer roles (a user can write them as role files);
  - Claude Stop-hook delivery.
- devws is used only through its documented commands, with no direct tmux
  calls. The one exception: devAgentics may set its own `@devagentics_*`
  pane options.
- Agents never message each other. Everything goes through the leader's tools
  and the dispatcher.
- No secrets in roles or the log; check output is saved as it is.
- Rust 2024, one binary. The devws seam is one module behind a trait, so tests
  use a fake devws.

## Files Involved

| File | Action |
| ---- | ------ |
| `Cargo.toml` | Modify: clap, serde, toml, serde_json, rmcp, tokio, fs2, globset, anyhow; dev: tempfile, assert_cmd |
| `src/main.rs` | Modify: CLI (`task`, `role`, `mcp`, `watch`, `report`) |
| `src/task.rs` | Create: task folder, `task.toml`, `state.json`, locking |
| `src/roles.rs`, `roles/*.md` | Create: role files, precedence, defaults |
| `src/agent_args.rs` | Create: role → claude/codex arguments |
| `src/devws.rs` | Create: the devws seam (trait + command implementation) |
| `src/orders.rs` | Create: queue, steps, reports, file rules |
| `src/checks.rs` | Create: clean tree, diff vs globs, check command |
| `src/watch.rs` | Create: dispatcher and log view |
| `src/mcp.rs` | Create: the leader's tools |
| `src/prs.rs` | Create: push and linked draft PRs (gh) |
| `src/log.rs` | Create: `log.jsonl` |
| `tests/` | Create: integration tests with a fake devws and temp git repos |
| `README.md`, `docs/usage.md` | Modify / create |

## Success Criteria

- [ ] `cargo test` and `cargo clippy -- -D warnings` pass.
- [ ] Unit: role precedence; claude/codex arguments per role; globs allow and
      deny; a dirty tree fails a step; the lock refuses a second writer; the
      agent and account limits refuse a spawn; the queue delivers only to a
      done or idle pane, oldest first.
- [ ] Integration (fake devws, temp repos): task new → approve → spawn →
      order → report done → check → accept, with the log holding each entry.
      A report that breaks `forbidden` fails. `open_prs` calls gh once per
      repo with the cross-links (gh stubbed).
- [ ] End to end on this machine: a two-repo test task (local API and web
      repos), a Claude and a Codex executor, one step each, two linked draft
      PRs on throwaway private repos. Killing the leader and running
      `task resume` continues the task.
