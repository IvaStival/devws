# Spec: Agent alerts only when something needs you or the work stopped

- **Date:** 12-11-2026
- **Branch:** fix/team-notifications
- **Status:** Executed

## Context

`agent_status.sh` posts a macOS alert whenever a workspace no client shows
turns `waiting` ("needs you") or `done`. In an agent team, every senior and
hammer turns `done` after each order, between one order and the next, so a
running team sends an alert for nearly every order. The user wants alerts only
when someone needs something, or when the work stops or finishes.

## Requirements

1. **Needs you** stays as today, for every agent, team or not.
2. A single agent outside a team keeps its **done** alert when its turn ends.
3. In an agent team (workspaces carrying `@devws_task`), no agent's own
   `done` alerts, the leader's included.
4. A team posts one **stopped** alert when it has gone quiet: no agent in any
   of its workspaces (leader, seniors, floor) is working or waiting, and no
   order is queued (a paused task's queue does not count, since nothing will
   be delivered). The team must stay quiet for 20 s, so the gap between two
   orders does not fire it.
5. The alert fires once per quiet spell. It reads
   `<task> ✓ team stopped`, says why when known (all steps accepted, paused,
   or idle), and a click goes to the leader's workspace.
6. No alert while a client shows any of the team's workspaces, as today.
   `DEVWS_NOTIFY=waiting` or `0` turn it off, as for `done`.

## Constraints

- No new background process: the check runs in the header ticker's 5 s slot.
- Read the task's `task.toml` (workspace) and `state.json` (queue, paused,
  steps) from `${DEVAGENTICS_HOME:-~/.devagentics}/tasks/<task>/`; no
  `devagentics` call every 5 s.
- Job alerts (`job.sh`) are unchanged.

## Files involved

| File | Expected change |
| --- | --- |
| `config/tmuxinator/agent_status.sh`, `alert.sh` (**new**) | Skip `done` alerts for a team's workspaces; the notifier moves to `alert.sh`, shared with the team alert |
| `config/tmuxinator/team_alert.sh` | **New**: `sweep` — per task, track quiet since / alerted, post the stopped alert |
| `config/tmuxinator/header_ticker.sh` | Run the sweep every 5 s |
| `scripts/test_agent_status.sh` | Team `done` is silent; needs-you still alerts |
| `scripts/test_team_alert.sh` | **New**: quiet 20 s alerts once; busy or queued doesn't; paused queue ignored; resets after activity; on screen silent; `DEVWS_NOTIFY` respected |
| `docs/guide/agents.md`, `docs/guide/agentic.md`, `docs/guide/configuration.md` | Describe the alerts |

## Success criteria

- [x] A team agent finishing an order posts no alert; a team agent needing you does.
- [x] A team that stays quiet 20 s posts one `team stopped` alert; activity then quiet again posts another.
- [x] A queued order keeps the team from counting as quiet, unless the task is paused.
- [x] Solo agents alert as before.
- [x] `make check` and `git diff --check` pass (except the known `test_devws_menu` failure on `main`).
