# Spec: Move a panel that already exists

- **Date:** 28-10-2026
- **Branch:** main
- **Status:** Executed (not committed)

## Context

New panel (C-b N) opens a panel right of, left of, below or above the focused
one, but an open panel can't be moved. The user wants to move panels while
respecting the layout: panels only change order, every slot keeps its size.

## Requirements

1. The focused panel trades places with its neighbour on a chosen side
   (Right, Left, Below, Above); repeating moves it further.
2. `C-b Shift+arrow` moves it straight away (repeatable after one prefix).
3. The devws menu's Workspace tab has a **⇆ Move panel** entry that asks for
   the direction.
4. Sizes stay, nothing restarts, roles travel with the panes, focus stays on
   the moved panel, and the workspace is saved afterwards.
5. The sidebar never moves; with no panel on that side nothing changes and a
   message says so. A minimized group is expanded first.

## Constraints

- No re-splitting or layout strings: `swap-pane` only.
- tmux's `{left-of}`-style targets wrap around the window, so the neighbour is
  checked against the focused pane's geometry.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/move_panel.sh` | Create — the move |
| `config/tmuxinator/workspace_actions.sh` | Modify — `move_panel()` asks the direction |
| `config/tmuxinator/devws_menu.sh` | Modify — menu entry |
| `config/tmux/tmux.conf` | Modify — `C-b S-arrows` |
| `scripts/test_move_panel.sh` | Create — tests |
| `scripts/check.sh` | Modify — run the new suite |
| `README.md` | Modify — Moving panels |

## Success Criteria

- [ ] `scripts/test_move_panel.sh` and `make check` pass.
- [ ] By hand: `C-b S-Right` on the editor swaps it with the agent; again moves it further.
