# Plan: Tabbed devws menu

- **Date:** 14-10-2026
- **Branch:** feat/devws-menu
- **Status:** Executed

## Context

Implements `spec.md`: a `C-b Space` popup with four tabs of devws actions, sharing the action
code with the sidebar, whose command list becomes one "☰ Menu" entry.

## Step 1 — Tests first

**Files:**
- `scripts/test_devws_menu.sh` (new), `scripts/check.sh`

**Actions:**
- Harness as in `test_agent_follow.sh`: isolated tmux server, fake `HOME/.tmuxinator` with
  logging stubs for `toggle_layout.sh`, `minimize_panes.sh`, `open_database.sh`,
  `follow_mode.sh`.
- Cases:
  - `devws_menu.sh --items <tab>` lists exactly the spec's actions and shortcuts per tab.
  - `--switch next|prev|<tab name>` stores the tab in `@devws_menu_tab` (wrapping around)
    and prints fzf's `reload(...)+change-header(...)`; the header marks the current tab.
  - The menu starts on the remembered tab.
  - `--run <action>` for script-backed actions calls the right script with the session;
    every action id maps to a function defined in `workspace_actions.sh`.
  - The sidebar header has "☰ Menu" and no longer the old command lines.
  - `tmux.conf` binds `Space` to the menu.
- Register the test in `check.sh` (its three lists).

**Verification:**
- The new test fails for the right reasons (missing scripts).

## Step 2 — Shared actions

**Files:**
- `config/tmuxinator/workspace_actions.sh` (new), `config/tmuxinator/session_picker.sh`

**Actions:**
- Move the action functions (`switch_agent` … `close_workspace`, with their helpers
  `choose_agent`, `agent_pane`, `refresh_pickers`, `save_state`, `dev_sessions`…) into
  `workspace_actions.sh`, unchanged, plus a new `minimize_agents` (`minimize_panes.sh
  <session> toggle agents`).
- `session_picker.sh` sources it and keeps only the sidebar (rows, fzf loop).

**Verification:**
- Existing tests (`test_workspace_*.sh`) still pass; `bash -n` both files.

## Step 3 — The menu

**Files:**
- `config/tmuxinator/devws_menu.sh` (new)

**Actions:**
- One table of `tab|action|label|shortcut` rows drives everything.
- Modes:
  - no argument: runs fzf (`--header` = tab bar, list = current tab's items, binds:
    `right`/`tab` → `--switch next`, `left`/`btab` → `--switch prev`, `click-header` →
    `--switch "$FZF_CLICK_HEADER_WORD"`, `left-click` → accept, `esc` → abort), then runs the
    chosen action through `--run`.
  - `--items`, `--header`, `--switch`, `--run`: as in Step 1.
  - `--popup [client]`: `tmux display-popup -E` with fixed size and title, running the menu.
- Tab state in the session option `@devws_menu_tab`.

**Verification:**
- Step 1 tests pass.

## Step 4 — Key and sidebar entry

**Files:**
- `config/tmux/tmux.conf`, `config/tmuxinator/session_picker.sh`

**Actions:**
- `bind Space run-shell -b "~/.tmuxinator/devws_menu.sh --popup '#{client_name}'"` in the
  devws bindings section.
- Sidebar header: "☰ Menu   C-b Space" replaces the 12 command lines; clicking or choosing it
  opens the popup; click-line numbers simplified.

**Verification:**
- Tests pass; in a real tmux client (pty), `C-b Space` shows the popup, `→` and a tab click
  switch tabs, `Enter` runs an action (e.g. Toggle layout changes the layout).

## Step 5 — Docs

**Files:**
- `README.md`

**Actions:**
- New "Workspace menu" section with the tabs; sidebar description updated; `C-b Space` in the
  shortcuts table; Configuration map lists `devws_menu.sh` and `workspace_actions.sh`.

## Final Verification

1. `./scripts/check.sh` passes (syntax, all tests).
2. `./install.sh` links the new scripts; reload tmux; in a real workspace open the menu with
   `C-b Space` and from the sidebar, switch tabs by key and click, run Toggle layout and
   Agent follow.
3. Commit with the commit skill (no AI credits).

## Notes from execution

- fzf splits header "words" on `--delimiter` (a tab here), so a click's word was the whole tab
  bar; tabs are found from the click column instead (`--switch-at`).
- The sidebar no longer runs actions itself, so it does not source `workspace_actions.sh`;
  `.env` (agents, editors) is loaded by `workspace_actions.sh`.
- Checked in a real tmux client driven through a pty and the pyte terminal emulator:
  `C-b Space` opened the popup; `→`, `Tab` and clicks on tabs switched tabs; Enter and a click
  on an item ran the action; the last tab was remembered.
