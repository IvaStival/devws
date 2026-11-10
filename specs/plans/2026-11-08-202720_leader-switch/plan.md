# Plan: The leader switches between its programs on request and near the 5-hour limit

- **Date:** 08-11-2026
- **Branch:** feat/agentic
- **Status:** Executed (not committed yet)

## Context

Implements [spec.md](spec.md): the leader moves between its role's program
and its `[fallback]` both ways — automatically when the leading account has
used 95% of its 5-hour window or week and the other has not, and on request
(the leader's `switch_leader` tool, or the devws menu's "Switch leader
model"). The board performs every move once the leader's turn is over.

## Step 1 — Run out by either window; the other program (leader.rs)

**Files:**
- `agentic/src/leader.rs`

**Actions:**
- Tests first: run out at 95% of `five_hour` (not 94%), at 95% of `week`;
  `choose` picks the fallback when the role's 5-hour window is out.
- `run_out` returns which window and how much (`"5-hour window" 96%` /
  `"week" 95%`): the max over both windows past `WEEK_LIMIT_PERCENT`, renamed
  `LIMIT_PERCENT`.
- `programs(role) -> Option<(Role, Role)>`: the role and its fallback;
  `current_and_other(role, state)`: which one leads now (`leader_agent`) and
  the other.
- Module docs and `choose`'s messages say "window" instead of "week".

**Verification:**
- `cargo test leader` passes.

## Step 2 — Both ways, and a requested switch (state.rs, leader.rs)

**Files:**
- `agentic/src/state.rs`, `agentic/src/leader.rs`

**Actions:**
- Tests first: a running leader on Codex moves to Claude when Codex's 5-hour
  window hits 95% and Claude has room; stays when both are out; not while
  working; no move back right after; a requested switch is performed with
  room left, once, cleared, logged with the reason; `request_switch` refused
  without a fallback and when the other is out.
- `State.leader_switch: Option<String>` (the reason of a pending request).
- `request_switch(home, task, devws, reason) -> Result<String>`: checks
  there is another program and it is not out, records the request, says
  "the leader moves to X once its turn is over".
- `hand_over`: target is the other program; acts when a request is pending
  or the current is out and the other is not; drops the "never back" rule;
  `TAKE_OVER` built with the reason (ran out of its 5-hour window / the user
  asked); clears the request in the same transaction.
- `pending_switch(task) -> bool`, cheap, for the board.

**Verification:**
- `cargo test leader` passes; existing start and board tests still pass.

## Step 3 — The leader's tool and the CLI (mcp.rs, main.rs)

**Files:**
- `agentic/src/mcp.rs`, `agentic/src/main.rs`

**Actions:**
- Tests first: `switch_leader` is listed; calling it records the request and
  returns the message (MCP fixture).
- `switch_leader` tool (`reason` optional): description tells it to tell the
  user in one line and end its turn.
- `devagentics leader switch <task> [--reason]` → `request_switch`, prints
  its message, fails with it when refused.

**Verification:**
- `cargo test mcp` passes; `devagentics leader switch --help` works.

## Step 4 — The board performs a request promptly (board_tui.rs)

**Files:**
- `agentic/src/board_tui.rs`

**Actions:**
- In `dispatch`, call `hand_over` every tick when `pending_switch` is true;
  limits are still read only every `LEADER_EVERY`.

**Verification:**
- `cargo test` passes; clippy clean.

## Step 5 — task_status says who leads and what the other has (status.rs)

**Files:**
- `agentic/src/status.rs`

**Actions:**
- Test first: `task_status` has `leader: { program, other, other_limits,
  switch_pending }` when the role has a fallback.
- Add that object to the summary.

**Verification:**
- `cargo test status` passes.

## Step 6 — The leader's instructions (roles/leader.md)

**Files:**
- `agentic/roles/leader.md`

**Actions:**
- Fallback comment: either program, 95% of the 5-hour window or week.
- A paragraph: when the user asks you to move to another model or program,
  call `switch_leader`, tell them in one line, end your turn; never say you
  cannot; it also happens on its own near a limit.

**Verification:**
- `cargo test roles` (the built-in roles parse).

## Step 7 — The devws menu command (shell)

**Files:**
- `config/tmuxinator/devws_menu.sh`, `config/tmuxinator/workspace_actions.sh`,
  `scripts/test_devws_menu.sh`

**Actions:**
- Test first: the entry is listed only when the session has `@devws_task`;
  running it calls `devagentics leader switch <task>` (stubbed via
  `DEVWS_AGENTIC_COMMAND`).
- Menu line `agent|switch-leader|⇄ Switch leader model||switch_leader_model|agentic`
  and a `shown` case `agentic` (the session has `@devws_task`).
- `switch_leader_model` in workspace_actions.sh: runs the command with the
  session's task, prints what it said, waits for Enter.

**Verification:**
- `scripts/test_devws_menu.sh` passes; shellcheck clean.

## Step 8 — Docs

**Files:**
- `agentic/docs/usage.md`, `docs/guide/agentic.md`

**Actions:**
- Describe both ways, the 5-hour/week rule, `switch_leader`, the menu entry
  and `devagentics leader switch`.

**Verification:**
- Read through; links valid.

## Final Verification

1. `scripts/check.sh` passes (cargo test, clippy `-D warnings`, shell tests,
   shellcheck).
2. `cargo install --path agentic --locked` so the running board picks it up
   on its next start.
3. Live: in the running team, tell the leader "switch to Claude" (or use the
   menu entry); its pane restarts on Claude, which catches up and says so.
