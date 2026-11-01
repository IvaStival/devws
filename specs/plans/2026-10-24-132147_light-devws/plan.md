# Plan: Make devws light to run

- **Date:** 24-10-2026
- **Branch:** feat/suspend-idle
- **Status:** In progress
- **Spec:** [spec.md](spec.md)

## Context

You asked for devws to cost as little as possible while it runs. I measured
the live machine (6 workspaces, 1 attached, the devws agent working) before
planning, rather than guessing:

| What | Cost now | Why |
| ---- | -------- | --- |
| iTerm2 | ~63% of a core, 767 min CPU so far | draws every redraw tmux sends: header ghost at 20 fps, spinner 4/s, plus the agents' own UIs |
| tmux server | 4.6% steady | every ghost frame / spinner turn is a command plus a redraw of the 3-row header |
| 5 "frozen" workspaces | not frozen at all | see bug below; their agents, editors and sidebars all run |
| ESP32's idle Claude | 1.5–11%, 168 min CPU | its pane runs `claude` directly (no runner), so it can never be frozen |
| 6 sidebar fzf | ~1.7% each, ~10% together, visible or not | fzf's own idle cost (nothing redraws them: rows untouched for 5 min) |
| 7 Neovim servers | 5.7% together | would be frozen in idle workspaces if freezing worked |
| Wi-Fi row | 0.34 s CPU, 3.8 s wall, once a minute | `system_profiler SPAirPortDataType` |

**Bug found:** `scripts/test_devws_reload.sh` does not isolate `TMPDIR`. Its
`reload_devws.sh` runs `suspend_workspace.sh resume --all` against the *real*
frozen records, finds none of their sessions on the test's tmux server, so
treats them as closed: it continues your real frozen processes and deletes the
records, leaving `@devws_frozen=1`. The sweep skips flagged workspaces, so
they never freeze again, and the sidebar wrongly shows them paused. The record
folder was last touched at 09:31:13 today, during `make check`.

Your choices: pause animations while iTerm2 is not focused (ghost at 10 fps
otherwise); restart workspaces whose panes predate the runners, automatically.

Before starting: the clickable-alerts work is still uncommitted; commit it
first (after your click test) so this stays a separate change. The work is
saved as spec + plan under `specs/plans/<date>_light-devws/` (spec-plan
skill) when execution starts.

## Hard rule: running work is never frozen

Freezing only ever pauses things that are doing nothing. Kept as today, and
covered by tests:
- Terminal panes (the workspace's terminal and New panel terminals) are never
  frozen, whatever they run.
- A workspace whose agent is working or waiting is never frozen.
- A workspace where the agent or editor left a job running in a shell (a dev
  server from Claude's Bash tool, a `:terminal` in Neovim) is never frozen.

Added (Step 2): a workspace is also left alone when **any** process the
freeze would pause used CPU since the last sweep (e.g. a build or test run an
agent or editor started directly, without a shell). Only a workspace whose
every process is asleep gets frozen.

## Step 1 — A repeatable load measurement

- Add `scripts/measure_load.sh [seconds]`: CPU % over N seconds for the tmux
  server (`#{pid}`), the header ticker, sidebar shells and fzf, Neovim servers,
  agents, and iTerm2, plus each workspace's frozen flag vs. the stopped (`T`)
  state of its panes. Based on the scratchpad script used for the numbers above.
- Run it now and save the "before" numbers in the plan folder.

## Step 2 — Freezing that holds (bug fix, biggest win)

`config/tmuxinator/suspend_workspace.sh`, tests in `scripts/test_suspend_workspace.sh`:
- **Records per tmux server:** `frozen_dir` gains the server's pid
  (`tmux display-message -p '#{pid}'`), so a test server, or any other
  server, can never continue or delete another server's frozen processes.
- **Self-healing flag:** the sweep clears `@devws_frozen` on a workspace that
  has no record (and resume does too), so a stale flag can't block freezing.
- **Busy guard (hard rule above):** the sweep keeps each candidate's total CPU
  time (`ps -o time=` of the processes it would pause) from the previous
  sweep; if it grew by more than a small allowance (an idle agent's
  housekeeping, measured while implementing, ~1 s a minute), the workspace is
  busy and stays awake. Freezing waits for a sweep where it was quiet.
- Test isolation: `scripts/test_devws_reload.sh` (and the other suites that
  load tmux.conf's hooks without it: `test_agent_follow.sh`,
  `test_workspace_minimize.sh`, `test_workspace_database.sh`,
  `test_install_lifecycle.sh`) export `TMPDIR="$TEST_ROOT"`, like
  `test_suspend_workspace.sh` already does.
- Regression tests: a resume from another server leaves the records alone; a
  flag without a record is cleared and the workspace freezes on the next sweep;
  an idle-state agent whose child burns CPU (a direct `yes > /dev/null`-style
  stub, no shell) is not frozen; a terminal running a command is never
  stopped (already covered, kept).

## Step 3 — Restart panes that predate the runners

So that every idle workspace can freeze (ESP32 today):
- `agent_runner.sh <agent> [--extra] [--resume]`: `--resume` continues the
  last conversation in the folder (`claude --continue`; Codex's `resume --last`,
  checked against `codex --help` while implementing).
- In the sweep, before freezing an idle workspace (same rules: unseen for
  `DEVWS_SUSPEND_AFTER`, agent idle or done, no job running): a role
  `agent` pane with no devws script in its process tree is respawned as
  `agent_runner.sh <@devws_agent> --resume`; a role `editor` pane likewise as
  `editor_runner.sh <@devws_editor>` **only when it is verifiably clean** —
  its Neovim answers on `@devws_editor_socket` with no modified buffers.
  An editor that can't be checked (ESP32's has no socket) or has unsaved
  changes is left running, and the sidebar row says so (`· reopen to pause`).
- Tests (with stub agents and an `nvim` stub answering the modified-buffer
  query): an old-style idle agent is restarted under the runner with
  `--resume` and then frozen; a working one is not touched; a dirty or
  unreachable editor is not restarted.

## Step 4 — Animations pause while iTerm2 is not focused

`config/tmuxinator/header_ticker.sh`, `agent_spinner.sh`, tests in `scripts/test_header.sh` / `test_agent_status.sh`:
- Once a second (where `read_clients` already runs) note whether any client
  has `focused` in `#{client_flags}` (tmux 3.7b reports it; `focus-events` is
  already on in tmux.conf).
- Unfocused: no ghost frames, no spinner turns (state still shows: ✓, ●, the
  spinner's current frame). Focus back: both resume at once.
- Focused: ghost at 10 fps (a frame every other tick), spinner unchanged at
  4/s, clock/layout unchanged.
- Tests: with no focused client, ticks send no ghost or spinner commands; with
  one, they do; the ghost sends at most 10 frames a second.

## Step 5 — Lighter sidebars

`config/tmuxinator/session_picker.sh`:
- Experiment first, on a throwaway tmux server: idle CPU of fzf with today's
  flags vs. without `--height=100%` (full-screen renderer; same look in its
  own pane) and vs. without `--highlight-line` / `--gap`. Adopt the cheapest
  that renders the same; record the numbers.
- With Step 2, the sidebars of idle workspaces freeze too, so this applies
  to the visible ones only.
- Existing sidebar tests (`test_agent_status.sh`, `test_suspend_workspace.sh`,
  `test_devws_menu.sh`) must pass unchanged.

## Step 6 — A cheaper Wi-Fi row

`config/tmuxinator/header_network.sh`:
- Replace `system_profiler SPAirPortDataType` with a lighter source of the
  signal and band if one works on this macOS (e.g. `ipconfig getsummary
  <interface>`); keep `system_profiler` as the fallback when it doesn't give
  the signal. Keep the 60 s refresh.
- Test: the row is unchanged for a stubbed source's output.

## Verification

1. `make check` passes, run alone.
2. `scripts/measure_load.sh 30`, before vs. after, same situation (one
   workspace attached, others idle 15+ min): idle workspaces show `T`
   (stopped) everywhere, frozen flags match records, tmux server and iTerm2
   lower with iTerm2 focused, and near zero ghost/spinner traffic when
   another app is in front.
3. By hand: switch to a frozen workspace — it resumes at once; ESP32's agent
   comes back with its conversation after the restart; `C-b a` and the
   alerts still work.

## Critical files

- `config/tmuxinator/suspend_workspace.sh`, `agent_runner.sh`, `editor_runner.sh`
- `config/tmuxinator/header_ticker.sh`, `agent_spinner.sh`, `session_picker.sh`, `header_network.sh`
- `scripts/test_suspend_workspace.sh`, `test_devws_reload.sh`, `test_header.sh`, `test_agent_status.sh` (+ `TMPDIR` in the suites listed in Step 2)
- New: `scripts/measure_load.sh`

## Execution Notes

- **Step 1:** `scripts/measure_load.sh`; "before" numbers in `before.txt`
  (5 workspaces flagged frozen with 0 processes stopped; iTerm2 84.8%).
- **Step 2:** records per tmux server, stale marks heal, busy guard at 10%
  of a core (`busy_cpu`), orphaned records let go; `TMPDIR` isolated in
  `test_devws_reload.sh`, `test_agent_follow.sh`, `test_workspace_minimize.sh`,
  `test_workspace_database.sh` (`test_install_lifecycle.sh` only runs a fake
  tmux). The busy test was checked to fail with the guard off.
- **Step 3:** `agent_runner.sh --resume` (`claude --continue`,
  `codex resume --last`). Reopened panes get a fresh login shell with the
  runner typed in, as tmuxinator opens them. The editor is reopened only when
  its own Neovim (by pid) answers on the workspace socket with no modified
  buffers; otherwise the sidebar shows `· partly paused` / `· reopen to pause`.
  Added: a workspace frozen around an old-style pane is resumed (without
  counting as seen) so the pane can be reopened.
- **Step 4:** focus checked once a second; ghost on even ticks; the loop waits
  with a timed `read` instead of 20 `sleep` processes a second (bash 4+).
- **Step 5:** no code change. The sidebar's fzf options idle at 0.05% of a
  core, all variants alike; the live sidebars' ~2.3% is the redraw loop of
  the sidebar code from before the 24-10 merge, still running in memory
  (started 23-10 22:57). `devws reload` restarts them on the current code.
- **Step 6:** CoreWLAN through `osascript` (0.26 s, ~0.1 s CPU) before
  `system_profiler` (3.8 s, 0.34 s CPU).
