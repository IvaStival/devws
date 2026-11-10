# Plan: A responsive, modern team board

- **Date:** 09-11-2026
- **Branch:** feat/agentic
- **Status:** Executed 10-11-2026 — `make check` passed; build installed;
  the live board pane still runs the old build until it is restarted

## Context

Implements [spec.md](spec.md). First the board stops blocking input on slow
`devws` calls; then the Overview becomes one scrollable list of sections under
a header with bars. Tests are written before each change, in
`board_view_tests.rs` (pure rendering and navigation) and `board_tui.rs`'s
tests (the loop's parts, with the existing `Devws` test double).

## Step 1 — Read the board in the background

**Status:** Done

**Files:**
- `agentic/src/board_tui.rs`

**Actions:**
- Test first: a `Loader` started with a test `Devws` whose `usage` sleeps
  2 s returns its current snapshot from `latest()` immediately while a read
  runs, and the next snapshot after the read ends; a failed read is reported
  as the notice, keeping the last good board.
- `Loader` owns a thread that runs `load` every `REFRESH` (and at once on
  `r`), publishing `(Board, Option<problem>)` through a `Mutex`; it stops when
  the board quits. `show` reads only `latest()`. The first board is still read
  before the screen opens, so it never starts empty.

**Verification:**
- `cargo test board_tui` passes; the new test fails against the old loop's
  synchronous `load`.

## Step 2 — Handle every pending input before drawing

**Status:** Done

**Files:**
- `agentic/src/board_tui.rs`

**Actions:**
- Test first: a sequence of six wheel-down events moves the selection by
  `6 × WHEEL` rows in one pass (a pure `apply_events` over a list of events).
- After `event::poll(KEY_WAIT)` succeeds, keep reading while
  `event::poll(Duration::ZERO)` is true, applying each, then draw once. Quit
  and navigation actions still run in order.

**Verification:**
- `cargo test board_tui` passes.

## Step 3 — Section headings the selection skips

**Status:** Done

**Files:**
- `agentic/src/board_view.rs`
- `agentic/src/board_view_tests.rs`

**Actions:**
- Tests first: `move_by`, `g/G` and `reconcile` never leave the selection on a
  heading; a list that is only headings selects nothing; the view scrolls so
  the heading above the first selected row stays visible.
- `Entry` gains `heading: bool` (`Entry::heading(title)`); `Navigation`
  steps over headings in both directions.

**Verification:**
- `cargo test board_view` passes.

## Step 4 — The new Overview

**Status:** Done

**Files:**
- `agentic/src/board_view.rs`
- `agentic/src/board_view_tests.rs`
- `agentic/src/board.rs` (only if the leader's state is not on `Board` yet)

**Actions:**
- Tests first, at 84×40 with the test board plus 101 steps: the order is
  NEEDS YOU, TEAM, QUEUED, DONE; Needs you holds the question, the step to
  review and waiting/gone agents; Team's first row is the leader (`L`, program,
  state mark), then agents with number, id, group and lane or model, state
  mark; Queued is one line with its count; Done lists accepted steps newest
  first with `×n` for retries, and the wheel reaches its last row; empty
  sections are not shown; the summary sentence and `Leader:` line are gone.
- Build those entries in `entries(View::Overview)`; state marks `⠋ working`,
  `● needs you`, `✓ done`, `✕ gone`; long titles trimmed with `…` so the mark
  stays at the right edge.

**Verification:**
- `cargo test board_view` passes; `board --once --width 84` shows the sections.

## Step 5 — The header with bars, and the short footer

**Status:** Done

**Files:**
- `agentic/src/board_view.rs`
- `agentic/src/board_view_tests.rs`

**Actions:**
- Tests first: row 1 is `task · branch` with `● N need you` right-aligned
  (absent at 0); row 2 the tabs, current one bold; row 3 the progress bar
  `▓…░ done/total steps`; one row per account with 5h and week bars and
  percentages, colored green / yellow from 60 % / red from 85 %; at 40 columns
  the week bar is dropped first and no row overflows; at short heights the
  attention count stays; the footer is one short line that fits.
- One `header` function used by every view (replacing `header_rows`' fixed
  rows); `bar(percent, cells)` shared by progress and limits.

**Verification:**
- `cargo test` passes, including the existing narrow and tiny-size tests
  (updated where they asserted the old header text).

## Step 6 — Steps view marks

**Status:** Done

**Files:**
- `agentic/src/board_view.rs`
- `agentic/src/board_view_tests.rs`

**Actions:**
- Test first: each step row starts with its status mark (`✓` accepted,
  `⠋` ordered, `⚠` failed or blocked, `·` pending) and keeps `c` for history.

**Verification:**
- `cargo test board_view` passes.

## Step 7 — Documentation and going live

**Status:** Done

**Files:**
- `agentic/docs/usage.md`
- `docs/guide/agentic.md`

**Actions:**
- Describe the Overview's sections, the bars and that the wheel scrolls all of
  it.
- Install the build (`cargo install --path agentic`) while the team is idle
  and restart the board pane so it runs it.

**Verification:**
- `bash scripts/test_guide.sh` passes.

## Final Verification

1. `make check` passes (Rust format, Clippy, tests, and the shell suites).
2. `git diff --check` is clean.
3. Live: the restarted board shows the new Overview, scrolls to the end of Done
   with the wheel, and keeps up with a fast wheel while agents run.
