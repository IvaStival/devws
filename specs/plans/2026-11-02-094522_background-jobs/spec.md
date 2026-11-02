# Spec: Background jobs in the header

- **Date:** 02-11-2026
- **Branch:** feat/background-jobs (off feat/jungleforge-runs)
- **Status:** Executed

## Context

The Run tab's actions (and Push) run inside the devws menu's tmux popup,
which is modal: while an app builds, nothing else can be used. Long work must
run in the background, and its progress must show somewhere that never blocks.
tmux 3.7's floating panes cannot be positioned from a script yet (tested:
always top-left, `select-layout` rejects a moved one), so the header —
already redrawn every second (header_ticker.sh) — shows it instead: while
jobs run, its network and weather groups give their place to a job group, and
come back afterwards.

Decisions come from a grilling session on 02-11-2026.

## Requirements

### Job runner
1. `devws job <title> [--lock <name>]... -- <command> [args]` (`job.sh`) runs
   the command detached from the caller (it survives the menu closing),
   output to a log, and returns at once. State per job in
   `~/.devws-state/jobs/<id>/`: title, state (queued / running / ok /
   failed), start and end times, exit code, log.
2. Jobs holding the same lock (a jungleforge app's name) run one after
   another; a waiting job shows as queued.
3. `devws note <text> [--error]` posts a message to the same place.
4. `devws jobs` lists recent jobs; `devws jobs log [id]` opens a log (the
   newest by default); jobs older than 7 days are pruned.

### Header
5. While a job is queued or running, or finished in the last 10 s, or a
   failure or error note is not yet seen, the header's network and weather
   groups are replaced by the job group:
   - row 1: each job with its state and time (`⏵ phi-planner 0:42`,
     `… phi-planner-web queued`, `✓ …`, `✗ … failed`);
   - rows 2–3: the newest running job's last two log lines (control
     sequences removed, cut to fit).
   A note shows its text on row 1 for 10 s.
6. A success leaves after 10 s; a failure (and an error note) stays until
   its log is opened or another job starts.
7. A click on the job group opens the newest relevant log (the failed one
   first) in a large popup that follows it (`less +F`); q closes it, which
   marks a failure as seen. A click elsewhere on the header still hides it.
8. tmux's own top line shows a compact, clickable summary in every state of
   the header: `⏵ 2 jobs`, `✗ 1 failed`.

### Actions as jobs
9. Run this branch, Switch, Stop, Services, Migrate (Run tab), the
   switch-back-to-main on Close, and Push run as jobs. Their questions (apps,
   branch, Push's "Open a PR after pushing? [Y/n/d]") are asked first; the
   menu then closes at once. jungleforge jobs lock each app they touch.
10. `devws run|stop|push` from a terminal run as jobs too.

## Constraints

- The header stays a once-a-second redraw from files: no polling added to
  the sidebar, no blocking calls in the ticker.
- Jobs never ask questions (stdin is /dev/null).
- Manual commit and agent commit stay as they are.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/job.sh` | Create — run, queue (locks), notes, list, log, prune; header rows |
| `config/tmuxinator/header_ticker.sh` | Modify — job group instead of network + weather while active |
| `config/tmux/tmux.conf` | Modify — job range click → log popup; compact summary on the top line |
| `config/tmuxinator/workspace_actions.sh` | Modify — Run tab, Close and Push submit jobs |
| `config/tmuxinator/git_actions.sh` | Modify — push asks about the PR first, then runs non-interactively |
| `shell/devws.zsh` | Modify — `devws job`, `devws note`, `devws jobs`; run/stop/push as jobs |
| `scripts/test_job.sh` | Create — tests |
| `scripts/test_header.sh`, `scripts/test_devws_menu.sh`, `scripts/test_git_actions.sh`, `scripts/check.sh`, `README.md` | Modify |

## Success Criteria

- [ ] A job returns at once, logs its output and records ok/failed with its exit code.
- [ ] Two jobs with the same lock run in order; the second shows queued meanwhile.
- [ ] The header rows show the job group while active and network/weather again 10 s after success; a failure stays until its log is opened.
- [ ] A click on the job group opens the log; elsewhere still hides the header.
- [ ] Run tab actions and Push return to the panes at once.
- [ ] `scripts/check.sh` passes.
