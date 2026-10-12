# Plan: Switchable workspace layout

- **Date:** 11-10-2026
- **Branch:** feat/switchable-workspace-layout
- **Status:** Executed

## Context

Today every devws workspace opens with the picker sidebar on the left, lvim on
top, and agent + terminal side by side underneath (`config/tmuxinator/dev.yml`).
You want a second arrangement — lvim full height in the middle, agent on top
and terminal below it in a right column — and a way to flip between the two at
any time. Decisions: new workspaces keep the current layout by default; the
switch lives in the picker sidebar (plus a tmux shortcut).

```
current (default)            new "side" layout
┌──┬───────────────┐         ┌──┬─────────┬─────┐
│P │     lvim      │         │P │         │agent│
│i ├───────┬───────┤   ⇄     │i │  lvim   ├─────┤
│k │ agent │ term  │         │k │         │term │
└──┴───────┴───────┘         └──┴─────────┴─────┘
```

## Approach

Rearrange the panes that already exist (no restarts — lvim, the agent and the
shell keep running) using `tmux join-pane`, located by their `@devws_role`
option. A fixed layout string can't be used, because pane counts vary (picker
may be closed, extra terminals may exist).

## Steps

1. **New script `config/tmuxinator/toggle_layout.sh`** (auto-linked into
   `~/.tmuxinator/` by the existing glob in `install.sh:197`)
   - Find editor / agent / terminal pane ids in the current session by
     `@devws_role` (title fallback for terminal, same rule as
     `terminal_pane()` in `session_picker.sh`). Missing pane → `tmux
     display-message` with a clear reason and exit non-zero.
   - Detect the current arrangement: agent's `pane_top` equals editor's
     `pane_top` → "side", otherwise "stacked".
   - To side: `join-pane -d -h -l 35% -s agent -t editor`, then
     `join-pane -d -v -l 50% -s terminal -t agent`.
   - To stacked: `join-pane -d -v -l 50% -s agent -t editor`, then
     `join-pane -d -h -l 50% -s terminal -t agent`.
   - Extra terminals (`@devws_role extra`) are re-joined next to the terminal
     pane so they move with it.
   - `-d` keeps keyboard focus where it was. Finish by running
     `save_state.sh` so a restore brings back the chosen layout (the saved
     `window_layout` string already captures it — no state-format change).

2. **Picker action** (`config/tmuxinator/session_picker.sh`)
   - Add `▥ Toggle layout` to the WORKSPACE COMMANDS header (after
     "New terminal"), shift the later `click-header` line numbers by one, and
     dispatch `toggle-layout` → `~/.tmuxinator/toggle_layout.sh`.

3. **Shortcut** (`config/tmux/tmux.conf`, next to the other pane bindings)
   - `bind '\' run-shell -b "~/.tmuxinator/toggle_layout.sh"` → `C-b \`.

4. **Tests** — new `scripts/test_workspace_layout.sh`, run against a real,
   isolated tmux server (`tmux -L <tmp socket> -f /dev/null`, reached via a
   PATH shim) with four role-tagged panes in the default layout:
   - toggle once → editor spans full height; agent above terminal, both right
     of the editor.
   - toggle again → back to editor on top, agent left of terminal below.
   - picker closed and one extra terminal present → still toggles both ways.
   - missing agent pane → exits non-zero, panes untouched.
   - Register it in `scripts/check.sh` (syntax check, executable check, run).

5. **Docs** — README: one line on the toggle (picker action and `C-b \`), and
   list the new script under the workspace files. Save spec/plan under
   `specs/plans/<timestamp>_switchable-workspace-layout/` per the spec-plan
   workflow.

## Files

| File | Change |
| ---- | ------ |
| `config/tmuxinator/toggle_layout.sh` | Create |
| `config/tmuxinator/session_picker.sh` | Add header action + dispatch |
| `config/tmux/tmux.conf` | Add `C-b \` binding |
| `scripts/test_workspace_layout.sh` | Create |
| `scripts/check.sh` | Register new script + test |
| `README.md` | Document the toggle |

Not changed: `dev.yml` default layout, save/restore format, lvim config.

## Verification

- `./scripts/check.sh` passes (includes the new layout test).
- Manual: `tmux source ~/.tmux.conf`, open a workspace, click "Toggle layout"
  in the picker and press `C-b \`; confirm panes swap arrangement, lvim/agent
  keep running, and focus doesn't jump. Restart tmux and confirm the restored
  workspace keeps the last layout.
