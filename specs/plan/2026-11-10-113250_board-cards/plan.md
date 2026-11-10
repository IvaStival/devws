# Plan: A tech-style board with the team's consumption

- **Date:** 10-11-2026
- **Branch:** feat/agentic
- **Status:** Executed 10-11-2026 — `make check` passed; installed and the
  team panel restarted on it

## Context

Implements [spec.md](spec.md) on top of the board redesigned on 10-11-2026
(background loading, sections the selection skips). Tests come first in each
step: `board_usage.rs` and `board.rs` tests for the figures, `board_view_tests.rs`
for the layout.

## Step 1 — Per-agent figures on the board

**Status:** Done

**Files:**
- `agentic/src/board_usage.rs`
- `agentic/src/board.rs`

**Actions:**
- Tests first: `UsageReader::of_team` also returns each agent's own usage (a
  stopped agent's last reading included), the role sums unchanged; `Board`
  gives each `AgentRow` its usage, the steps it got accepted and its retries
  (orders beyond the first on a step it was given), and the leader its usage.
- `of_team` returns `TeamUsage { by_role, by_agent }`; `AgentRow` gains
  `usage: Option<Usage>`, `accepted: u32`, `retries: u32`; `Board` gains
  `leader_usage: Option<Usage>`. `Board.usage` (the Usage view) keeps its rows.

**Verification:**
- `cargo test board` passes.

## Step 2 — Rows of different kinds

**Status:** Done

**Files:**
- `agentic/src/board_view.rs`
- `agentic/src/board_view_tests.rs`

**Actions:**
- Tests first: notes (an agent's second line) and gaps (blank lines) are never
  selected, as headings are; moving down from an agent skips its note.
- Replace `Entry.heading: bool` with `kind: Kind { Row, Heading, Note, Gap }`;
  `Entry::heading`, `Entry::note`, `Entry::gap`; navigation skips all but `Row`.

**Verification:**
- `cargo test board_view` passes (the existing heading tests unchanged in intent).

## Step 3 — Group sections with totals and two-line agents

**Status:** Done

**Files:**
- `agentic/src/board_view.rs`
- `agentic/src/board_view_tests.rs`

**Actions:**
- Tests first, at 84×40: sections NEEDS YOU, CTO, SENIORS, FLOOR, DONE, each
  after a gap and headed `▌NAME · count` with `$cost · tokens tok` at the right
  for groups; a group's total equals its agents' sum; an agent row has number,
  id, program, state, cost and tokens at fixed columns; its note shows
  `└ now <step>` when it has one and `N accepted · M retries` (parts left out
  when zero); the leader is the CTO's row.
- `fmt_tokens` (`4.6M`, `474M`, `900k`) and `fmt_cost` (`$31.40`, `+` when
  unpriced), shared with the Usage view's `human`/`dollars`.

**Verification:**
- `cargo test board_view` passes.

## Step 4 — The framed header with team totals and a braille progress bar

**Status:** Done

**Files:**
- `agentic/src/board_view.rs`
- `agentic/src/board_view_tests.rs`

**Actions:**
- Tests first: a rounded frame (`╭─ … ─╮`, `│`, `╰─╯`) holds the header; its
  title line has task · branch and `● N need you`; the progress bar is `⣿`
  filled and `⣀` free with `done/total · pct%`; the team line shows total cost
  and in/out/cache tokens; limit bars keep `▓░`; at 40 columns nothing
  overflows, the token column and program drop before the state; a pane too
  short for the frame keeps the title line.
- Draw the frame with ratatui's `Block` (rounded borders); the rows inside are
  the current header rows plus the team line.

**Verification:**
- `cargo test` passes, with `tests/flow.rs` and
  `scripts/test_agentic_cross.sh` updated for the new text.

## Step 5 — Documentation and going live

**Status:** Done

**Files:**
- `agentic/docs/usage.md`

**Actions:**
- Describe the groups, the per-agent figures, the team line and the bars.
- Install the build and restart the team panel (`q`, then
  `devagentics board <task>` in the same pane) while the team is idle.

**Verification:**
- `bash scripts/test_guide.sh` passes.

## Final Verification

1. `make check` passes, its own exit status recorded (not a pipe's).
2. `git diff --check` is clean.
3. Live: the restarted board shows groups with totals and agents with their
   cost, tokens and second lines.
