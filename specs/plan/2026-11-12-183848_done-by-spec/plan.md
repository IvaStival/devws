# Plan: DONE grouped by spec, with a fixed height

- **Date:** 12-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed
- **Authorization:** The user approved the recommendation in chat ("we can follow with this") and added the fixed height.

## Context

The board's DONE card lists every accepted step, newest first (`DONE · 101`), so one piece of work such as H2f takes seven rows. In `giacomeli/unni-comp`, finished work is read by spec: each spec (`specs/etapa-N-*.md`) ships as a few PRs, and later specs summarize the earlier ones as "etapa 5: #66 e #67". DONE should read the same way: one row per finished spec. The card also takes all the room left in the pane today; it gets a fixed height instead.

## Step 1 — A step's spec

**Files:** `agentic/src/state.rs`, `agentic/src/orders.rs`, `agentic/src/mcp.rs`

- `Step` gets `spec: Option<String>` (`serde(default)`, skipped when empty), so existing `state.json` files still load.
- `OrderRequest` gets `spec: Option<String>`. `send_order` records it on a new step, and on an existing step when it is given.
- The `send_order` tool takes an optional `spec` ("the spec or plan part this step belongs to: H2f, etapa-5; the board groups finished steps by it").

## Step 2 — Groups on the board

**Files:** `agentic/src/board.rs`

- **New** `SpecRow { id, title, steps: Vec<StepRow>, retries }`. `Board.done_specs` lists the specs whose steps are all accepted, newest first (by their latest step).
- A step's group is its `spec`. Without one, it falls back to the id's prefix before the first `.` or `-` when that prefix has a digit (`H2f.3`, `H2f-execution` → `H2f`). Otherwise the step is a group of its own (`replay-contracts`, `s1`).
- The group's title is its oldest step's title.
- `Board.done` (the accepted steps) stays as it is for the counts and the progress bar.

## Step 3 — The DONE card

**Files:** `agentic/src/board_view.rs`, `agentic/src/board_view_tests.rs`

- DONE shows one row per finished spec: `✓ H2f  <title>  7 steps · 2 retries`. The heading reads `DONE · <specs>` with the tail `<steps> steps · newest first`.
- Enter on a spec row shows its steps, each with its retries.
- **Fixed height:** the DONE card shows at most `DONE_ROWS` (15, raised from 8 at the user's request) rows. It scrolls within them, keeps its scrollbar and its `1–15 of N` footer, and shrinks only when the pane is too short. `list_rows` counts it the same way, so `board --once` is not taller than the card.

## Step 4 — Docs and install

- `agentic/docs/usage.md`: DONE by spec, its fixed height, and the `spec` field of `send_order`.
- Leader role guidance (`agentic/roles/leader.md`): pass `spec` when ordering steps of a plan part.
- Run `cargo test`, `cargo fmt --check` and `cargo clippy --all-targets`. Install the binary and restart the running board.

## Verification

- Unit tests: grouping (the `spec` field, the prefix fallback, specs still in progress left out, newest first), old state without `spec` loading, `send_order` recording `spec`.
- View tests: spec rows and details, DONE capped at 8 rows with a scrollbar.
- Live: `devagentics board feat-system-observer --once` shows DONE grouped (H2f as one row).
