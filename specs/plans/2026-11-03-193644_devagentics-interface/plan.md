# Plan: The devws commands devAgentics drives

- **Date:** 03-11-2026
- **Branch:** feat/agent-commands
- **Status:** Pending

## Context

Implements `spec.md`: a stable `devws open` / `devws agent …` interface,
task grouping in the sidebar, and team panes left to devAgentics on restore.
Each step is test-first, using the existing suite pattern (a private tmux
socket, stub agents, a fake HOME; see `scripts/test_new_panel.sh`).

## Step 1 — Agent arguments file

**Files:** `config/tmuxinator/agent_runner.sh`, `scripts/test_agent_runner.sh` (or the closest existing suite)

**Actions:**
- `--args-file <file>`: read NUL-separated arguments (bash 3.2
  `read -d ''`, no `mapfile`), so one can be a prompt with line breaks. Append them after devws's own arguments for claude and
  codex, never with `--resume` (a team agent always starts fresh).
- Sanity check: a missing or unreadable file stops the runner with a message.

**Verification:**
- The stub claude and codex see devws's arguments followed by the file's
  lines, including an argument with spaces and an empty-line-free prompt.

## Step 2 — `agent_cli.sh state | send | mark | stop`

**Files:** `config/tmuxinator/agent_cli.sh` (new), `scripts/test_agent_cli.sh` (new)

**Actions:**
- `state <pane>`: `@devws_pane_state`, or `idle` when it's empty, or `gone`
  when `display-message -t <pane>` fails.
- `send <pane> <text>`: exit 4 when gone, 3 with `busy (<state>)` on working
  or waiting. Otherwise `load-buffer -` from stdin, `paste-buffer -p -d -t`,
  then `send-keys Enter`. A `--force` flag skips the busy check, for callers
  whose policy is "always" (memory.sh).
- `mark <pane> waiting|done`: `agent_status.sh <session> <state> <pane>`.
- `stop <pane>`: send the CLI's exit (`C-c` twice, then `kill-pane` after a
  short wait), then `agent_status.sh <session> idle <pane>`.

**Verification:**
- With a stub agent whose state is set through `agent_status.sh`:
  - send delivers one multi-line paste to a done pane;
  - send exits 3 on a working pane and 4 on a killed one;
  - mark waiting makes the session summary `waiting`;
  - stop removes the pane and clears the summary.

## Step 3 — Shared send for existing callers

**Files:** `git_actions.sh`, `memory.sh`, `suspend_workspace.sh`, their tests

**Actions:**
- Replace the duplicated `send-keys -l` + `Enter` pairs with
  `agent_cli.sh send`.
- The commit action keeps its workspace-level busy check; memory.sh uses
  `--force`; the suspend reopen keeps its current behaviour.

**Verification:**
- The existing git actions, memory and suspend suites pass unchanged, and
  Commit (agent) still refuses a busy workspace.

## Step 4 — `spawn` and `run`

**Files:** `config/tmuxinator/new_panel.sh`, `agent_cli.sh`, `scripts/test_new_panel.sh`, `test_agent_cli.sh`

**Actions:**
- `new_panel.sh` takes `--beside <pane>`, `--args-file <file>` and
  `--no-focus` (`split-window -d`); its current call stays the same.
- `agent_cli.sh spawn <session> <agent> [--beside] [--position] [--agent-args]`
  calls it, defaulting to right, and prints the pane id.
- `run <pane> <agent> [--agent-args]`: `respawn-pane -k -t <pane> -c
  <@devws_root>` with `agent_runner.sh <agent> --extra --args-file …`. It
  keeps the role options and resets the pane's state to idle.

**Verification:**
- Spawn beside the terminal leaves focus where it was and prints a pane id
  whose stub received the args.
- Run swaps the stub in place: same pane id, same layout string.

## Step 5 — `devws open` and `devws agent`, `limits`

**Files:** `bin/devws`, `config/tmuxinator/dev.yml`, `agent_cli.sh`, `scripts/test_devws_cli.sh` (or the closest existing suite)

**Actions:**
- `devws open [--no-attach] [--task id] [--agent-args file] <folder> [agent] [editor]`:
  - parse the flags, then reuse the existing folder path;
  - `--no-attach` means `tmuxinator start --no-attach` and no prompts (as when
    there's no TTY);
  - print `dev-<basename>`.
- `dev.yml`: `task` sets `@devws_task`; `agent_args` passes `--args-file` to
  the main agent's runner.
- `devws agent <sub> …` → `agent_cli.sh`.
- `devws worktree ensure <repo> <branch>`:
  - `owner/repo` or a URL: `github_repo.sh ensure`, `default-branch`, then
    `worktree.sh ensure <bare> <branch> origin/<default>`, then
    `shared_files.sh link`, the same as github_workspace.sh's `main`;
  - a folder: `worktree.sh ensure <folder> <branch>`.
  It prints the folder only. `devws worktree <branch> …` is unchanged.
- `limits <agent>`: load `.env`, then the same logic as `header_agents.sh`
  `limits_of` (moved into `agents.sh` or called from there, so it isn't
  duplicated).

**Verification:**
- `devws open --no-attach --task t1 <dir> claude nvim` with no TTY prints the
  session, which runs detached and has `@devws_task t1`. Its stub agent got
  the args.
- limits prints `<5h> <week>` from a set `@devws_limits_claude` and from a
  stub Codex sessions log.

## Step 6 — Sidebar grouping and restore

**Files:** `session_picker.sh`, `save_state.sh`, `restore_state.sh`, their tests

**Actions:**
- `workspace_rows()` also reads `#{@devws_task}`. It lists ungrouped
  workspaces as today, then each task's under a `▸ <id>` heading line, which
  isn't selectable.
- save: the `W` line gains a trailing task field (format version stays
  readable: the old 7 fields still load). Panes with `@devws_team 1` get a
  `team` marker.
- restore: set `@devws_task` and skip `team` panes.

**Verification:**
- Picker rows for two task sessions and one plain session show the heading
  and grouping.
- A save → restore round trip keeps the task and leaves team panes out. An
  old-format `sessions.tsv` still restores.

## Step 7 — Docs

**Files:** `docs/guide/commands.md`, `scripts/test_guide.sh`

**Actions:**
- "Agent commands": each command, its output and its exit codes, marked as
  the stable interface devAgentics uses; the sidebar's task headings in
  `workspaces.md`.

**Verification:**
- `test_guide.sh` passes with the new commands required.

## Final Verification

1. `scripts/check.sh` passes (full, not `--quick`).
2. Manual: `devws open --no-attach --task demo ~/Projects/devws claude nvim`
   from a plain terminal, then `devws agent spawn dev-devws codex`,
   `devws agent state`, `devws agent send` on a done agent, and the sidebar
   shows `▸ demo`.
3. Existing flows unchanged: start screen, New panel, Commit (agent), restore.
