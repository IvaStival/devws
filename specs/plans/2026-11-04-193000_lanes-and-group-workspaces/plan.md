# Agent teams: lanes, group workspaces, hammers that stay

## Context
Seen live on publisherProvider: the CTO stops a hammer after every step and
starts another; only one hammer can write at a time (one writer per
worktree); and the agents, now in tabs, still sit inside the CTO's workspace.
The user wants: hammers reused between steps; work split between hammers by
**area** (back, front, infrastructure, documentation…) so they work at the
same time without breaking each other; and each group in **a workspace of its
own**, listed under the task in the sidebar.

## Design

### Lanes (devAgentics)
- A **lane** is an area of the repo: a name and its globs (`back`:
  `backend/**`; `front`: `frontend/**`; `docs`: `docs/**`…). The senior's
  execution plan names each step's lane; the CTO declares lanes with a new
  tool `add_lane(name, globs, repo)` (stored in `task.toml`).
- Each lane has **its own worktree** on a sub-branch of the task branch
  (`feat/x--back`), made with `devws worktree ensure`. One hammer writes a
  lane at a time; different lanes write at the same time.
- `spawn_agent(role, lane)` opens the hammer in its lane's worktree;
  `send_order` to it gets the lane's globs as `allowed` (plus the order's
  own narrowing) — a file outside its area fails the step, as today.
- Before an order is typed in, the lane catches up with the task branch
  (`git merge` of the task branch into the lane; no clash since areas are
  disjoint). `accept_step` merges the lane into the task branch. A clash (a
  plan that crossed areas) refuses with the files named, for the CTO to fix
  by a new order.
- `open_prs` pushes the task branch only, as today.

### Hammers stay
- The leader's prompt: keep hammers and seniors running; give a finished
  hammer the next step of its lane; stop agents only at the end, near an
  account's limit, or to make room. `max_agents` default 3 → 6.

### A workspace per group (devws)
- `team_tabs.sh` makes **sessions** instead of windows: `<workspace>-seniors`,
  `<workspace>-floor`, `<workspace>-floor-2` (4 agents each), with the CTO's
  workspace's `@devws_task` (the sidebar lists them under the task),
  `@devws_template agentic` (restore leaves them to devAgentics), and the
  same header/status options as a workspace.
- `go` switches the client to the agent's session; `peek` still joins the
  pane under the board (across sessions) and sends it back.
- An emptied group workspace closes; the sidebar's agent rows move under
  their group workspace.

### Already done in this turn (tested)
- Header ticker restarted; an agent event restarts a dead ticker.
- jungleforge **Info** in the App tab.
- Hammers in auto mode (`approvals = "auto"`: Claude `--permission-mode
  auto`, Codex `--approve-for-me`); Codex may write the repo's git data.
- Team workspaces are never frozen by the idle sweep.

## Steps
1. devws: group sessions in `team_tabs.sh` (+ tests in `test_new_panel.sh`,
   sidebar rows, cross test).
2. devAgentics: lanes in `task.toml`, `add_lane`, lane worktrees,
   `spawn_agent(role, lane)`, `send_order` lane globs, catch-up and merge on
   accept (unit + flow tests with two lanes in parallel and a clash).
3. Leader prompt and senior prompts: lanes in the plan, reuse, max 6.
4. Board: each agent's lane; docs.

## Verification
- `scripts/check.sh` (all suites, Rust, cross test with two hammers on two
  lanes at once).
- Live on publisherProvider: back and front hammers working together, each
  in the Floor workspace, the CTO accepting and the task branch holding both.
