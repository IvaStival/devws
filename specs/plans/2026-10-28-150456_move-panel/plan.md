# Plan: Move a panel that already exists

- **Date:** 28-10-2026
- **Branch:** main
- **Status:** Executed (not committed)

## Context

New panel (C-b N, `new_panel.sh`) can open a panel right, left, below or
above the focused one, but a panel that is already open can't be moved. You
want to move it while respecting the layout: panels only change order, and
every slot keeps its size.

Your choices:
- **Direction:** pick Right, Left, Below or Above, and the focused panel
  trades places with its neighbour on that side. Repeat to move it further.
- **Shortcut:** `C-b Shift+arrow` moves it straight away, with no popup.
- **Menu:** the devws menu gets a **⇆ Move panel** entry too, which asks for
  the direction like New panel does.

When execution starts, the spec and plan are saved under
`specs/plans/<date>_move-panel/` (spec-plan skill).

## Approach

The move is tmux's `swap-pane`: the two panes trade places in the layout,
and the geometry stays the same.
- The programs keep running, since nothing is restarted.
- The pane options travel with the pane (`@devws_role`, `@devws_agent`), and
  so do its header labels.
- The layout toggle, minimize and the sidebar look panes up by role, so they
  keep working after a swap.

## Step 1 — `config/tmuxinator/move_panel.sh` (new)

`move_panel.sh <session> <right|left|below|above>`, written in the style of
`new_panel.sh` (same `fail()`, which reports through tmux's display-message
and exits 1).

1. **Validate:** the session must exist, and an unknown direction fails.
2. **Expand minimized panes first:** `minimize_panes.sh <session> restore all`,
   as `toggle_layout.sh` does. A minimized group's real panes live in a
   hidden window, so swapping a placeholder would mix them up.
3. **Focused pane:** `#{pane_id}` of the session's active pane.
4. **Neighbour:** tmux's own pane targets, `{right-of}`, `{left-of}`,
   `{down-of}` and `{up-of}`, resolved from the focused pane.
   - With no pane on that side (or when tmux hands back the same pane), it
     shows "Move panel: no panel to the <direction>" and leaves everything
     as it is.
   - Exactly what tmux returns at an edge gets checked while implementing.
5. **The sidebar never moves:** if the focused pane or the neighbour is the
   sidebar (pane title `devws-picker`, as `new_panel.sh` checks it), it says
   "the sidebar stays where it is" and changes nothing.
6. **Swap:** `tmux swap-pane -s <focused> -t <neighbour>`. Focus stays on the
   moved panel, so pressing the key again keeps moving it; I'll check that
   tmux's default does this, and add `select-pane` if it doesn't.
7. **Save:** `save_state.sh`, so a restore brings the new order back. It
   already records `#{window_layout}` and each pane's index and role.

## Step 2 — Menu, popup and keys

- **`config/tmuxinator/workspace_actions.sh`:** a `move_panel()` function next
  to `new_panel()`. It asks for the direction with the same fzf list
  (Right, Left, Below, Above; header "Move the panel you are in"), then calls
  `move_panel.sh`.
- **`config/tmuxinator/devws_menu.sh`:** a new row,
  `workspace|move-panel|⇆ Move panel|C-b S-←→↑↓|move_panel`, right after New panel.
- **`config/tmux/tmux.conf`,** next to `bind N`:
  `bind -r S-Right run-shell -b "~/.tmuxinator/move_panel.sh '#{session_name}' right"`,
  and the same for S-Left (left), S-Up (above) and S-Down (below).
  - `-r` lets you press the arrows repeatedly after one `C-b`.
  - These replace tmux's default prefix S-arrow bindings (`refresh-client`
    panning, used only when the client is smaller than the window), which
    devws doesn't use.

## Step 3 — Tests: `scripts/test_move_panel.sh` (new), added to `scripts/check.sh`

The setup mirrors `scripts/test_new_panel.sh`: an isolated tmux server
through the `-L` wrapper, the `DEFAULT_LAYOUT` (sidebar | editor on top,
agent and terminal below), stub programs, and an isolated `HOME`/`TMPDIR`.

| Test | Expectation |
| ---- | ----------- |
| The terminal moves left | It trades places with the agent. Each slot's size is unchanged (the window layout's geometry is the same, only the pane ids swapped), the roles moved with the panes, and the focus is on the terminal. |
| The editor moves below | It swaps with the pane under it, with the same checks. |
| No neighbour | Moving the agent left (the sidebar is there) or the editor above (nothing there) changes nothing. The second case also shows the message. |
| The sidebar stays put | The sidebar never moves, whether it is the focused pane or the neighbour. |
| Minimized group | The group is expanded first, then the move happens. |
| Saved | The saved state has the new order. |
| Menu and keys | The menu lists Move panel (`devws_menu.sh --items workspace`), and tmux.conf binds the four S-arrows to `move_panel.sh`. |

## Step 4 — README

Add a short **Moving panels** paragraph after **New panels** that covers:
- the menu entry and `C-b Shift+arrow` (repeatable);
- panels trade places, sizes stay, nothing restarts, and the sidebar stays put;
- the Workspace row of the menu table gains `Move panel C-b S-arrows`;
- `move_panel.sh` is added to the file list.

## Verification

1. `scripts/test_move_panel.sh` passes, and so does the full `make check`.
2. By hand, after `devws reload` (which reloads tmux.conf):
   - in a workspace, `C-b S-Right` on the editor moves it to the agent's slot,
     and pressing again keeps moving it;
   - the layout toggle and minimize still work afterwards;
   - the menu's Move panel asks for the direction and does the same.

## Critical files

- New: `config/tmuxinator/move_panel.sh`, `scripts/test_move_panel.sh`.
- `config/tmuxinator/workspace_actions.sh`, `devws_menu.sh`,
  `config/tmux/tmux.conf`, `scripts/check.sh`, `README.md`.
- Reused: `minimize_panes.sh` (restore all), `save_state.sh`, and the sidebar
  check by `pane_title` from `new_panel.sh`.

## Execution notes

- tmux's `{left-of}`/`{right-of}` wrap around the window and `{up-of}`/`{down-of}`
  return the pane itself at an edge, so `move_panel.sh` checks the neighbour's
  position before swapping.
- `swap-pane` leaves the focus on the place, not the pane; `select-pane` brings
  it back to the moved panel.
- `scripts/test_devws_menu.sh` gained `test_move_panel_asks_which_way`.
- `make check` passes (18 suites).
