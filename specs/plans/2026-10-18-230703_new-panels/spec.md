# Spec: New panels in a workspace — agents, terminals, Neovim

- **Date:** 18-10-2026
- **Branch:** feat/new-panels
- **Status:** Executed

## Context

A workspace has one agent, one terminal and one editor, and the menu's New terminal adds shells
beside the terminal. The user wants to add more panels of any kind: another agent (a second
account, or a second Claude on another task), a terminal, or a LazyVim. A second agent started
by hand in a terminal is invisible to devws: no state, no spinner, and it confuses the "agent
pane" lookups.

## Requirements

1. **New panel** (devws menu, Workspace tab, and `C-b N`) asks for *Agent*, *Terminal* or
   *Neovim*; *Agent* then asks which registered agent.
2. The panel opens in the agent group, beside the terminal: it minimizes with the group, moves
   with the layout toggle and comes back with the group. A minimized group is expanded first.
3. An extra agent is started as the main one is (account, hooks, status line, editor prompt). It
   has its own working / needs you / done state in its own pane header; the workspace's state
   (sidebar, status line, ghost, Go to waiting agent) sums up all its agents: waiting, else
   working, else done, else none.
4. Go to waiting agent focuses the agent pane that is waiting (else done), not only the main one.
5. An extra Neovim is the user's LazyVim in the project folder, not linked to the agent.
6. Extra panels are saved and come back after a restore, relaunched: the same agent (a new
   conversation) or a fresh Neovim.
7. The main agent and editor keep their meaning: Restart / Switch agent, the follow link, the
   conversation on tmux's line and the layout all refer to them.

## Constraints

- Reuse: `agent_runner.sh`, `agent_status.sh`'s ticker, `extra_panes`, `minimize_panes.sh`,
  `save_state.sh` / `restore_state.sh`, `choose_agent`.
- Workspaces and state files written before this keep working (no 6th field = shell extra).
- Every behavior change gets a test in the existing isolated-tmux harnesses.

## Files Involved

See [plan.md](plan.md).

## Success Criteria

- [x] New panel adds each kind beside the terminal, with its role.
- [x] Two agents in a workspace show their own states; the workspace shows the summary.
- [x] Go to waiting agent focuses the waiting extra agent.
- [x] Minimize, layout toggle and restore carry the new panels; a pane beside the editor stays out.
- [x] Save → restore relaunches an extra agent (right name) and an extra Neovim.
- [x] `scripts/check.sh` passes; checked in a real workspace.
