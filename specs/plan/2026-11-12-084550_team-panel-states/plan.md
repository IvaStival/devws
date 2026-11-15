# Plan: Team panel states

- **Date:** 12-11-2026
- **Branch:** fix/team-panel-states (to be created from `main`)
- **Status:** Executed
- **Authorization:** User approved the plan on 12-11-2026.

## Context

Implement [the specification](spec.md): clear interrupted agents out of
`working`, and stop presenting the leader's reviews as the user's.

## Step 1 — Pin down the interrupt markers

**Status:** Complete. Taken from real transcripts on disk instead of new agent runs: Codex `turn_aborted` (hammer-7, 11-11, 41 s after the pause) and Claude Code `[Request interrupted by user for tool use]`. Turn boundaries: Codex `task_started` / `task_complete` / `turn_aborted`; Claude Code a user message with text (tool results excluded). The hook sets `working` a moment before a new prompt reaches the transcript, so an interrupt counts only when two sweeps in a row see it (~10 s, not ~5 s). The test lines are written into the test itself rather than saved as fixture files, because the real transcripts hold conversation content.

**Files:** none (notes recorded in this plan).

- Interrupt a throwaway Claude Code agent and a Codex agent mid-turn. Record the
  exact tail of each transcript right after the interrupt, and again after a new
  prompt.
- Already known: Codex writes `{"type":"turn_aborted","reason":"interrupted"}`.
  Claude Code writes a user message whose text starts with
  `[Request interrupted by user`.
- Decide the rule: the newest turn-boundary line in the last ~20 lines is an
  interrupt. The turn-boundary lines are a user prompt, a turn start, an
  interrupt, or a Stop. Lines such as Codex's `token_count` are ignored.

**Verification:** both fixtures saved under `scripts/fixtures/` for Step 2.

## Step 2 — Interrupt sweep (tests first)

**Status:** Complete. Test failed first (no script), then passed. Run against real transcripts: both cut at the interrupt are detected; the next prompt clears it; live agents were not affected.

**Files:** `scripts/test_agent_interrupt.sh` (new),
`config/tmuxinator/agent_interrupt.sh` (new),
`config/tmuxinator/header_ticker.sh`, `scripts/check.sh`, `install.sh`.

- Tests first, against a private tmux server as the other shell suites do:
  - a Claude pane interrupted → `idle`;
  - a Codex pane interrupted → `idle`;
  - a new prompt after the interrupt → stays `working`;
  - `waiting` and `done` panes are untouched;
  - a pane with no transcript is untouched;
  - a missing transcript file is untouched.
- `agent_interrupt.sh sweep`: one `tmux list-panes -a` call, filtered to panes
  whose `@devws_pane_state` is `working`. For each one, run `tail -n 20` on its
  `@devws_transcript` and `jq` the newest turn-boundary line. When that line is
  an interrupt, mark the pane `idle` through
  `agent_status.sh <session> idle <pane>`, which also updates the workspace
  summary and the sidebars.
- Call it from the ticker's 5-second source slot. When no pane is working, it
  costs one tmux call.

**Verification:** `scripts/test_agent_interrupt.sh` and `scripts/test_header.sh` pass.

## Step 3 — Leader reviews vs. needs you (tests first)

**Status:** Complete. New tests failed first, then passed. The field is `review_since` and is set through `Step::set_status`, used for every status change. `crate::log::now()` was pulled out of the log's timestamp code.

**Files:** `agentic/src/state.rs`, `agentic/src/orders.rs`,
`agentic/src/board.rs`, `agentic/src/board_view.rs`,
`agentic/src/board_view_tests.rs`.

- Tests first:
  - setting a step to Reported stamps `review_since`;
  - an old state file without the field loads;
  - the board puts a fresh review under `leader_review` and leaves it out of
    `needs_you`;
  - a review older than 10 minutes, with the leader not working, moves to
    NEEDS YOU with its age;
  - the same old review with the leader working stays under LEADER REVIEWING.
- `Step.review_since: Option<u64>` with `#[serde(default)]`, set where
  `orders.rs` moves a step to Reported (and cleared on any other status).
- `Board`: replace `review` with `leader_review` and `stale_review`. "Stale"
  means reported for more than `STALE_REVIEW_SECS = 600` (or with no report
  time), while `leader_state` is not `Working`. Failed, blocked and rejected
  steps follow the same rule, because the leader acts on them too.
- `board_view`: NEEDS YOU takes the stale entries, labeled
  `leader hasn't reviewed <id> · <age>`. A new LEADER REVIEWING card after the
  CTO card lists the others with their current marks (`●`, `⚠`).
  The header count follows NEEDS YOU.

**Verification:** `cargo test --manifest-path agentic/Cargo.toml` passes.

## Step 4 — Docs, checks and delivery

**Status:** Complete, except the live interrupt check (see Results).

**Files:** `agentic/docs/usage.md`, `docs/guide/agentic.md`, this plan and spec.

- Document the LEADER REVIEWING card, the 10-minute stale rule, and that an
  interrupted agent shows idle.
- Run `make check` and `git diff --check`.
- Run `cargo install --path agentic --locked`. Restart only the board pane of
  `feat-system-observer`, and reload the ticker
  (`header_ticker.sh --start`).
- Live check:
  - the board shows `ph` as `leader hasn't reviewed ph` while the leader is
    idle;
  - interrupting a test agent turns it idle.
- Record results here. No commit unless asked.

## Results

- `scripts/test_agent_interrupt.sh`: 7 cases pass (Claude/Codex interrupt, new prompt after it, prompt landing between sweeps, turn in progress, waiting/done/no transcript/missing file, pending flag cleared).
- `cargo test`: 151 passed (new: 3 state, 2 board split, 2 board view; updated order and board tests).
- `make check`: everything passed except `test_devws_menu` ("an agent team lost the job log"). It fails the same way on a clean `main` worktree, so it was already failing and isn't caused by this change. `test_agent_status` failed when run alongside the other suites and passed when run alone.
- `git diff --check` clean.
- Installed with `cargo install --path agentic --locked`; restarted only the board process in pane `%28` and the header ticker. Live board: NEEDS YOU shows `leader hasn't reviewed ph` and `leader hasn't reviewed general-observation-model` (both reported before `review_since` existed, leader done). Working agents `%8`, `%32` untouched by the sweep.
- Not done: interrupting a live agent end to end; the running team's agents were left alone. Covered by tests using real transcript formats and by checks against real transcript cut points.
- No commit made.
