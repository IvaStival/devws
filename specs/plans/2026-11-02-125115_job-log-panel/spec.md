# Spec: A log panel in the workspace that started a job

- **Date:** 02-11-2026
- **Branch:** feat/job-log-panel (off feat/ux-round)
- **Status:** Executed

## Context

Background jobs show in the header as a jobs line plus the newest job's last
two log lines, squeezed into the network and weather columns: too little to
follow a build. The log should show in the workspace that started the job,
readable, without blocking anything. tmux 3.7's floating panes cannot be
positioned from a script, so it is a docked panel. Decisions from 02-11-2026.

## Requirements

1. A job remembers the workspace it was started from.
2. Once such a job has run 3 s, a panel (~12 lines) opens below that
   workspace's terminal — at the window's bottom when the terminal is
   minimized — without taking focus. One panel per workspace, on its newest
   job.
3. Its header shows the job, its progress and time
   (`⏵ phi-planner → feat/login ▓▓▓▓░░░░░░ 4/9 0:42`); its body the live log
   with colors.
4. Success: `✓ … · done`, closes after 5 s. Failure: `✗ … · failed — q
   closes`, stays until `q`, which marks the job seen. `q` closes it any time;
   the job carries on.
5. `C-b j`, the Job log item and a click on the header's jobs open the panel
   in the current workspace (a popup only outside a workspace).
6. The header's job group keeps only its jobs line.
7. The panel is never saved or restored; layout toggle and minimize close it.

## Constraints

- No focus change, no blocking; jobs keep running whatever the panel does.

## Success Criteria

- [ ] A 2 s job from a workspace opens an unfocused panel below its terminal; a 0.1 s job none.
- [ ] Success closes it after the linger; failure keeps it until q, then the job is seen.
- [ ] A second job reuses the panel; save_state skips it; toggle_layout closes it.
- [ ] `scripts/check.sh` passes.
