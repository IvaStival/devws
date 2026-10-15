# Spec: devws header dashboard

- **Date:** 15-10-2026
- **Branch:** feat/header-dashboard
- **Status:** Executed

## Context

The tmux status bar at the top of devws is a single line: the session, the windows and the
`sysstat` CPU / MEM widgets. The user wants it to be a small dashboard: the token usage of the
registered agents, a clock, the weather, richer system usage and an animated ASCII pet. The header
may grow taller, with the information in columns spread across the screen width.

## Requirements

1. The header has 3 lines; each line has left / centre / right columns that spread with the
   terminal width.
2. Line 0 keeps the session, the agent icon and the window list (with their mouse actions), and
   adds a clock and the weather.
3. Line 1 shows CPU (bar, %, recent history), memory (bar, %, used/total), disk use of `/`,
   battery and load average. Missing sources (no battery) are left out.
4. Line 2 shows, for every agent in `DEVWS_AGENTS`, the tokens used today (since local midnight)
   and this week (since Monday 00:00 local), plus the tokens of the current conversation of the
   workspace's agent and how full its context is.
5. Weather comes from wttr.in, located by IP unless `DEVWS_WEATHER_LOCATION` is set in `.env`;
   it refreshes every 30 minutes and keeps the last value when offline.
6. A 3-line ASCII ghost lives at the right of the header. It reacts to the workspace's agent:
   asleep while idle, floating while working, alarmed when the agent needs the user, happy when
   it is done. `DEVWS_HEADER_PET=0` hides it.
7. Narrow terminals (< 110 columns) drop the CPU history and the ghost.
8. Nested remote sessions (`tmux.remote.conf`) keep a one-line status bar.

## Constraints

- No shell-outs in the status formats: one background ticker collects the data and writes tmux
  options; it is restarted when tmux.conf is reloaded and stops when the server is gone.
- Token logs are large (~140 MB); the ticker reads only files modified this week, and refreshes
  tokens once a minute at low priority.
- Token totals count input, output and cache tokens, deduplicating Claude Code's repeated
  message lines.

## Files Involved

| File | Action |
| --- | --- |
| `config/tmux/tmux.conf` | Modify — 3-line status, formats, ticker start |
| `config/tmux/tmux.remote.conf` | Modify — one-line status for remote |
| `config/tmuxinator/header_ticker.sh` | Create — the scheduler |
| `config/tmuxinator/header_system.sh` | Create — system line |
| `config/tmuxinator/header_tokens.sh` | Create — agent token usage |
| `config/tmuxinator/header_weather.sh` | Create — weather |
| `config/tmuxinator/header_pet.sh` | Create — ghost frames |
| `config/tmuxinator/agent_state.sh` | Modify — record the conversation's transcript |
| `config/tmuxinator/.env` | Modify — weather / pet settings |
| `scripts/test_header.sh` | Create |
| `scripts/test_agent_status.sh` | Modify |
| `scripts/check.sh` | Modify |
| `README.md` | Modify |

## Success Criteria

- `scripts/check.sh` passes, including the new header test.
- In a real workspace the three lines render at full width and at ~100 columns, the ghost
  changes mood with the agent, and token numbers match a manual `jq` sum.
