# Spec: Tabbed devws menu

- **Date:** 14-10-2026
- **Branch:** feat/devws-menu
- **Status:** Executed

## Context

devws's actions live in one long list at the top of the sidebar ("WORKSPACE COMMANDS", 12
entries), and several only have their own keys (`C-b \`, `C-b _`, `C-b B`, `C-b F`). The user
wants one default menu for all of them, grouped into tabs. Decisions: a popup opened with
`C-b Space`; four tabs; the sidebar keeps the workspace list plus one entry that opens the menu.

## Requirements

1. `C-b Space` opens a centred popup menu from any pane of a devws workspace.
2. The menu has four tabs, each listing its actions with their shortcut, if any:
   - **Workspace:** New workspace · Open folder · Toggle layout (`C-b \`) · Minimize agent and
     terminal (`C-b _`) · New terminal · Close workspace
   - **Agent:** Switch agent · Restart agent · Initialize agent · Agent follow (`C-b F`)
   - **Editor:** Switch editor · Agent follow (`C-b F`)
   - **Database:** Database (`C-b B`) · Database folder
3. Keys: `←`/`→` and `Tab`/`Shift-Tab` change tabs; `↑`/`↓` move; `Enter` runs the action;
   `Esc` closes. Clicking a tab switches to it; clicking an action runs it. Typing filters the
   current tab.
4. Running an action closes the menu, then the action runs as it does today. Actions that ask
   something (agent, editor, folder, workspace to close…) ask inside the popup.
5. The sidebar's command list is replaced by one line, **☰ Menu  C-b Space**, which opens the
   same popup; the workspace list stays as it is.
6. The menu opens on the tab used last in that workspace.
7. Minimize gets a menu entry for the first time (today only `C-b _` and header clicks).

## Constraints

- Actions keep their current behaviour; their code is shared, not copied: the functions move
  out of `session_picker.sh` into a sourced file used by the sidebar and the menu.
- No new dependencies: fzf (already required) inside `tmux display-popup`.
- `C-b Space` replaces tmux's "next layout"; devws has its own layout toggle (`C-b \`).
- The existing keys (`C-b \`, `C-b _`, `C-b B`, `C-b F`) and header clicks keep working.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/workspace_actions.sh` | Create — the action functions, moved from session_picker.sh |
| `config/tmuxinator/session_picker.sh` | Modify — source the actions; one "☰ Menu" entry instead of the command list |
| `config/tmuxinator/devws_menu.sh` | Create — the tabbed fzf menu (lists tabs, runs the chosen action) |
| `config/tmux/tmux.conf` | Modify — `bind Space` opening the popup |
| `scripts/test_devws_menu.sh` | Create — tabs, items, dispatch, sidebar entry, key binding |
| `scripts/check.sh` | Modify — run the new test |
| `README.md` | Modify — document the menu; sidebar and shortcut sections |

## Success Criteria

- [x] Each tab lists exactly its actions, with shortcuts (tested).
- [x] Choosing each action runs the right shared function (tested with stubs).
- [x] `C-b Space` opens the popup; tabs switch by key and by click; Enter runs (checked in a
      real tmux client).
- [x] The sidebar shows the workspace list plus "☰ Menu", which opens the popup.
- [x] The last tab is remembered per workspace.
- [x] `./scripts/check.sh` passes.
