#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="${TMPDIR:-/tmp}"
TEST_ROOT="$(mktemp -d "${tmp_dir%/}/devws-header.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-header-test-$$"
CLIENT_SOCKET="devws-header-client-$$"

cleanup() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server 2>/dev/null || true
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Header test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server, and
# the agents' logs live under a throwaway HOME.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home" "$TEST_ROOT/stubs"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
# The weather service: answers from a file, or fails when there is none.
cat > "$TEST_ROOT/bin/curl" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" > "$TEST_ROOT/stubs/curl.args"
[ -f "$TEST_ROOT/stubs/weather" ] || exit 6
cat "$TEST_ROOT/stubs/weather"
EOF
chmod +x "$TEST_ROOT/bin/"*
ln -s "$SCRIPTS" "$TEST_ROOT/home/.tmuxinator"
# TMPDIR keeps the ticker's CPU history apart from a real workspace's.
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT" LC_ALL=en_US.UTF-8
unset TMUX
SCRIPTS="$HOME/.tmuxinator"

wait_until() {
  local attempt
  for attempt in $(seq 1 40); do
    "$@" && return 0
    sleep 0.25
  done
  return 1
}

# Captured first: with pipefail, grep -q stopping early would fail the pipe.
shows() {
  local text="$1" pattern="$2"
  grep -q -- "$pattern" <<< "$text"
}

plain() {
  sed 's/#\[[^]]*\]//g'
}

# ---------------------------------------------------------------- the ghost

test_the_ghost_has_three_rows_of_one_width() {
  local mood tick output line count
  for mood in idle working waiting done; do
    for tick in 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14; do
      output="$("$SCRIPTS/header_pet.sh" "$mood" "$tick" | plain)"
      count=0
      while IFS= read -r line; do
        count=$((count + 1))
        [ "${#line}" -eq 12 ] || fail "$mood/$tick row $count is ${#line} wide: '$line'"
      done <<< "$output"
      [ "$count" -eq 3 ] || fail "$mood/$tick has $count rows"
    done
  done
}

# Two seconds of a mood, with their styles.
frames_of() {
  local mood="$1" tick frames=""
  for tick in $(seq 0 39); do
    frames+="$("$SCRIPTS/header_pet.sh" "$mood" "$tick")"$'\n'
  done
  printf '%s' "$frames"
}

distinct_frames() {
  local mood="$1" tick
  for tick in $(seq 0 23); do
    "$SCRIPTS/header_pet.sh" "$mood" "$tick" | tr '\n' '|'
    printf '\n'
  # Bytewise: a UTF-8 collation sorts the block glyphs as equal.
  done | LC_ALL=C sort -u | wc -l | tr -d ' '
}

# The eyes: white (colour255), the pupil (colour232) a quarter of them.
looking_right='#\[fg=colour232,bg=colour255\]▗'
looking_left='#\[fg=colour232,bg=colour255\]▖'

test_the_ghost_shows_its_mood() {
  local idle working waiting done
  idle="$(frames_of idle)"
  working="$(frames_of working)"
  waiting="$(frames_of waiting)"
  done="$(frames_of done)"
  shows "$idle" 'z' || fail "the idle ghost does not snore"
  ! shows "$idle" 'colour255' || fail "the idle ghost opens its eyes"
  shows "$idle" 'fg=colour245' || fail "the idle ghost is not grey"
  shows "$working" 'fg=colour173' || fail "the working ghost is not orange"
  shows "$working" "$looking_right" || fail "the working ghost does not look where it goes"
  shows "$("$SCRIPTS/header_pet.sh" working 20)" "$looking_left" || fail "the ghost does not look back on its way back"
  shows "$waiting" '!' || fail "the waiting ghost does not call you"
  shows "$(plain <<< "$waiting")" '▝▘' || fail "the waiting ghost does not gasp"
  shows "$done" '♥' || fail "the done ghost has no heart"
  ! shows "$done" "$looking_right\|$looking_left" || fail "the done ghost is not smiling"
  [ "$(distinct_frames working)" -ge 6 ] || fail "the working ghost barely moves"
  [ "$(distinct_frames idle)" -ge 2 ] || fail "the idle ghost does not move"
  ! "$SCRIPTS/header_pet.sh" sideways 0 2>/dev/null || fail "an unknown mood was accepted"
}

# Without a tick, the whole cycle: the same frames, one after the other, and
# the cycle loops back to its first frame.
test_the_ghost_cycle() {
  local mood cycle rows frames tick
  for mood in idle working waiting done; do
    cycle="$("$SCRIPTS/header_pet.sh" "$mood")"
    rows="$(wc -l <<< "$cycle" | tr -d ' ')"
    [ $((rows % 3)) -eq 0 ] || fail "the $mood cycle is not whole frames"
    frames=$((rows / 3))
    for tick in 0 1 $((frames - 1)); do
      [ "$(sed -n "$((tick * 3 + 1)),$((tick * 3 + 3))p" <<< "$cycle")" = "$("$SCRIPTS/header_pet.sh" "$mood" "$tick")" ] ||
        fail "the $mood cycle's frame $tick is not its tick's"
    done
    [ "$("$SCRIPTS/header_pet.sh" "$mood" "$frames")" = "$("$SCRIPTS/header_pet.sh" "$mood" 0)" ] ||
      fail "the $mood cycle does not loop"
  done
}

# ---------------------------------------------------------------- bars

test_the_bar() {
  local bar
  bar="$("$SCRIPTS/header_bar.sh" 30)"
  [ "$bar" = '#[fg=green]▓▓▓░░░░░░░#[fg=default]' ] || fail "unexpected 30% bar: $bar"
  bar="$("$SCRIPTS/header_bar.sh" 70)"
  [ "$bar" = '#[fg=yellow]▓▓▓▓▓▓▓░░░#[fg=default]' ] || fail "unexpected 70% bar: $bar"
  bar="$("$SCRIPTS/header_bar.sh" 90)"
  [ "$bar" = '#[fg=red]▓▓▓▓▓▓▓▓▓░#[fg=default]' ] || fail "unexpected 90% bar: $bar"
  # What is left of a limit 90% used: little bar, in the color of 90% used.
  bar="$("$SCRIPTS/header_bar.sh" 10 90)"
  [ "$bar" = '#[fg=red]▓░░░░░░░░░#[fg=default]' ] || fail "unexpected bar of what is left: $bar"
}

# ---------------------------------------------------------------- tokens

# A Wednesday noon: Monday and last week are both before it.
NOW="$(date -j -f '%Y-%m-%d %H:%M:%S' '2026-10-14 12:00:00' +%s)"
iso() {
  date -u -r "$1" +%Y-%m-%dT%H:%M:%S.000Z
}
TODAY="$(iso $((NOW - 3600)))"
MONDAY="$(iso $((NOW - 2 * 86400)))"
LAST_WEEK="$(iso $((NOW - 9 * 86400)))"

claude_message() {
  printf '{"type":"assistant","timestamp":"%s","message":{"id":"%s","model":"claude-opus-5-5","usage":{"input_tokens":%s,"output_tokens":%s,"cache_creation_input_tokens":%s,"cache_read_input_tokens":%s}}}\n' "$@"
}

# <time> <total> <last input> <5-hour % used> <5-hour reset> <week % used> <week reset>
codex_tokens() {
  printf '{"timestamp":"%s","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":%s},"last_token_usage":{"input_tokens":%s,"total_tokens":1},"model_context_window":258400},"rate_limits":{"primary":{"used_percent":%s,"window_minutes":300,"resets_at":%s},"secondary":{"used_percent":%s,"window_minutes":10080,"resets_at":%s}}}}\n' "$@"
}

CLAUDE_LOG="$HOME/.claude/projects/-project/one.jsonl"
CODEX_LOG="$HOME/.codex/sessions/2026/10/12/rollout-one.jsonl"
mkdir -p "$(dirname "$CLAUDE_LOG")" "$(dirname "$CODEX_LOG")"
{
  claude_message "$LAST_WEEK" msg_old 5000000 0 0 0
  claude_message "$MONDAY" msg_monday 2000000 0 0 0
  printf '{"type":"user","timestamp":"%s","message":{"content":"hi"}}\n' "$TODAY"
  printf 'not json, "output_tokens"\n'
  # Claude Code writes one message's usage on several lines: counted once.
  claude_message "$TODAY" msg_today 100 50 1000 10000
  claude_message "$TODAY" msg_today 100 50 1000 10000
} > "$CLAUDE_LOG"
# The real clock decides whether a limit has reset: one window resets in an
# hour, the other reset an hour ago.
LATER=$(($(date +%s) + 3600))
EARLIER=$(($(date +%s) - 3600))
{
  printf '{"timestamp":"%s","type":"session_meta","payload":{"id":"x"}}\n' "$LAST_WEEK"
  codex_tokens "$LAST_WEEK" 1000 1000 90 "$LATER" 90 "$LATER"
  codex_tokens "$MONDAY" 301000 300000 50 "$LATER" 50 "$LATER"
  printf '{"timestamp":"%s","type":"event_msg","payload":{"type":"token_count","info":null}}\n' "$TODAY"
  codex_tokens "$TODAY" 351000 51680 37.6 "$LATER" 12.2 "$EARLIER"
  codex_tokens "$TODAY" 351000 51680 37.6 "$LATER" 12.2 "$EARLIER"
} > "$CODEX_LOG"

test_tokens_today_and_this_week_per_agent() {
  local output
  output="$(DEVWS_TOKENS_NOW="$NOW" DEVWS_AGENTS="claude codex" "$SCRIPTS/header_tokens.sh" totals)"
  [ "$output" = $'claude 11k 2.0M\ncodex 50k 350k' ] || fail "unexpected totals: $output"
  output="$(DEVWS_TOKENS_NOW="$NOW" DEVWS_AGENTS="codex" "$SCRIPTS/header_tokens.sh" totals)"
  [ "$output" = "codex 50k 350k" ] || fail "an unregistered agent is shown: $output"
  output="$(HOME="$TEST_ROOT/empty" DEVWS_TOKENS_NOW="$NOW" DEVWS_AGENTS="claude" "$SCRIPTS/header_tokens.sh" totals)"
  [ "$output" = "claude 0 0" ] || fail "no logs is not zero: $output"
}

test_codex_limits() {
  local output
  output="$("$SCRIPTS/header_tokens.sh" limits codex)"
  [ "$output" = "37 0" ] || fail "unexpected Codex limits (the week has reset): $output"
  output="$(HOME="$TEST_ROOT/empty" "$SCRIPTS/header_tokens.sh" limits codex)" || fail "no Codex logs failed"
  [ -z "$output" ] || fail "no Codex logs printed limits: $output"
}

test_tokens_of_the_current_conversation() {
  local output
  output="$("$SCRIPTS/header_tokens.sh" session "$CLAUDE_LOG")"
  [ "$output" = "conv 7.0M · ctx 11k" ] || fail "unexpected Claude conversation: $output"
  output="$("$SCRIPTS/header_tokens.sh" session "$CODEX_LOG")"
  [ "$output" = "conv 351k · ctx 20%" ] || fail "unexpected Codex conversation: $output"
  output="$("$SCRIPTS/header_tokens.sh" session "$TEST_ROOT/missing.jsonl")" ||
    fail "a missing transcript failed"
  [ -z "$output" ] || fail "a missing transcript printed: $output"
}

# ---------------------------------------------------------------- agents

test_claude_limits_come_from_its_status_line() {
  local output
  tmux new-session -d -s dev-work -x 200 -y 50 'sleep 600'
  local payload='{"model":{"display_name":"Opus"},"rate_limits":{"five_hour":{"used_percentage":37.6},"seven_day":{"used_percentage":12.2}}}'
  # Without a status line of the user's own, it shows nothing.
  output="$(printf '%s' "$payload" | TMUX=test "$SCRIPTS/agent_statusline.sh")"
  [ -z "$output" ] || fail "the status line printed without one of the user's: $output"
  [ "$(tmux show-option -gqv @devws_claude_limits)" = "37 12" ] || fail "Claude's limits are not recorded"

  mkdir -p "$HOME/.claude"
  printf '{"statusLine":{"type":"command","command":"jq -r .model.display_name; printf mine"}}\n' > "$HOME/.claude/settings.json"
  output="$(printf '%s' '{"model":{"display_name":"Sonnet"}}' | TMUX=test "$SCRIPTS/agent_statusline.sh")"
  [ "$output" = $'Sonnet\nmine' ] || fail "the user's own status line is not shown: $output"
  [ "$(tmux show-option -gqv @devws_claude_limits)" = "37 12" ] ||
    fail "a status line without limits forgot them"
  rm "$HOME/.claude/settings.json"
  jq -e '.statusLine.command | contains("agent_statusline.sh")' "$SCRIPTS/agent_settings.json" >/dev/null ||
    fail "devws does not start Claude Code with its status line"
}

test_a_column_per_agent() {
  local output
  output="$(DEVWS_AGENTS="claude codex" "$SCRIPTS/header_agents.sh" | plain)"
  [ "$(wc -l <<< "$output" | tr -d ' ')" -eq 6 ] || fail "not three rows per agent: $output"
  shows "$(sed -n 1p <<< "$output")" '^claude  .* today · .* week$' || fail "unexpected Claude title: $output"
  [ "$(sed -n 2p <<< "$output")" = "session ▓▓▓▓▓▓░░░░  63% left" ] || fail "unexpected Claude session: $output"
  [ "$(sed -n 3p <<< "$output")" = "week    ▓▓▓▓▓▓▓▓▓░  88% left" ] || fail "unexpected Claude week: $output"
  shows "$(sed -n 4p <<< "$output")" '^codex   .* today · .* week$' || fail "unexpected Codex title: $output"
  [ "$(sed -n 5p <<< "$output")" = "session ▓▓▓▓▓▓░░░░  63% left" ] || fail "unexpected Codex session: $output"
  [ "$(sed -n 6p <<< "$output")" = "week    ▓▓▓▓▓▓▓▓▓▓ 100% left" ] || fail "unexpected Codex week: $output"

  tmux set-option -gu @devws_claude_limits
  output="$(DEVWS_AGENTS="claude" "$SCRIPTS/header_agents.sh" | plain)"
  [ "$(sed -n 2p <<< "$output")" = "session ░░░░░░░░░░    —" ] || fail "an unknown limit is not empty: $output"
  tmux kill-server
}

# ---------------------------------------------------------------- system

# The macOS sources, with fixed numbers: 4 CPUs at 80% in total, 8 GiB of
# memory, a disk 75% full and a battery at 71%.
stub_system() {
  local stubs="$TEST_ROOT/system-bin"
  mkdir -p "$stubs"
  printf '#!/usr/bin/env bash\nprintf "50.0\\n30.0\\n"\n' > "$stubs/ps"
  cat > "$stubs/sysctl" <<'STUB'
#!/usr/bin/env bash
case "$2" in
  hw.ncpu) echo 4 ;;
  hw.memsize) echo 8589934592 ;;
  vm.loadavg) echo '{ 4.42 4.22 7.38 }' ;;
esac
STUB
  cat > "$stubs/vm_stat" <<'STUB'
#!/usr/bin/env bash
cat <<'OUT'
Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                                     4200.
Pages active:                                  67230.
Pages wired down:                             185502.
Pages occupied by compressor:                 164919.
OUT
STUB
  cat > "$stubs/df" <<'STUB'
#!/usr/bin/env bash
printf 'Filesystem 1024-blocks Used Available Capacity Mounted\n/dev/disk3s1 1000 700 250 74%% /\n'
STUB
  cat > "$stubs/pmset" <<STUB
#!/usr/bin/env bash
[ -f "$TEST_ROOT/stubs/no-battery" ] && { echo "Now drawing from 'AC Power'"; exit 0; }
printf "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t71%%; discharging; 4:34 remaining present: true\n"
STUB
  chmod +x "$stubs/"*
  printf '%s' "$stubs"
}

test_system_usage() {
  local stubs output history="$TEST_ROOT/cpu.history"
  stubs="$(stub_system)"
  PATH="$stubs:$PATH" "$SCRIPTS/header_system.sh" "$history" >/dev/null
  PATH="$stubs:$PATH" "$SCRIPTS/header_system.sh" "$history" >/dev/null
  output="$(PATH="$stubs:$PATH" "$SCRIPTS/header_system.sh" "$history" | plain)"
  [ "$output" = $'CPU ▓▓░░░░░░░░  20%  ▂▂▂\nMEM ▓▓▓▓▓▓▓▓░░  79%  6.4/8.0G\nDSK ▓▓▓▓▓▓▓▓░░  75%  BAT 71%  LOAD 4.42' ] ||
    fail "unexpected system rows: $output"

  touch "$TEST_ROOT/stubs/no-battery"
  output="$(PATH="$stubs:$PATH" "$SCRIPTS/header_system.sh" "$history")"
  ! shows "$output" BAT || fail "a machine without a battery shows one: $output"
  rm "$TEST_ROOT/stubs/no-battery"

  for sample in 1 2 3 4 5 6 7 8 9 10 11 12; do
    PATH="$stubs:$PATH" "$SCRIPTS/header_system.sh" "$history" >/dev/null
  done
  [ "$(wc -w < "$history" | tr -d ' ')" -eq 12 ] || fail "the CPU history keeps more than 12 samples"
}

# ---------------------------------------------------------------- weather

test_weather() {
  local output
  printf '+21°C|Partly cloudy|São Paulo, Brazil|70%%|↙12km/h\n' > "$TEST_ROOT/stubs/weather"
  output="$(DEVWS_WEATHER_LOCATION="" "$SCRIPTS/header_weather.sh")"
  [ "$output" = $'+21°C  Partly cloudy\nSão Paulo\nhumidity 70% · wind ↙12km/h' ] ||
    fail "unexpected weather: $output"
  grep -q '^https://wttr.in/?format=' "$TEST_ROOT/stubs/curl.args" || fail "the weather is not located by IP"

  DEVWS_WEATHER_LOCATION="Lisbon" "$SCRIPTS/header_weather.sh" >/dev/null
  grep -q '^https://wttr.in/Lisbon?format=' "$TEST_ROOT/stubs/curl.args" || fail "the location setting is ignored"

  printf 'Unknown location; please try ~-23.5,-46.6\n' > "$TEST_ROOT/stubs/weather"
  output="$("$SCRIPTS/header_weather.sh")" || fail "an unknown location failed"
  [ -z "$output" ] || fail "an unknown location printed: $output"

  rm "$TEST_ROOT/stubs/weather"
  output="$("$SCRIPTS/header_weather.sh")" || fail "being offline failed"
  [ -z "$output" ] || fail "being offline printed: $output"
}

# ---------------------------------------------------------------- layout

# Rows as the ticker passes them: the system, two agents, the weather.
layout_rows() {
  printf '%s\n' \
    'CPU #[fg=green]▓▓▓#[fg=default]░░░░░░░  27%' 'MEM ▓▓▓▓▓▓▓▓░░  80%' 'DSK ▓▓▓▓▓▓▓▓▓▓  98%' \
    '#[bold]claude #[nobold] 31.6M today · 458.6M week' 'session ▓▓▓▓▓▓░░░░  62% left' 'week    ▓▓▓▓▓▓▓▓▓░  88% left' \
    'codex   0 today · 278k week' 'session ▓▓▓▓▓▓▓▓▓▓ 100% left' 'week    ▓▓▓▓▓▓▓▓▓▓  99% left' \
    '+18°C  Light rain' 'Curitiba' 'humidity 94%'
}

test_the_layout_spreads_the_groups() {
  local width output line
  for width in 238 160 120 100 60; do
    output="$(layout_rows | "$SCRIPTS/header_layout.sh" "$width" 4 | plain)"
    [ "$(wc -l <<< "$output" | tr -d ' ')" -eq 3 ] || fail "the layout at $width is not three rows"
    while IFS= read -r line; do
      [ "${#line}" -eq "$width" ] || fail "a row at $width is ${#line} wide: '$line'"
      [ "$(tr -cd '│' <<< "$line" | wc -m | tr -d ' ')" -eq 3 ] ||
        fail "a row at $width does not separate four groups: '$line'"
    done <<< "$output"
  done

  output="$(layout_rows | "$SCRIPTS/header_layout.sh" 238 4 | plain)"
  line="$(sed -n 1p <<< "$output")"
  case "$line" in
    " CPU ▓▓▓░░░░░░░  27%"*"│"*"claude  31.6M today · 458.6M week"*"│"*"codex   0 today · 278k week"*"│"*"+18°C  Light rain"*) ;;
    *) fail "unexpected first row: $line" ;;
  esac
  ! shows "$output" 'SYSTEM\|AGENTS\|WEATHER' || fail "the groups still have labels"
  # Spread: wide gaps between the groups, the last one ending at the edge.
  shows "$line" '27%       *│       *claude' || fail "the groups are not spread: $line"
  shows "$line" 'Light rain $' || fail "the last group does not end at the edge"
  # Narrow, the widest groups give way first.
  output="$(layout_rows | "$SCRIPTS/header_layout.sh" 100 4 | plain)"
  shows "$output" 'Curitiba' || fail "a narrow header cut a short group"
}

# ---------------------------------------------------------------- tmux

option() {
  tmux show-option -gqv "$1"
}

session_option() {
  tmux show-option -qv -t "=$1:" "$2"
}

ticker_pid() {
  option @devws_header_ticker
}

ticker_replaced() {
  [ -n "$(ticker_pid)" ] && [ "$(ticker_pid)" != "$1" ]
}

process_gone() {
  ! kill -0 "$1" 2>/dev/null
}

# The header as a real client draws it: the client runs in a pane of a second
# server, whose capture shows the client's screen.
client_header() {
  local screen
  screen="$("$REAL_TMUX" -L "$CLIENT_SOCKET" capture-pane -p -t client)"
  sed -n 1,5p <<< "$screen"
}

header_shows() {
  shows "$(client_header)" "$1"
}

header_row_shows() {
  shows "$(client_header | sed -n "$1p")" "$2"
}

test_a_client_shows_the_header() {
  printf '+25°C|Sunny|Porto, Portugal|60%%|↑8km/h\n' > "$TEST_ROOT/stubs/weather"
  tmux new-session -d -s dev-work -x 200 -y 50 'sleep 600'
  # tmux.conf's status settings and the colors and widgets they use.
  grep -E '^(color_[a-z_]+=|wg_[a-z0-9_]+=|set -g (status|status-[a-z]+|status-format\[[0-9]\]|@devws_[a-z_]+) )' \
    "$ROOT/config/tmux/tmux.conf" > "$TEST_ROOT/status.conf"
  tmux source-file "$TEST_ROOT/status.conf"
  [ "$(option status)" = 5 ] || fail "the header is not five lines"
  tmux set-option -g @devws_claude_limits "40 10"
  tmux set-option -t dev-work @devws_agent_state working
  tmux set-option -t dev-work @devws_agent_transcript "$CODEX_LOG"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t dev-work"
  "$SCRIPTS/header_ticker.sh" --start

  wait_until header_row_shows 3 'CPU ▓' || fail "the client shows no system: $(client_header)"
  wait_until header_row_shows 3 '│ *claude .*│ *codex .*│ *+25°C  Sunny' ||
    fail "the client shows no agent columns or weather: $(client_header)"
  wait_until header_row_shows 4 'session ▓▓▓▓▓▓░░░░  60% left' ||
    fail "the client shows no Claude limits: $(client_header)"
  # tmux's own line, then a blank line before the groups.
  header_row_shows 1 'dev-work' || fail "tmux's own line moved: $(client_header)"
  [ -z "$(client_header | sed -n 2p | tr -d ' ')" ] || fail "no blank line under tmux's own line: $(client_header)"
  header_row_shows 5 'week    ▓' || fail "the groups' last row moved: $(client_header)"
  wait_until header_row_shows 1 'conv 351k · ctx 20%' ||
    fail "the client shows no conversation: $(client_header)"
  header_shows '█' || fail "the client shows no ghost: $(client_header)"
  shows "$(option @devws_header_row_1)" 'CPU' || fail "no global rows stand in for new sessions"
  # The ghost moves 10 times a second: frames a few tenths apart differ.
  local before
  before="$(session_option dev-work @devws_ghost_2)$(session_option dev-work @devws_ghost_3)"
  ghost_moved() {
    [ "$(session_option dev-work @devws_ghost_2)$(session_option dev-work @devws_ghost_3)" != "$before" ]
  }
  wait_until ghost_moved || fail "the ghost does not move"
  shows "$(session_option dev-work @devws_ghost_2)" 'colour173' || fail "the working ghost is not orange"

  local first
  first="$(ticker_pid)"
  "$SCRIPTS/header_ticker.sh" --start
  wait_until ticker_replaced "$first" || fail "a restart kept the old ticker"
  wait_until process_gone "$first" || fail "the old ticker still runs"

  local second
  second="$(ticker_pid)"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server
  tmux kill-server
  wait_until process_gone "$second" || fail "the ticker outlived the tmux server"
}

test_the_ghost_has_three_rows_of_one_width
test_the_ghost_shows_its_mood
test_the_ghost_cycle
test_the_bar
test_tokens_today_and_this_week_per_agent
test_codex_limits
test_tokens_of_the_current_conversation
test_claude_limits_come_from_its_status_line
test_a_column_per_agent
test_system_usage
test_weather
test_the_layout_spreads_the_groups
test_a_client_shows_the_header

printf 'Header tests passed.\n'
