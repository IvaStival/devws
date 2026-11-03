# Spec: Make devws light to run

- **Date:** 24-10-2026
- **Branch:** feat/suspend-idle
- **Status:** Specified (approved with the plan)

## Context

devws should cost as little as possible while it runs. Measured on the live
machine (see [plan.md](plan.md) for the numbers): iTerm2 draws ~63% of a core,
the tmux server takes 4.6%, idle workspaces that should be frozen are not
(a test leaked into the real frozen records), an old-style workspace's agent
can never be frozen, and every sidebar's fzf idles at ~1.7%.

## Requirements

1. **Running work is never frozen** (hard rule): terminal panes never; a
   workspace whose agent is working or waiting, runs a shell job, or whose
   would-be-frozen processes used CPU since the last sweep, never.
2. Idle workspaces really freeze, and stay frozen until you return: another
   tmux server (a test) can't touch their records, and a stale flag heals.
3. Workspaces whose agent pane predates the runners get their agent restarted
   under the runner, its conversation resumed, so they can freeze; their
   editor only when it has nothing unsaved and can be checked.
4. The ghost and the spinner pause while no client is focused; the ghost runs
   at 10 fps otherwise.
5. Sidebars idle more cheaply, if an fzf setting that renders the same does.
6. The Wi-Fi row uses a lighter source than `system_profiler` when one works.
7. A script measures devws's load, before and after.

## Constraints

- No change to what a workspace shows; no new `.env` settings.
- Nothing restarts while it is in use: restarts follow the freeze rules.
- An editor with unsaved changes, or that can't be checked, is never
  restarted.

## Files Involved

See "Critical files" in [plan.md](plan.md).

## Success Criteria

- [ ] `make check` passes, run alone.
- [ ] After 15+ minutes idle, every process devws freezes in an idle
      workspace is stopped (`T`), and flags match records.
- [ ] A busy agent child (no shell) keeps its workspace awake; terminals are
      never stopped.
- [ ] With another app in front, the header ticker sends no ghost or spinner
      updates.
- [ ] `scripts/measure_load.sh` shows lower tmux server and iTerm2 load than
      the recorded "before".
