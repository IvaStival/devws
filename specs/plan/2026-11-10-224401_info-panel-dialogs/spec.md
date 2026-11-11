# Spec: Harmonize board colors and open content inside the info panel

- **Date:** 10-11-2026
- **Branch:** feat/menu-network-devices
- **Status:** Executed

## Context

The leader workspace's info panel presents the team board as cards. Its green structural borders dominate the interface. Opening an item's details currently replaces the board body with plain text. The user wants colors that fit the rest of devws and content displayed in a dialog using the new devws menu's visual language, confined to the info panel.

## Requirements

1. Use the menu's muted blue accent (terminal palette index 110) for structural card borders and titles, with cyan selection accents consistent with the existing interface. Keep green where it communicates successful or healthy status.
2. Open full content from board entries in a titled, bordered dialog within the info panel. Apply the menu's visual language: clear selection and title hierarchy, subdued framing, and concise keyboard hints.
3. Keep the board rendered behind the dialog. Confine the dialog to the info panel's terminal area so the leader, sidebar, and other panes remain visible and usable.
4. Preserve complete multiline content, wrapping and scrolling. Support arrows, j/k, Page Up/Down, Home/End, and mouse-wheel scrolling with bounds based on the dialog's content area.
5. Close the dialog with Esc or q and return to the same view and selected entry. Apply the same dialog treatment to board help.
6. Resize safely, including narrow and very small panes. Live updates must not reset scrolling unnecessarily or redirect the dialog to a different entry.

## Constraints

- Keep the leader panel layout and existing board views and agent navigation.
- Use the new menu's appearance without opening a workspace-wide popup. Reuse existing rendering conventions where practical; avoid introducing a separate UI toolkit or a second interactive process over the board.
- Scope this change to the board/info panel. Status colors across unrelated interfaces do not change.
- Do not add unrelated SSH, terminal, or network discovery changes.

## Files involved

| File | Expected change |
| --- | --- |
| `agentic/src/board_view.rs` | Card palette, dialog geometry, background and dialog rendering |
| `agentic/src/board_tui.rs` | Detail/help state and scrolling aligned with dialog geometry |
| `agentic/src/board_view_tests.rs` | Rendering, bounds, wrapping, background visibility, and semantic color coverage |
| `docs/guide/agentic.md`, `agentic/docs/usage.md` | Describe info-panel dialogs and controls where appropriate |
| `devws-ui/src/view.rs` | Reference for established menu styling; extract reusable code only if implementation warrants it |

## Success criteria

- [x] Card borders and titles use the agreed accent while success states retain their meaning.
- [x] Details and help open as dialogs fully contained in the info panel, with the board behind them.
- [x] Long content remains readable through scrolling and resizing, without clipping the last lines.
- [x] Closing restores the prior selection and board view.
- [x] Rendering and input regression tests pass, including tiny dimensions and content updates.
- [x] `make check` and `git diff --check` pass; visually inspect the resulting board and dialog.
