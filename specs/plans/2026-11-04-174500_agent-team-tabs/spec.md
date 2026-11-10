# Spec: Agent team tabs

- **Date:** 04-11-2026
- **Branch:** feat/agentic
- **Status:** Executed (uncommitted)

## Context

In an agent team every agent opened in the workspace's window, under the
board; with two agents the board was cut in half and each agent was a narrow
column. Agents move to tabs of the workspace, by group, and come back only
when needed.

## Requirements

1. The workspace's window keeps the sidebar, the leader and the full-height
   board.
2. Agents open in tabs named by group (Seniors, Floor), four to a tab, tiled;
   the next go to "<name> 2".
3. The board numbers the running agents: a number opens its tab; `p` and the
   number peeks it under the board, and again sends it back.
4. The sidebar lists a team's agents under their workspace; Enter opens one.
5. A tab with an agent waiting for you shows a ● in the tab list.
6. Saving and the menu act on the workspace's own window (marked
   `@devws_main`), whatever tab is on screen.

## Success Criteria

- [x] `test_new_panel.sh`: four to a tab, Floor 2, peek and back, go.
- [x] `test_agent_status.sh`: the tab mark follows the waiting agent.
- [x] `test_agent_cli.sh`: the sidebar's agent rows and cursor position.
- [x] `test_workspace_state.sh`: the save reads the workspace's window.
- [x] Cross test with real devws and devagentics.
- [ ] Live look in the user's tmux.
