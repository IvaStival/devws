# Spec: Minimizable agent and terminal panes

- **Date:** 12-10-2026
- **Branch:** feat/pane-headers
- **Status:** Executed

## Context

While coding, the agent and terminal panes take space the editor could use,
but closing them loses the running agent and shell. The user wants a header on
the agent and terminal panes that collapses them to a small bar, showing they
still exist, and expands them again later.

A prototype on tmux 3.7b (nested client receiving real mouse clicks) showed:

- `pane-border-status top` gives each pane a header line; a click anywhere on
  it fires the `Border` mouse event with `mouse_pane` set to the pane below it.
- Clickable `range=control|N` buttons only work in the status line, not in
  pane headers, so the whole header is the click target.
- The click does not report its position, and a press on a border also starts
  drag-resizing (`MouseDown1Border`/`MouseDrag1Border`), so the toggle must
  react to a click-release without a drag (`MouseUp1Border`).

Decisions taken with the user: agent and terminal minimize **together**, in
both layouts; there is a keyboard shortcut as well as the click.

## Requirements

1. In devws workspaces, the agent, terminal and extra-terminal panes show a
   header with their name and state (e.g. `▾ Agent · claude`, `▾ Terminal`;
   `▸ …` when minimized). Picker and editor headers stay blank.
2. Clicking any of those headers (press and release without dragging)
   minimizes agent + terminal (+ extras) together; clicking again restores
   them.
3. `C-b _` does the same from the keyboard (`C-b m` is tmux's mark-pane key,
   kept as is).
4. Minimized, the panes become bars and the editor takes the space:
   - default layout: the bottom row shrinks to one line under its headers;
   - full-height-editor layout: the right column shrinks to a narrow strip.
5. Restoring brings back the sizes from before minimizing when the panes are
   unchanged; otherwise it falls back to the layout's default proportions.
6. Processes keep running; keyboard focus stays where it was, except that a
   minimized pane that had focus hands it to the editor.
7. Toggling the layout while minimized restores the panes first, so the
   layout toggle always produces full-size panes.
8. Minimized state is read from pane geometry, so it stays correct after a
   save/restore without a new state field.

## Constraints

- Border drag-resizing and tmux's default `MouseDown1Border` behaviour keep
  working.
- Headers apply to devws workspace windows only, not every tmux window.
- No change to the saved-state file format.
- Pane discovery/layout detection is shared with `toggle_layout.sh`, not
  copied.
- Known limit: the editor pane gains a blank header row, since tmux headers
  are per window, not per pane.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/workspace_panes.sh` | Create — shared pane lookup and layout detection |
| `config/tmuxinator/toggle_layout.sh` | Modify — use the shared helpers; restore before toggling |
| `config/tmuxinator/minimize_panes.sh` | Create — minimize/restore agent + terminal |
| `config/tmuxinator/dev.yml` | Modify — enable headers and their format on workspace windows |
| `config/tmux/tmux.conf` | Modify — `MouseUp1Border` and `C-b _` bindings after the key reset |
| `scripts/test_workspace_minimize.sh` | Create — real-tmux tests, including real mouse clicks |
| `scripts/check.sh` | Modify — run the new test |
| `README.md` | Modify — document headers and minimizing |

## Success Criteria

- [ ] Headers show the right labels and state in both layouts.
- [ ] A header click (real mouse event) minimizes and restores; a border drag still resizes without toggling.
- [ ] `C-b _` minimizes and restores.
- [ ] Previous sizes come back after a restore; default proportions when panes changed.
- [ ] Layout toggle while minimized gives full-size panes.
- [ ] `scripts/check.sh` passes.

## Amendment (12-10-2026)

The first version shrank the real panes, which left a squeezed line of Claude
or the shell in each bar and cut the header labels in the narrow strip. After
review the user asked for both fixes:

- While minimized, the real agent/terminal/extra panes move to a hidden window
  (`devws-minimized`, left out of the status bar) and keep running; a
  placeholder (`minimized_placeholder.sh`) takes each slot and shows an icon
  plus "minimized — click the header or press C-b _", or only the icon in the
  narrow strip.
- Headers use Nerd Font icons and a short label when minimized
  (`▸ 󰚩 Agent`, `▸  Term`) so they fit the strip.
- Picker actions that need the real panes (switch/restart agent, new
  terminal) expand them first; saved state records placeholders under the
  role they stand in for.
