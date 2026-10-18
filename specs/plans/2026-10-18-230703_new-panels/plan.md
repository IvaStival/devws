# Plan: New panels in a workspace — agents, terminals, Neovim

- **Date:** 18-10-2026
- **Branch:** feat/new-panels
- **Status:** Executed
- **Spec:** [spec.md](spec.md)

## Context

A devws workspace has one agent, one terminal and one editor; the menu's **New terminal** adds
shells beside the terminal. The user wants to add more panels of any kind: another agent (e.g.
a second account, or a second Claude on another task), a terminal, or a LazyVim. Today a second
agent can only be started by hand in a terminal, which devws doesn't see (no state, no
spinner, breaks "the agent pane" lookups — the dev-devws trouble of the last days).

Decisions (user):
- New panels open **in the agent group**, beside the terminal like today's extra terminals, so
  they minimize, move with the layout toggle and restore with the group.
- An extra agent **joins in fully**: started like the main one (devws hooks and status line),
  with its own working / needs you / done state in its pane header, counted for the ghost, the
  sidebar and **Go to waiting agent** (C-b a); its edits show in the main editor.
- An extra Neovim is **independent**: your LazyVim in the project folder, not linked to the
  agent (the agent keeps opening files in the main editor).
- Extra panels **come back after a restore**, relaunched (an agent starts a new conversation;
  `/resume` brings the old one back).

Workflow: spec-plan — on execution, save `spec.md` + `plan.md` under
`specs/plans/2026-10-18-<hhmmss>_new-panels/` and work on branch `feat/new-panels`.

## Approach

### 1. Roles
- Extra agent: `@devws_role extra-agent`, with a pane option `@devws_agent <name>`.
- Extra Neovim: `@devws_role extra-editor`. Extra shell: `extra` (unchanged).
- All three are the agent group's extras: `extra_panes()` (`config/tmuxinator/workspace_panes.sh`)
  matches `^extra(-agent|-editor)?$`. The main `agent` / `editor` roles stay unique, so
  `pane_with_role agent`, Restart / Switch agent, the follow link and the layout keep meaning
  the main ones.

### 2. Starting them
- `agent_runner.sh <agent> [--extra]` — `--extra` marks the pane `extra-agent` (and
  `@devws_agent`) instead of `agent`; everything else as for the main agent (account env,
  hooks, status line, editor prompt).
- Neovim: `nvim .` in the project root, role set by the action right after the split (as
  `new_terminal` does today).

### 3. One state per agent pane
- `agent_state.sh` (the hook) passes its `$TMUX_PANE` to `agent_status.sh`; it records the
  conversation's transcript only for the main agent, so tmux's own line keeps showing the
  main conversation.
- `agent_status.sh <session> <state> [pane]`: with a pane, sets that pane's
  `@devws_agent_state` (idle stored as `idle`, so its header never falls back to the
  workspace's), then sets the workspace's `@devws_agent_state` to the summary of all its agent
  panes (also those hidden while minimized): waiting, else working, else done, else unset; the
  `…_since` stamp and sidebar refresh as now. The spinner ticker stays one per workspace,
  running while the summary is `working`. Without a pane: as today (restart / switch / tests).
- tmux checks pane options before session options, so in `pane-border-format` each agent
  header already shows its own state. The format gains the `extra-agent` branch
  (`▾ <icon> Agent · <name>` + its state) and `extra-editor` (`▾  Neovim`); the minimized strip
  and `MouseUp1Border` click rule (`config/tmux/tmux.conf`) accept the new roles.
- `jump_agent.sh`: in the chosen workspace, focuses the agent pane in that state (waiting,
  else done), else the main agent.
- Sidebar, ghost, status line, reload: read the workspace summary — unchanged.

### 4. The action
- Menu: **▣ New terminal** becomes **▣ New panel** (Workspace tab) — fzf: *Agent*, *Terminal*,
  *Neovim*; *Agent* then asks which registered agent (`choose_agent`, `agents.sh`).
  Shortcut `C-b N`.
- `new_panel` in `workspace_actions.sh` (replaces `new_terminal`): expands a minimized group
  (`expand_minimized_panes`), splits beside the terminal (`terminal_pane`, `split-window -h`),
  starts the program, sets the role, `save_state`, `refresh_pickers`.

### 5. Minimize, layout, reload
- `minimize_panes.sh`: placeholders `minimized-extra-agent` / `minimized-extra-editor`
  (`group_placeholders` regex), labels `Agent · <name>` / `Neovim` (`placeholder_label`).
- `toggle_layout.sh`: extras already move beside the terminal — covered by `extra_panes`.
- `reload_devws.sh`: placeholders and spinners already handled generically.

### 6. Save and restore
- `save_state.sh`: `P` lines keep the `extra-agent` / `extra-editor` role and add a 6th field,
  the agent's name (older readers ignore it).
- `restore_state.sh`: pending extras become `role|cwd|agent`; restore splits beside the
  terminal as now, starting `agent_runner.sh <agent> --extra` (name checked with
  `valid_choice`, else the default agent) or `nvim .`, and sets the role.

### 7. README
"New panels" section: the menu item and C-b N, how extra agents show their state, that extra
Neovims are independent, that they are restored as new sessions.

## Files

| File | Change |
| --- | --- |
| `config/tmuxinator/workspace_actions.sh` | `new_panel` replaces `new_terminal` |
| `config/tmuxinator/devws_menu.sh` | New panel item |
| `config/tmuxinator/agent_runner.sh` | `--extra` |
| `config/tmuxinator/agent_state.sh`, `agent_status.sh` | per-pane state + workspace summary |
| `config/tmuxinator/jump_agent.sh` | focus the waiting pane |
| `config/tmuxinator/workspace_panes.sh` | extras include the new roles |
| `config/tmuxinator/minimize_panes.sh` | placeholders, labels |
| `config/tmuxinator/save_state.sh`, `restore_state.sh` | persist and relaunch |
| `config/tmux/tmux.conf` | header format, click rule, `C-b N` |
| `scripts/test_*.sh` (below), `README.md` | tests, docs |

## Verification (TDD, existing isolated-tmux harnesses)

- `test_agent_status.sh`: two agent panes in one workspace — main waiting + extra working →
  workspace waiting; each pane header shows its own state; extra idle + main done → done; all
  idle → unset; the transcript is the main agent's.
- `test_jump_agent.sh`: a waiting extra agent gets the focus.
- `test_workspace_minimize.sh`: extra agent and Neovim minimize with the group (labels) and
  come back with the same processes; a pane beside the editor still stays out.
- `test_workspace_layout.sh`: extras of each kind move beside the terminal both ways.
- `test_workspace_state.sh`: save → restore brings back an extra agent (stub agent, right
  name, `--extra`) and an extra Neovim (stub), with their roles.
- `test_devws_menu.sh`: Workspace tab lists `new-panel`; running it with a stubbed fzf choice
  starts each kind beside the terminal (pattern of `test_register_agent.sh`).
- `scripts/check.sh` passes; then in a real workspace: C-b N → Agent → second account; prompt
  it and watch its own spinner and the ghost; C-b _ hides and brings back all panels; C-b a
  jumps to it when it asks for permission; `devws reload` keeps it running.

## Deviations during execution

- The pane's own state is `@devws_pane_state` (with `@devws_pane_icon` / `@devws_pane_badge`),
  not `@devws_agent_state` on the pane: tmux renders the status bar in the focused pane's
  context too, so a pane-level `@devws_agent_state` leaked into the workspace's name on the
  status bar.
- `agent_status.sh` without a pane applies to the main agent pane; `devws reload` restarts the
  spinner with a new `--spin` mode, which leaves the agents' states alone.
- `new_panel.sh` (the work, testable) is split from the menu's `new_panel` (the questions),
  following `register_agent.sh`.
- A new panel splits the widest of the terminal and its panels, so their widths stay even
  (the old New terminal halved the terminal every time).
- Minimizing copies an extra agent's name onto its placeholder, so a workspace saved while
  minimized keeps it.
