# Plan: Background jobs in the header

- **Date:** 02-11-2026
- **Branch:** feat/background-jobs
- **Status:** Executed
- **Commit:** 3466ef2 (with the menu reorganization, the start screen and
  close/detach, made on the same branch)

Notes from execution:
- `header_layout.sh` now counts `##` (a literal `#`, as job rows escape the
  log's) as one column and never as a style: Docker logs are full of `#`.
- `job.sh header <width>` prints the summary and the rows in one call, and
  only the 20 newest jobs are read each second.
- A job inside tmux runs through `tmux run-shell -b` with `DEVWS_JOBS_DIR`
  passed along, so closing the menu popup cannot end it.
- `git_actions.sh` takes `DEVWS_ROOT` (a job has no workspace) and `ask-pr`
  (asks only when the branch has no PR yet).
- Close: when the worktree is removed and its apps move, one job switches the
  apps and then removes the folder, in that order.
- `jungleforge.sh run|stop` refresh the sidebars' ▶ marks when they finish.
- After a first try: the job group spans the network's and weather's room
  (`header_layout.sh --span-last-two`) instead of being a 4th group, so the
  clock, system and agents never move when jobs appear.

## Context

Implements `spec.md` in this folder. Order: the job runner with its own
tests (nothing else depends on how it runs), then the header showing it, then
the click and top-line summary, then moving the actions onto it.

## Step 1 — `job.sh`, the runner

**Files:** `config/tmuxinator/job.sh`, `scripts/test_job.sh`

**Actions:**
- `start <title> [--lock <name>]... -- <command>`: makes
  `~/.devws-state/jobs/<id>/` (title, locks, `state` = queued, `queued_at`),
  then detaches the runner: through `tmux run-shell -b` inside tmux (the
  tmux server owns it, so closing the menu popup cannot kill it), else
  `nohup … &`. Prints the id.
- `run <id>` (internal): waits for its locks (`mkdir` on
  `jobs/locks/<name>`, taken in name order, so two jobs never deadlock),
  `state` = running, runs the command with stdin `/dev/null` and output to
  `log`, records `exit` and `state` = ok / failed, releases the locks.
- `note <text> [--error]`: a finished job with no command.
- `list`, `log [id]` (`less +F`; marks a failure as seen), `prune` (older
  than 7 days; run by `start`).
- `rows <width>`: the job group's three rows, or nothing when no job is
  active (queued, running, finished < 10 s ago, or failed and unseen); log
  lines stripped of control sequences and cut to <width>.
- `summary`: `⏵ 2 jobs` / `✗ 1 failed` / nothing.

**Verification:** `scripts/test_job.sh`: returns at once; ok/failed with
the exit code; the log; two jobs on one lock run in order (the second queued
meanwhile); rows while running, for 10 s after success, until seen after a
failure; notes; prune.

## Step 2 — the header shows the job group

**Files:** `config/tmuxinator/header_ticker.sh`, `scripts/test_header.sh`

**Actions:**
- `lay_out`: `job.sh rows` each second; when it prints rows they replace the
  network and weather groups (4 groups laid out instead of 5), wrapped in a
  `devws-jobs` range that hands back to `devws-header` after it.
- `job.sh summary` into the global `@devws_jobs_summary`, only when changed.

**Verification:** `scripts/test_header.sh`: with an active job the rows
show it and not the weather; without, the weather is back.

## Step 3 — click and top line

**Files:** `config/tmux/tmux.conf`

**Actions:**
- `MouseDown1Status`: the `devws-jobs` range runs `job.sh open`, which opens
  the relevant log in a large popup; `devws-header` still hides the header.
- `status-right` shows `#{@devws_jobs_summary}` in a `devws-jobs` range.

**Verification:** `tmux source-file` loads; manual click test.

## Step 4 — Push without questions at the end

**Files:** `config/tmuxinator/git_actions.sh`, `scripts/test_git_actions.sh`

**Actions:** `push [--pr yes|no|draft]`: with `--pr` nothing is asked (a
job); without it, it asks first, before pushing.

**Verification:** `scripts/test_git_actions.sh` updated (PR answer before
the push; `--pr draft` asks nothing).

## Step 5 — actions become jobs

**Files:** `config/tmuxinator/workspace_actions.sh`, `shell/devws.zsh`,
`scripts/test_devws_menu.sh`

**Actions:**
- Run this branch / Switch / Stop / Services / Migrate: questions first, then
  `job.sh start "<verb> <apps>" --lock <app>... -- jungleforge.sh …`; no pause.
- Close: the switch-back / stop command becomes a job.
- Push: asks the PR question, then a job `git_actions.sh push --pr <answer>`.
- `devws job`, `devws note`, `devws jobs [log [id]]`; `devws run|stop|push`
  submit jobs.

**Verification:** `scripts/test_devws_menu.sh`: the actions call `job.sh
start` with the right title, locks and command, and return without pausing.

## Step 6 — docs and check

**Files:** `README.md`, `scripts/check.sh`

**Verification:** `scripts/check.sh` passes.

## Final Verification

1. `scripts/check.sh` passes.
2. Manual: Run this branch on PhiPlanner → the menu closes at once, the
   header shows `⏵ phi-planner` and its build lines where Wi-Fi and weather
   were, you keep typing in the terminal; a click opens the log; after it
   finishes, `✓` for 10 s and the weather is back. A failing job stays `✗`
   until its log is opened.
