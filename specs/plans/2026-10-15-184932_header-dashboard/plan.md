# Plan: devws header dashboard

- **Date:** 15-10-2026
- **Branch:** feat/header-dashboard
- **Status:** Executed

## Context

The devws "header" is the tmux status bar at the top (`config/tmux/tmux.conf:256-330`, one line:
session/windows on the left, `sysstat` CPU | MEM on the right). The user wants more in it: token
usage of the registered agents (`DEVWS_AGENTS="claude codex"`), a clock, the weather, richer
system usage and a small animated ASCII pet, with the header made taller and the information laid
out in columns that spread across the screen width.

Decisions from the user:
- Tokens per agent: **today**, **this week**, and the **current conversation** ("session") of the
  focused workspace's agent.
- Weather: **auto-located from IP** (wttr.in), overridable in `.env`.
- Pet: a **ghost** that **reacts to the agent's state** (asleep idle, floats while working,
  alarmed when it needs you, happy when done).
- Layout: columns spread by screen width; height is my call → **3 lines**.

Following the repo workflow (spec-plan), execution starts by saving `spec.md` + `plan.md` under
`specs/plans/2026-10-15-<hhmmss>_header-dashboard/` on branch `feat/header-dashboard`.

## Layout (3 status lines, `set -g status 3`)

Each line uses tmux's `#[align=left]` / `#[align=centre]` / `#[align=right]` so the three columns
spread with the terminal width; the ghost is a fixed 9-column lane at the far right of all three
lines.

```
 [proj] ⠹  1:main  2:logs              Thu 15 Oct  14:32              ⛅ +21°C São Paulo   .-.
 CPU ▕███░░░▏ 34% ▂▃▅▂▁▃▇   MEM ▕████░░▏ 62% 10.1/16G   ⌂ 48%  ⚡ 87%  load 2.1      (o o)
 claude  today 1.2M · week 8.4M     codex  today 340k · week 2.1M     session 230k · ctx 41%   |\/\|
```

- **Line 0** keeps tmux's default `status-format[0]` (so `status-left` from `dev.yml`, the window
  list, its mouse ranges, prefix/zoom/keys-off indicators keep working); `status-right` becomes
  clock + weather and the clock goes in the centre.
- **Line 1** — system: CPU bar + 12-sample sparkline, MEM bar + used/total, disk `/` %, battery,
  load average. Colours reuse `color_level_ok/warn/stress`.
- **Line 2** — tokens: one column per registered agent (today · week), plus the current
  conversation of this workspace's agent (tokens + context fill %). Outside devws sessions the
  session column is empty.
- Below ~110 columns, line 1/2 drop the sparkline and the ghost (`#{e|<:#{client_width},110}`).
- `tmux.remote.conf` sets `status on` (back to one line) so nested remote sessions stay compact.

## Approach

One background **header ticker** collects everything and writes global tmux user options; the
status formats only read options (no `#()` shell-outs in the formats). Each concern is a small
script that prints, so it can be tested alone. Files are flat in `config/tmuxinator/` (install.sh
already links every file there into `~/.tmuxinator`).

1. **`header_ticker.sh`** (new) — scheduler, modelled on the spinner ticker in
   `agent_status.sh:25-42` (pid in an option, stops when tmux is gone).
   - `--start`: kills the ticker recorded in `@devws_header_ticker` (so `tmux source
     ~/.tmux.conf` picks up new code), then starts itself via `tmux run-shell -b`.
   - Loop every 0.5 s: pet frames; every 5 s: system; every 60 s: tokens (global + every `dev-*`
     session's conversation); every 30 min: weather. All option writes of a tick go in **one**
     tmux call chained with `\;`.
   - Runs collectors with `nice`; a collector that fails keeps the previous value.
   - Started from `tmux.conf`: `run-shell -b '~/.tmuxinator/header_ticker.sh --start'`.

2. **`header_system.sh`** (new, macOS; prints one formatted line) — CPU from `ps -A -o %cpu=` /
   `sysctl -n hw.ncpu`, memory from `vm_stat` + `hw.memsize` (active+wired+compressed), disk from
   `df -k /`, battery from `pmset -g batt`, load from `sysctl -n vm.loadavg`. Keeps the CPU
   history in a state file passed by the ticker. Missing sources (e.g. no battery) are omitted.
   Replaces `#{sysstat_cpu} | #{sysstat_mem}` locally; the `sysstat` plugin stays for the remote
   conf.

3. **`header_tokens.sh`** (new)
   - `header_tokens.sh totals` → `claude <today> <week>` / `codex <today> <week>` for each agent
     in `DEVWS_AGENTS` (read from `~/.tmuxinator/.env`).
   - `header_tokens.sh session <transcript>` → `<tokens> <context %>`.
   - Claude: `~/.claude/projects/*/*.jsonl`, assistant lines with `message.usage`, **deduplicated
     by `message.id`** (Claude Code writes a message's usage on several lines); total = input +
     output + cache creation + cache read (same definition as Codex's `total_tokens`, which
     includes cached input). Context = last message's input + cache tokens / model window.
   - Codex: `~/.codex/sessions/YYYY/MM/DD/*.jsonl` `token_count` events; per-period usage =
     deltas of `total_token_usage.total_tokens` attributed to each event's timestamp; context =
     `last_token_usage.input_tokens / model_context_window`.
   - "Today" = since local midnight; "week" = since Monday 00:00 local. Only files modified
     since the week start are read, prefiltered with `grep` before `jq` (logs are ~137 MB).
     Numbers humanised (`980`, `12k`, `1.2M`).

4. **Conversation link** — `agent_state.sh` (existing hook) also records the hook payload's
   `transcript_path` on the session as `@devws_agent_transcript` (both Claude Code and Codex send
   it; if Codex doesn't, its session column stays empty — checked during implementation). The
   ticker writes `@devws_agent_session_usage` per `dev-*` session, which line 2 reads.

5. **`header_weather.sh`** (new) — `curl -fsS --max-time 5 "https://wttr.in/${DEVWS_WEATHER_LOCATION}?format=%c%t+%l"`
   (empty location = IP auto-location), trims the place name to ~16 chars. On failure prints
   nothing and the ticker keeps the last value.

6. **`header_pet.sh <mood> <tick>`** (new, pure) — prints the ghost's 3 lines, each padded to the
   lane width so neighbouring columns never jitter. Moods:
   - `idle` (no agent state): eyes closed `(- -)`, slow `z Z` rising beside it.
   - `working`: eyes open, drifts left↔right within the lane, wavy tail alternates
     `\/\/` ↔ `/\/\`.
   - `waiting`: wide eyes `(O O)` + `!` blinking, yellow.
   - `done`: happy `(^ ^)`, little bounce, green.
   The ticker renders all four moods each tick into `@devws_pet_<mood>_<0|1|2>`; each status
   line picks one by `#{@devws_agent_state}`, so every workspace's ghost matches its own agent.

7. **`tmux.conf`** — `status 3`, `status-format[1]`/`[2]`, clock+weather in line 0, ticker start,
   remove local `sysstat` widgets; reuse `color_*` variables. `status-interval` stays 5 (the
   ticker's option writes already redraw, as for the spinner).

8. **`.env`** — `DEVWS_WEATHER_LOCATION=""` (empty = auto) and `DEVWS_HEADER_PET=1`.

9. **README** — header section: what each line shows, the `.env` knobs, that weather uses
   wttr.in, and how token numbers are counted.

## Critical files

| File | Action |
| --- | --- |
| `config/tmux/tmux.conf` | Modify — 3-line status, formats, ticker start |
| `config/tmux/tmux.remote.conf` | Modify — `status on` for remote |
| `config/tmuxinator/header_ticker.sh` | Create |
| `config/tmuxinator/header_system.sh` | Create |
| `config/tmuxinator/header_tokens.sh` | Create |
| `config/tmuxinator/header_weather.sh` | Create |
| `config/tmuxinator/header_pet.sh` | Create |
| `config/tmuxinator/agent_state.sh` | Modify — record `transcript_path` |
| `config/tmuxinator/.env` | Modify — weather/pet knobs |
| `scripts/test_header.sh` | Create |
| `scripts/test_agent_status.sh` | Modify — transcript recorded |
| `scripts/check.sh` | Modify — syntax + run new test |
| `README.md` | Modify |

Reused: spinner-ticker pattern (`agent_status.sh`), `color_level_*` vars (`tmux.conf:240-242`),
isolated-tmux-server test harness (`scripts/test_agent_status.sh:1-50`), `.env` agent list.

## Verification

TDD — `scripts/test_header.sh` written first, in the existing isolated-server style:
- tokens: fixture Claude transcripts (duplicate message ids, entries today / earlier this week /
  last week) and Codex rollouts (cumulative `token_count`) under a temp `HOME` → exact today/week
  totals and session tokens + context %.
- pet: every mood × several ticks prints exactly 3 lines of equal width; `working` frames move.
- system: stubbed `ps`, `vm_stat`, `sysctl`, `df`, `pmset` in `PATH` → expected bars/percentages;
  no battery → battery omitted.
- weather: stubbed `curl` success → value; failure → empty output, non-fatal.
- ticker: on an isolated server `--start` sets `@devws_pet_*`, `@devws_header_system`; a second
  `--start` replaces the first pid; killing the server ends the ticker.
- `test_agent_status.sh`: a hook payload with `transcript_path` sets `@devws_agent_transcript`.
- `scripts/check.sh` passes (also loads `tmux.conf` on a throwaway server to catch format errors).

Manual: `./install.sh`, `tmux source ~/.tmux.conf`, open a `dev-*` workspace — check the three
lines at full width and at ~100 columns, the ghost changing mood as Claude works / asks for
permission / stops, token numbers against a manual `jq` sum, and `top` showing the ticker's CPU
use stays negligible.

## Deviations during execution

- The clock sits beside the weather on the right of line 0, not centred: tmux's own line 0
  keeps the window list in the middle, and a centred clock would overlap it.
- Claude Code's logs don't name the context window, so its conversation shows `ctx` in tokens;
  Codex's shows a percentage.
- Lines 1 and 2 use left / right columns only; a centred column overlapped the left one below
  ~150 columns.
- The system lines are expanded with `#{E:}`, which also runs strftime, so they write `%%`.

## Redesign after review (same day)

The first version crowded everything against the edges. On review the user asked for groups
and for bars like their Claude Code status line:

- The header is five lines, the most tmux allows: tmux's own line (session, windows, this
  workspace's conversation, clock), a blank line, then three rows of groups.
- tmux aligns only left, centre or right, so `header_layout.sh` composes the rows at each
  client's width: groups sized to their content, the leftover shared between them with a `│`,
  no labels, the ghost at the right edge.
- A column per agent (`header_agents.sh`): tokens today/week, then "session" (5-hour) and
  "week" limits as bars of what is left. Codex's limits come from its logs; Claude Code's only
  from a status line, so devws starts it with `agent_statusline.sh`, which records them and
  then prints the user's own status line unchanged.
- Bars (`header_bar.sh`) use the user's status-line style: `▓░`, green / yellow / red.
- The ghost is a 6×5 pixel sprite drawn with half-blocks, so it can bob half a row at a time.
- No blank line above the panes: there's no sixth status line, and the user chose to keep it
  as is.
