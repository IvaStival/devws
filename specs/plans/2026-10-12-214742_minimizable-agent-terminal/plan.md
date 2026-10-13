# Plan: Minimizable agent and terminal panes

- **Date:** 12-10-2026
- **Branch:** feat/pane-headers
- **Status:** Executed

## Context

Implements `spec.md` in this folder.

## Step 1 — Shared pane helpers

**Files:** `config/tmuxinator/workspace_panes.sh` (new), `config/tmuxinator/toggle_layout.sh`

**Actions:**
- Move pane lookup (`pane_with_role`, `pane_field`, extras) and
  `is_side_layout` out of `toggle_layout.sh` into a sourced library.
- `toggle_layout.sh` sources it and, before moving panes, asks
  `minimize_panes.sh <session> restore` to expand minimized panes.

**Verification:** `scripts/test_workspace_layout.sh` still passes.

## Step 2 — Minimize/restore script

**Files:** `config/tmuxinator/minimize_panes.sh` (new)

**Actions:**
- `minimize_panes.sh [session] [toggle|restore]`.
- Minimized is read from geometry: agent height ≤ 1 (default layout) or
  agent width ≤ 14 (full-height-editor layout).
- Minimize: store `#{window_layout}` in window option
  `@devws_expanded_layout`, move focus to the editor if it was on
  agent/terminal/extra, then `resize-pane -y 1` (default) or `-x 14` (side)
  on the agent.
- Restore: apply the stored layout; if that fails or none is stored, use the
  layout's default proportions (bottom row 50 % / right column 35 %).
  Clear the stored layout.

**Verification:** new real-tmux tests (Step 4).

## Step 3 — Headers and bindings

**Files:** `config/tmux/tmux.conf`, `config/tmuxinator/dev.yml`

**Actions:**
- Global `pane-border-format`: `▾`/`▸` (from geometry) + `Agent · <agent>` or
  `Terminal`; blank for other panes. `dev.yml` turns on
  `pane-border-status top` for workspace windows only.
- After the default-key reset: `MouseUp1Border` → toggle when the clicked
  pane is agent/terminal/extra; `C-b _` → toggle.

**Verification:** load the config in an isolated server and check bindings.

## Step 4 — Tests

**Files:** `scripts/test_workspace_minimize.sh` (new), `scripts/check.sh`

**Actions:** real isolated tmux server; cases:
- minimize/restore in both layouts (sizes, previous sizes restored);
- default proportions when the stored layout no longer fits;
- focus moves off a minimized pane;
- layout toggle while minimized yields full-size panes;
- headers render the expected labels and arrows;
- real mouse click on a header toggles (nested client), a drag does not.

## Step 5 — Docs

**Files:** `README.md`

## Final Verification

1. `./scripts/check.sh` passes.
2. Manual: click headers and `C-b _` in a real workspace.

## Step 6 — Placeholders and icon labels (amendment)

**Files:** `config/tmuxinator/minimized_placeholder.sh` (new),
`minimize_panes.sh`, `toggle_layout.sh`, `session_picker.sh`, `save_state.sh`,
`config/tmux/tmux.conf`, `scripts/test_workspace_minimize.sh`

**Actions:**
- Minimize: split a placeholder off the agent, `break-pane` it into a hidden
  window (new-window would trigger tmux.conf's rename prompt), add the other
  placeholders there, then `swap-pane` each real pane with its placeholder.
  Restore swaps back and kills the hidden window.
- Header format and click binding accept the `minimized-*` roles; icons are
  written as escapes (the literal glyphs were lost when writing files).
- Toggle layout expands before validating panes; picker actions expand first;
  save_state maps `minimized-*` roles back.

**Verification:** tests assert the real panes sit in the hidden window with
their processes intact, placeholders fill the slots, headers and placeholder
text render with icons, clicks on minimized headers restore.
