# Plan: The leader moves to another model when its account runs out

- **Date:** 05-11-2026
- **Branch:** feat/agentic
- **Status:** Executed

## Context

See `spec.md`. The leader falls back to Codex Sol (medium) when Claude has
used 95% of its week: at start, and while running, by a handover.

## Step 1 — The fallback in the role

**Files:** `agentic/src/roles.rs`, `agentic/roles/leader.md`

**Actions:**
- `Role.fallback: Option<Fallback { agent, model, effort }>`, read from a
  `[fallback]` table in the front matter; refused on a role other than the
  leader.
- `Role::on_fallback()`: the same role (instructions included) on the
  fallback's agent, model and effort.
- Built-in leader: `[fallback] agent = "codex", model = "gpt-6.1-sol",
  effort = "medium"`.

**Verification:** roles tests (parsed, refused elsewhere, the user's leader
without one still loads).

## Step 2 — Choosing at start

**Files:** `agentic/src/leader.rs` (new), `agentic/src/start.rs`,
`agentic/src/state.rs`, `agentic/src/log.rs`

**Actions:**
- `leader.rs`: `WEEK_LIMIT_PERCENT = 95`; `runs_out(devws, agent)` (the
  week ≥ 95); `choose(role, devws) -> (Role, reason)`: the fallback when the
  leader's account runs out and the fallback's does not.
- `start_task` writes `leader.args` for the chosen program and returns its
  agent; `State.leader_agent` records it, and the log says why.

**Verification:** start tests with the fake devws's limits: 95 picks Codex,
94 keeps Claude, both out keep Claude with a warning.

## Step 3 — The handover while running

**Files:** `agentic/src/leader.rs`, `agentic/src/watch.rs`,
`agentic/src/devws.rs`

**Actions:**
- `Devws::run(pane, agent, args_file)` → `devws agent run` (the pane keeps
  its place and role; the header and sidebar name the new agent).
- `leader::hand_over(task, devws)`: still on the leader's own agent, its
  account out, the fallback's not, the leader's pane not `working` → write
  `leader.args` for the fallback, with a first message "You take over as
  the leader: <agent> ran out of its week. Call task_status and read_log,
  then tell the user in one line that you took over." → `run` → state and
  log.
- `watch.tick` calls it once a minute (limits are read from devws's
  cached options, cheap).

**Verification:** watch tests: no handover while working; one handover once
idle; none twice; none when the fallback is out too.

## Step 4 — Board and docs

**Files:** `agentic/src/board.rs`, `agentic/src/board_view.rs`,
`agentic/docs/usage.md`, `docs/guide/agentic.md`

**Actions:** the leader row names its program (`leader · codex`) and the
log shows the handover; the docs describe the fallback, the 95% week rule
and how to change it in a user `leader.md`.

**Verification:** board test; `scripts/test_guide.sh`.

## Final Verification

1. `cargo test`, clippy `-D warnings`, `cargo fmt --check` in `agentic/`.
2. `scripts/check.sh` passes (cross test included).
3. Live: `devws agentic` with Claude's week at 95% or more starts the leader
   on Codex Sol, which can call `task_status`.

## Commits

- (pending)
