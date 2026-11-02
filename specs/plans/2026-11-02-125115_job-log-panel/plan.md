# Plan: A log panel in the workspace that started a job

- **Date:** 02-11-2026
- **Branch:** feat/job-log-panel
- **Status:** Executed (not committed yet)

## Approach

### 1. Jobs remember their workspace — `config/tmuxinator/job.sh`
- `start` records the starting session in `jobs/<id>/session`
  (`tmux display-message -p '#S'` when inside tmux; works from the menu popup,
  which runs on the client's session). Nothing outside tmux.
- `run`: a watcher (backgrounded inside `run`) sleeps `DEVWS_JOB_PANEL_AFTER`
  (default 3 s); if the job still runs and its session exists, calls
  `job_panel.sh open <session> <id>`.
- `rows`: row 1 only (jobs, states, progress); rows 2–3 empty — the panel
  shows the log now. Keep `--span-last-two` in the ticker as is.
- `open [client] [id]` (header click, `C-b j`, notification click): opens the
  panel in the client's current workspace when it is a devws one; the popup
  stays only as the fallback (no workspace, e.g. a job from a plain shell).
- New `follow <id>`: the panel's program (below).

### 2. The panel — new `config/tmuxinator/job_panel.sh`
- `open <session> <id>`: if the session already has a pane with role
  `job-log`, `respawn-pane -k` it on the new job (one panel per workspace,
  the newest job); else `split-window -d -v -l 12` below the terminal pane
  (role `terminal`, found with workspace_panes.sh's `pane_with_role`); when
  the terminal is minimized (not in the window), `split-window -d -f -v -l 12`
  at the window's bottom. Mark it `@devws_role job-log`.
- `close <session>`: kills the session's `job-log` pane (used by layout
  toggle and minimize, below).
- `job.sh follow <id>` (runs in the pane): sets the pane title each second to
  `⏵ <title> <bar> <n/m> <elapsed>` (select-pane -T; the pane border header
  shows titles), streams the log with `tail -n +1 -F` (colors kept: the log is
  the command's raw output), reads keys with `read -t 1 -n 1` so `q` exits at
  any time. On end: success → title `✓ <title> · done`, wait
  `DEVWS_JOB_PANEL_LINGER` (default 5 s) or `q`, exit (the pane closes);
  failure → title `✗ <title> · failed — q closes`, wait for `q`, mark the job
  seen (as `log` does), exit.

### 3. Keep the rest of the workspace unaware of it
- `save_state.sh`: skip panes with role `job-log` (not restored after a
  reboot; their job is long gone).
- `toggle_layout.sh` and `minimize_panes.sh`: `job_panel.sh close` first —
  they move panes by role and would leave it misplaced; `C-b j` reopens it.
- `C-b j` / Job log menu item: unchanged entry points, now open the panel via
  `job.sh open`.

### 4. Docs and checks
- README "Background jobs": the panel replaces the log lines; how it opens and
  closes; `q`.
- `scripts/check.sh`: new `test_job_panel.sh`.

## Files
- Modify: `config/tmuxinator/job.sh`, `save_state.sh`, `toggle_layout.sh`,
  `minimize_panes.sh`, `README.md`, `scripts/check.sh`, `scripts/test_job.sh`
- Create: `config/tmuxinator/job_panel.sh`, `scripts/test_job_panel.sh`
- Reuse: `workspace_panes.sh` (`load_workspace_panes`, `pane_with_role`),
  `job.sh` `progress`/`elapsed`/`log_tail`, the spec-plan folder pattern
  (`specs/plans/<timestamp>_job-log-panel/`).

## Verification
- `scripts/test_job_panel.sh` on an isolated tmux server (as test_devws_menu.sh
  does), with `DEVWS_JOB_PANEL_AFTER=0.3`, `DEVWS_JOB_PANEL_LINGER=0.5`:
  - a 2 s job started from a workspace opens a `job-log` pane below the
    terminal, not focused; a 0.1 s job opens none;
  - success: the pane closes after the linger; failure: it stays until `q`
    (send-keys q), and the job is then seen;
  - a second job in the same workspace reuses the one pane;
  - `save_state.sh` output has no `job-log` row; `toggle_layout.sh` closes it.
- `scripts/test_job.sh`: rows now have only the jobs line; the session is
  recorded.
- `scripts/check.sh` passes.
- Manual: `devws job "Try" -- sh -c 'for i in $(seq 1 8); do echo step $i; sleep 1; done'`
  in a workspace → after 3 s a panel opens under the terminal while you keep
  typing; it closes 5 s after "✓ done"; `devws job "Fail" -- sh -c 'sleep 4; exit 1'`
  stays until `q`.
