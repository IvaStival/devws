# Agent team tabs: one tab per group, 4 agents per tab

## Context
In an agent team (`devws agentic`) every agent opens in the main window, under
the board. With a senior and a hammer the screen is already full (screenshot
17:41): the board is cut to half its height and each agent is a narrow column.
The user wants agents out of the main window, in tabs grouped by team group,
and reached only when needed:

- **Main tab** (the CTO): sidebar, leader, board at full height. Nothing else.
- **Seniors** tab, **Floor** tab: the agents of that group, at most 4 per tab,
  tiled; a 5th floor agent opens **Floor 2**, and so on.
- Reach an agent: from the board (a number per agent), by **peeking** (its
  panel joins the main window for a moment, then goes back), or from the
  **sidebar** (agents listed under the workspace).
- An agent that needs you marks its tab (`Seniors ●`) and its board row; no
  panel moves by itself.

## Design

### devws (bash): tabs are generic, devws knows nothing of groups
- **`new_panel.sh --tab <name>`** (with `devws agent spawn --tab <name>`),
  replacing `--place team` for teams. Finds a window of the session tagged
  `@devws_tab=<name>` with fewer than 4 panes, else makes one
  (`new-window -d`, named `<name>`, `<name> 2`, …). Splits it and applies
  `select-layout tiled`. Panes keep `@devws_team 1`; each team window is tagged
  `@devws_team_window 1`. `--place team` stays for old callers (main window).
- **New `team_tabs.sh`** with three verbs, shared by spawn, peek and the
  sidebar:
  - `place <pane> <tab>`: put an existing pane into its tab (join-pane into a
    tab window with room, or `break-pane -d` into a new one), re-tile;
  - `peek <pane>`: `join-pane` it under the board in the main window
    (`@devws_peeked_from=<tab>`); a second `peek` of it, or peeking another,
    sends it back with `place`;
  - `go <pane>`: `select-window` + `select-pane` (board numbers, sidebar).
  A tab whose last pane leaves closes (tmux does it); `place` makes it again.
- **The main window is named, not "the active one".** Today many scripts use
  `"$session:"` (the active window): `save_state.sh`, `workspace_panes.sh`
  (`load_workspace_panes`), `new_panel.sh`, `workspace_actions.sh`,
  `bin/devws` (line ~512), `restore_state.sh`. Visiting a tab would make them
  act on the tab. A helper `main_window <session>` (in `workspace_panes.sh`)
  returns the window tagged `@devws_main 1` (set by `agentic.yml`, and by
  `dev.yml` for every workspace), falling back to the first window; those
  scripts target it. Team tabs are not saved (restore already leaves a team to
  devAgentics).
- **Layout** `config/tmuxinator/agentic.yml`: sidebar | leader | board, the
  board at full height (no `anchor` split; `@devws_place anchor` goes).
  Layout string and checksum recomputed; `check.sh` expectation updated.
- **Tab marks** `config/tmux/tmux.conf`: `window-status-format` and
  `-current-format` add ` ●` when `@devws_tab_alert` is set; `agent_state.sh`
  (the hook that sets a pane's state) recomputes it for the pane's window
  (any pane `waiting` → `● `; any `working` → nothing extra). Hidden
  `devws-minimized` stays hidden.
- **Sidebar** `session_picker.sh`: under a workspace row, one row per team
  agent (`@devagentics_agent` panes of that session, all windows):
  `  ◦ ui-designer-1  ✓ done` / `● needs you`; Enter runs `team_tabs.sh go`.
- **Back to the main tab**: tmux's own `C-b l` (last window) and a click on
  `1:workspace` in the top bar; the board's keys list it.

### devAgentics (Rust)
- `devws.rs`: `spawn` passes `--tab <tab>` instead of `--place team`, where
  the tab is the group's: Seniors → `Seniors`, Floor → `Floor`
  (`Group::tab()`); the leader never spawns (it is the main tab). FakeDevws
  records it.
- `board.rs`/`board_view.rs`: each running agent gets a number (1–9) in
  "Who does what" (`1 ● ui-designer-1  done`), and the keys line shows
  `1-9 Go  p Peek  C-b l Back`.
- `board_tui.rs`: a digit → `devws agent go <pane>` (new devws verb wrapping
  `team_tabs.sh go`); `p` then a digit → `devws agent peek <pane>`; `p` with a
  peeked panel → send it back. The board shows which agent is peeked.
- Docs: `agentic/docs/usage.md`, `docs/guide/agentic.md` (screen drawing:
  main tab + Seniors/Floor tabs), `docs/guide/commands.md` (`--tab`, `go`,
  `peek`).

## Files
| Area | Files |
| ---- | ----- |
| devws tabs | `config/tmuxinator/new_panel.sh`, new `team_tabs.sh`, `agent_cli.sh` (`spawn --tab`, `go`, `peek`) |
| main window | `workspace_panes.sh` (`main_window`), `save_state.sh`, `workspace_actions.sh`, `restore_state.sh`, `bin/devws`, `dev.yml`, `agentic.yml` |
| marks, sidebar | `config/tmux/tmux.conf`, `agent_state.sh`, `session_picker.sh` |
| devAgentics | `agentic/src/devws.rs`, `roles.rs` (`Group::tab`), `board.rs`, `board_view.rs`, `board_tui.rs` |
| tests | `scripts/test_new_panel.sh`, new cases in `test_agentic.sh`, `test_agent_cli.sh`, `test_workspace_state.sh`, `test_picker.sh`/menu; `scripts/test_agentic_cross.sh`; Rust unit tests |

## Steps (TDD)
1. `main_window` helper and every `"$session:"` user switched to it; a test
   that saving while a tab is active still saves the main window.
2. `new_panel.sh --tab` + `team_tabs.sh place` (4 per tab, `Floor 2`, tiled,
   tab closes when emptied) — `test_new_panel.sh`.
3. `go` and `peek` (join under the board, back to its tab) — `test_agent_cli.sh`.
4. `agentic.yml` full-height board; `check.sh` layout check.
5. Tab alert mark from `agent_state.sh`; tmux.conf formats.
6. Sidebar agent rows and Enter — picker test.
7. devAgentics: spawn with the group's tab; board numbers, `go`/`peek` keys.
8. Docs; cross test: senior lands in `Seniors`, hammer in `Floor`, 5th floor
   agent in `Floor 2`, peek and back, save while on a tab.

## Verification
- `scripts/check.sh` (all suites, agentic Rust checks, cross test).
- Live: `devws agentic` on publisherProvider; the CTO starts ui-designer and
  hammer-codex → main tab keeps leader + full board; tabs `Seniors`, `Floor`
  appear; `1` on the board opens ui-designer, `C-b l` returns; `p 2` peeks the
  hammer under the board and `p` sends it back; a Codex approval prompt shows
  `Floor ●` and `● needs you` on the board and in the sidebar.
