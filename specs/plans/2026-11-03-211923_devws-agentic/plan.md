# Plan: `devws agentic`, a workspace for an agent team

- **Date:** 03-11-2026
- **Branch:** feat/agentic (off feat/agent-commands)
- **Status:** Pending

## Context

Implements `spec.md`. Each step is test-first in `scripts/test_agentic.sh`
(a private tmux server, stub `claude`/`codex`/`nvim`/`fzf`, a stub
`devagentics`, a fake HOME; the harness of `test_agent_cli.sh`), and keeps the
existing suites green. Steps 1–3 only need what exists; step 4 introduces the
template.

## Step 1 — Workspaces with no editor

**Files:** `minimize_panes.sh`, `toggle_layout.sh`, `open_database.sh`, `new_panel.sh`, `scripts/test_agentic.sh` (new)

**Actions:**
- Test first: in a workspace with picker, agent and terminal panes only (no
  editor), `minimize_panes.sh toggle agents`, `toggle_layout.sh` and
  `open_database.sh` exit 0, print "This workspace has no editor", and change
  nothing; `new_panel.sh … agent` opens its panel.
- Replace each `fail "editor pane not found"` with that message and `exit 0`;
  a workspace that has an editor but lost the pane keeps failing as before is
  not distinguishable, so the test pins the new behaviour.
- `new_panel.sh`: skip `minimize_panes.sh restore agents` when there is no
  editor pane (nothing can be minimized).

**Verification:**
- The new test, plus `test_workspace_minimize`, `test_workspace_layout`,
  `test_workspace_database` and `test_new_panel` pass.

## Step 2 — Role labels in the panel header

**Files:** `config/tmux/tmux.conf`, `agent_cli.sh`, `new_panel.sh`

**Actions:**
- Test first: a pane with `@devws_label` "Backend" renders "Backend ·
  claude" in `#{T:pane-border-format}` (evaluated against a server that
  sources `tmux.conf`, as the header tests already do); without it, "Agent".
- In `pane-border-format`, show `#{?#{@devws_label},#{@devws_label},Agent}`.
- `new_panel.sh --label <text>` and `agent_cli.sh spawn --label <text>` set
  the pane option `@devws_label` next to the role.

**Verification:**
- The label test; the existing header and agent-status suites pass.

## Step 3 — The team area (`spawn --place team`)

**Files:** `new_panel.sh`, `agent_cli.sh`

**Actions:**
- Test first, on a leader pane (full width) with a log pane under it: three
  `spawn --place team` calls leave the leader and the log in the left half
  (same column, log under leader), the first panel full-height in the right
  half, the next two splitting it in a grid; the focus is unchanged; every
  panel has `@devws_place team`.
- First team panel: `split-window -d -f -h -l 50%` (a full-height split of
  the window), so it spans the leader and the log. Next ones: the largest
  `team` panel is split `-h` when it is at least twice as wide as tall, else
  `-v`, `-d`, in its folder.
- `--place` takes `team` only; `--beside` and `--place` together are an error.

**Verification:**
- The grid test checks pane positions and sizes. `test_new_panel` and
  `test_agent_cli` pass.

## Step 4 — The template and `devws agentic`

**Files:** `config/tmuxinator/agentic.yml` (new), `config/tmuxinator/agentic.sh` (new), `bin/devws`, `scripts/test_agentic.sh`

**Actions:**
- Test first, with a stub `devagentics` (`task start <folder>` prints a task
  id, `claude` and an args-file path; `watch <id>` records its call):
  `devws agentic <folder>` (detached for the test) opens `dev-<name>` with
  `@devws_template agentic`, `@devws_task`, a leader pane running the stub
  agent with the file's arguments, a log pane running `watch <id>`, no
  editor; `owner/repo feat/x` makes the worktree first (a local remote, as
  `test_devws_cli`); with no `devagentics`, exit 1, nothing opened.
- `agentic.yml`: sidebar, leader (role `agent`) and under it the log pane
  (role `team-log`, ~8 lines). Capture the layout string from tmux once built
  (as `dev.yml` documents), and make the leader pane the focused one.
- `bin/devws`: `devws-workspace` takes `--template <name>` (starting
  `tmuxinator start <name>`), `--watch <id>` for the log pane; the pre-window
  command sets `@devws_template`. `devws agentic` calls `agentic.sh`.
- The log pane's header says `Log` (`tmux.conf`); `check.sh` renders
  `agentic.yml` as it does `dev.yml`.
- `agentic.sh`: no argument → `github_workspace.sh`; `owner/repo <branch>` →
  `devws worktree ensure`; a folder as it is; `--editor` adds a Neovim panel
  under the log after the workspace is up. Then `devagentics task start`
  (`DEVWS_AGENTIC_COMMAND`), then `devws-workspace --template agentic --task …
  --agent-args …`.

**Verification:**
- The new tests; `test_devws_cli` still passes; `devws agentic` in a real
  terminal opens the layout (manual).

## Step 5 — Save and restore

**Files:** `save_state.sh`, `restore_state.sh`, `scripts/test_workspace_state.sh`

**Actions:**
- Test first (the fake-tmux harness): a workspace with `@devws_template
  agentic` is saved with it, and restore opens none of it, printing `devws
  restore: <session> is an agent team: run devws agentic …`; a workspace
  without a template restores as before, and so does an older file.
- Save a `W` template field after the task; restore skips a `W` whose
  template is not empty and not `dev`, and its `P` lines.

**Verification:**
- `test_workspace_state` passes, within its usual time.

## Step 6 — Menu and guide

**Files:** `devws_menu.sh`, `docs/guide/agentic.md` (new), `docs/guide/commands.md`, `layout.md`, `workspaces.md`, `README.md`, `scripts/test_devws_menu.sh`

**Actions:**
- Test first: the Workspace tab has `◈ Open an agent team`; in an agentic
  workspace the Layout tab's Toggle layout, Minimize, Switch editor and the
  App tab's Database items are hidden (`shown()` condition `editor`: the
  workspace has an editor pane; cached like `jungleforge_available`), and
  shown in a normal one.
- The guide page and its Help item; the command in `commands.md`; one line in
  `layout.md` that an agent team has no editor panels. `test_guide.sh` enforces
  the labels and the new `agentic` subcommand.

**Verification:**
- `test_devws_menu` and `test_guide` pass.

## Final Verification

1. `scripts/check.sh` passes.
2. Manual: `devws agentic` from the start screen opens the layout (the leader
   focused, the log under it); with a stub `devagentics` on PATH,
   `devws agent spawn <session> codex --place team --label Backend` twice
   fills the right half in a grid with the labels in the headers; Close
   workspace and the sidebar group behave as for any task workspace.
3. Existing flows unchanged: the start screen, New panel, Commit (agent),
   restore of normal workspaces.
