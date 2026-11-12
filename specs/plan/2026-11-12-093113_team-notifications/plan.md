# Plan: Team notifications

- **Date:** 12-11-2026
- **Branch:** fix/team-notifications
- **Status:** Executed
- **Authorization:** User approved the plan on 12-11-2026.

## Context

Implement [the specification](spec.md): no per-turn `done` alerts inside an
agent team, and one alert when the whole team stops.

## Step 1 — Silence team `done` alerts (tests first)

**Status:** Complete. Test failed first (a team agent's done alerted), then passed. The notifier moved into a new sourced `config/tmuxinator/alert.sh` (`notify_setting`, `post_alert`) rather than a function inside `agent_status.sh`, so `team_alert.sh` can source it.

**Files:** `scripts/test_agent_status.sh`, `config/tmuxinator/agent_status.sh`.

- Test: a workspace with `@devws_task` turning `done` posts nothing, and
  turning `waiting` still posts needs-you. A workspace without it still
  posts both.
- In `notify`, return early for `done` when the session has `@devws_task`.
- Move the notifier call (terminal-notifier / osascript /
  `DEVWS_NOTIFY_COMMAND`) into a small `post_alert` function that
  `team_alert.sh` can reuse with its own title, message and click target.

## Step 2 — Team stopped alert (tests first)

**Status:** Complete. Tests failed first (no script), then passed 4 runs in a row. The subtitle is the task's branch (`task.toml`); the alerted flag is set once per spell even when the alert is not posted (on screen or turned off).

**Files:** `scripts/test_team_alert.sh` (new), `config/tmuxinator/team_alert.sh`
(new), `config/tmuxinator/header_ticker.sh`.

- Tests, against a private tmux server and a fake task folder
  (`DEVAGENTICS_HOME`):
  - quiet for 20 s → one alert;
  - a later sweep → no repeat;
  - a working or waiting agent → no alert;
  - a queued order → no alert, unless the task is paused;
  - activity then quiet again → a second alert;
  - a client showing a team workspace → no alert;
  - `DEVWS_NOTIFY=waiting` → no alert;
  - the reason text: all steps accepted, paused, or idle.
- `team_alert.sh sweep`:
  1. Get each task from one `tmux list-panes -a` call: the session's
     `@devws_task` and each agent pane's `@devws_pane_state`.
  2. If any agent is working or waiting, the task is busy.
  3. Otherwise `jq` its `state.json` for the queue length and `paused`.
  4. Keep `@devws_team_quiet_since` and `@devws_team_alerted` on the leader's
     workspace (`task.toml`'s `workspace`).
  5. Alert through `agent_status.sh`'s `post_alert` once quiet for 20 s or
     more and not yet alerted. Clear both options when the task is busy again.
- Call it from the ticker's 5 s slot.

## Step 3 — Docs, checks and delivery

**Status:** Complete; live check pending (see Results).

- Update `docs/guide/agents.md`, `docs/guide/agentic.md` and
  `docs/guide/configuration.md`.
- Run `make check` and `git diff --check`, then restart the header ticker.
- Watch the live team: no alerts per order, one alert when it stops.
- Record results here.

## Results

- `scripts/test_agent_status.sh`: passed, with the new team test.
- `scripts/test_team_alert.sh`: 5 tests pass (quiet 20 s alerts once with reason and click target; busy agents; queue unless paused; second spell; on screen and `DEVWS_NOTIFY=waiting`).
- `make check`: all passed except `test_devws_menu` ("an agent team lost the job log"), already failing on `main`. The first run flagged `alert.sh` as not executable; fixed.
- `git diff --check` clean.
- Header ticker restarted. Live team `feat-system-observer`: a hammer working, so no quiet spell is recorded and no alert is sent.
- Not yet seen: the live team actually stopping and alerting.
- No commit made.
