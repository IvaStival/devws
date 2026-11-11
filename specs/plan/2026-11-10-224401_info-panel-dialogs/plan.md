# Plan: Info-panel colors and dialogs

- **Date:** 10-11-2026
- **Branch:** feat/menu-network-devices
- **Status:** Executed
- **Authorization:** User requested continuing with implementation after reviewing the saved specification.

## Context

Implement [the specification](spec.md) using the existing board renderer and input loop. Keep dialogs within the info pane and preserve agent navigation.

## Step 1 — Rendering and geometry

**Status:** Complete. Rendering regressions first failed for the old palette and replacement detail view, then passed with the new renderer.

**Files:** `agentic/src/board_view.rs`, `agentic/src/board_view_tests.rs`

- Add regression coverage for the muted blue palette, board visible behind details/help, wrapping, final-line scrolling, and tiny sizes.
- Render the board first, then a titled bordered dialog with an opaque background and keyboard hints.
- Share dialog content geometry and scroll bounds with the input loop.

**Verification:** Run board rendering tests; inspect rendered output.

## Step 2 — Interaction and refresh

**Status:** Complete. Board rendering/input tests pass, including help over the board, final-line wrapping after resize, keyboard and mouse scrolling, and removed entries.

**Files:** `agentic/src/board_tui.rs`, `agentic/src/board_view.rs`

- Use dialog dimensions for page movement and scroll clamping.
- Show help over the actual board; preserve selection when closing.
- Prevent a refreshed or removed entry from silently changing the open detail target.
- Cover keyboard and mouse scrolling, closing, refresh, and resizing with regression tests.

**Verification:** Run board input and rendering tests.

## Step 3 — Documentation and delivery

**Status:** Complete. User guides updated, all checks passed, and the updated executable is installed. The existing reload script deliberately leaves the board process running, so installing a new binary alone does not update an already-open info panel. Refreshed only the board pane to apply this update.

**Files:** `docs/guide/agentic.md`, `agentic/docs/usage.md`, this plan and specification.

- Describe dialogs, colors, and controls in existing user guides.
- Run `make check` (includes Rust formatting, clippy, tests, and shell validation) and `git diff --check`.
- Build the updated executable and refresh the running board if it can be done without interrupting active work. Verify pane-contained dialogs visually.
- Record final verification and any runtime limitations. No commit requested.

**Verification:** Final checks pass and installed/running version is checked explicitly.

## Results

- Regression tests caught the old green frames, replacement detail view, and an open dialog changing targets when its row was removed.
- `cargo test --manifest-path agentic/Cargo.toml board_ --quiet` passed.
- `make check` passed: agentic formatting, clippy, 142 unit tests and 5 integration tests; devws-ui formatting, clippy and 24 tests; all shell suites.
- `git diff --check` passed.
- `cargo install --path agentic --locked` installed the release executable.
- Restarted only pane `%8` (`devagentics board feat-system-observer`), preserving the leader and other panes. Inspected live tmux renders of Overview, details and help: structural borders use palette 110, successful states retain green, and both dialogs stay inside the info pane with the board behind them.
- Closed the verification dialog and restored the previously selected row. No commits created.
