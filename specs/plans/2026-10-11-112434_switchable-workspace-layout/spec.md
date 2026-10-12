# Spec: Switchable workspace layout

- **Date:** 11-10-2026
- **Branch:** feat/switchable-workspace-layout
- **Status:** Executed

## Context

Every devws workspace opens with the picker sidebar on the left, the editor on
top, and the agent and terminal side by side underneath. While reading code an
agent is changing, a full-height editor is more useful, with the agent and
terminal stacked in a right-hand column. The user wants to flip between both
arrangements whenever they like.

## Requirements

1. A second arrangement: picker (when open) on the left, editor full height in
   the middle, agent on top and terminal below it in a right column.
2. One action switches the current workspace between the default ("stacked")
   and the new ("side") arrangement, in both directions.
3. The action is available from the picker sidebar's WORKSPACE COMMANDS and as
   a tmux shortcut (`C-b \`).
4. Switching keeps every pane's process running and keeps keyboard focus on the
   same pane.
5. Extra terminals move together with the terminal pane.
6. The chosen arrangement survives a save/restore.

## Constraints

- New workspaces keep the current default layout (`dev.yml` is unchanged).
- No change to the saved-state file format.
- No pane is restarted or recreated.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/toggle_layout.sh` | Create — rearranges the panes |
| `config/tmuxinator/session_picker.sh` | Modify — add "Toggle layout" action |
| `config/tmux/tmux.conf` | Modify — add `C-b \` binding |
| `scripts/test_workspace_layout.sh` | Create — tests against a real tmux server |
| `scripts/check.sh` | Modify — run the new test |
| `README.md` | Modify — document the toggle |

## Success Criteria

- [ ] Toggling once gives the side arrangement; toggling again restores stacked.
- [ ] Works with the picker closed and with extra terminals.
- [ ] Fails with a message, leaving panes untouched, when a role pane is missing.
- [ ] `scripts/check.sh` passes.
