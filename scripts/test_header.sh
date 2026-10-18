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
# Every call that updates the header's rows or ghost is logged, so a test can
# count the redraws.
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
case "\$*" in
  *@devws_ghost_*) echo ghost >> "$TEST_ROOT/redraws.log" ;;
  *@devws_header_row_*) echo rows >> "$TEST_ROOT/redraws.log" ;;
esac
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
# The weather service: answers from a file (weather.json for the JSON
# format), or fails when there is none.
cat > "$TEST_ROOT/bin/curl" <<EOF
#!/usr/bin/env bash
answer="$TEST_ROOT/stubs/weather"
case "\$*" in
  *format=j1*) answer="$TEST_ROOT/stubs/weather.json"; printf '%s\n' "\$@" > "$TEST_ROOT/stubs/curl.json.args" ;;
  *) printf '%s\n' "\$@" > "$TEST_ROOT/stubs/curl.args" ;;
esac
[ -f "\$answer" ] || exit 6
cat "\$answer"
EOF
chmod +x "$TEST_ROOT/bin/"*
ln -s "$SCRIPTS" "$TEST_ROOT/home/.tmuxinator"
# TMPDIR keeps the ticker's CPU history apart from a real workspace's.
# The ticker's settings: fixed here, not the user's own .env.
printf 'DEVWS_AGENTS="claude codex"\n' > "$TEST_ROOT/devws.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT" LC_ALL=en_US.UTF-8 \
  DEVWS_ENV_FILE="$TEST_ROOT/devws.env"
# Run from a devws agent pane, the account it runs in would leak in.
unset TMUX CLAUDE_CONFIG_DIR CODEX_HOME DEVWS_AGENT
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
  [ "$("$SCRIPTS/header_bar.sh" 40 40 5 | plain)" = '▓▓░░░' ] || fail "a bar of 5 cells is not 5 wide"
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

# Costs: Claude messages priced by model, writes split by cache lifetime, last
# month's left out, an unknown model flagged; Codex responses priced by their
# turn's model, cached input at the cache-read price.
LAST_MONTH="$(iso $((NOW - 24 * 86400)))"
{
  printf '{"type":"assistant","timestamp":"%s","message":{"id":"msg_september","model":"claude-opus-5-5","usage":{"input_tokens":1000000,"output_tokens":0}}}\n' "$LAST_MONTH"
  printf '{"type":"assistant","timestamp":"%s","message":{"id":"msg_split","model":"claude-sonnet-5","usage":{"input_tokens":0,"output_tokens":1000000,"cache_creation_input_tokens":2000000,"cache_read_input_tokens":1000000,"cache_creation":{"ephemeral_5m_input_tokens":1000000,"ephemeral_1h_input_tokens":1000000}}}}\n' "$TODAY"
  printf '{"type":"assistant","timestamp":"%s","message":{"id":"msg_mystery","model":"claude-mystery-9","usage":{"input_tokens":1000000,"output_tokens":0}}}\n' "$TODAY"
} > "$HOME/.claude/projects/-project/two.jsonl"
{
  printf '{"timestamp":"%s","type":"turn_context","payload":{"turn_id":"turn-sol","model":"gpt-5.6-sol"}}\n' "$TODAY"
  printf '{"timestamp":"%s","type":"token_usage_record","payload":{"turn_id":"turn-sol","usage":{"input_tokens":1000000,"cached_input_tokens":500000,"cache_write_input_tokens":0,"output_tokens":100000,"total_tokens":1100000}}}\n' "$TODAY"
} >> "$CODEX_LOG"

# Claude: $20 + $8 (Opus 5.5 input) + $0.0084 (cached message) + $16.70
# (Sonnet 5: 5 min and 1 h writes, a read, output); Codex: $4.20 (Sol: fresh
# and cached input, output).
test_month_costs_per_agent() {
  local output
  output="$(DEVWS_TOKENS_NOW="$NOW" DEVWS_AGENTS="claude codex" "$SCRIPTS/header_tokens.sh" costs)"
  [ "$output" = $'claude 44.71 1\ncodex 4.2 0' ] || fail "unexpected costs: $output"
  output="$(DEVWS_TOKENS_NOW="$NOW" DEVWS_AGENTS="codex" "$SCRIPTS/header_tokens.sh" costs)"
  [ "$output" = "codex 4.2 0" ] || fail "an unregistered agent is shown: $output"
  output="$(HOME="$TEST_ROOT/empty" DEVWS_TOKENS_NOW="$NOW" DEVWS_AGENTS="claude codex" "$SCRIPTS/header_tokens.sh" costs)"
  [ "$output" = $'claude 0 0\ncodex 0 0' ] || fail "no logs is not zero: $output"
}

# Two accounts of one kind: each is priced from its own folder's logs. Work:
# $4 (1M Opus 5.5 input tokens).
test_month_costs_per_account() {
  local output
  mkdir -p "$HOME/.claude-work/projects/-project"
  printf '{"type":"assistant","timestamp":"%s","message":{"id":"msg_work","model":"claude-opus-5-5","usage":{"input_tokens":1000000,"output_tokens":0}}}\n' \
    "$TODAY" > "$HOME/.claude-work/projects/-project/work.jsonl"
  output="$(DEVWS_TOKENS_NOW="$NOW" DEVWS_AGENTS="claude work" DEVWS_AGENT_work="claude ~/.claude-work" \
    "$SCRIPTS/header_tokens.sh" costs)"
  [ "$output" = $'claude 44.71 1\nwork 4 0' ] || fail "unexpected costs per account: $output"
  output="$(DEVWS_TOKENS_NOW="$NOW" DEVWS_AGENTS="work ghost" DEVWS_AGENT_work="claude ~/.claude-work" \
    "$SCRIPTS/header_tokens.sh" costs)"
  [ "$output" = "work 4 0" ] || fail "an unregistered agent was priced: $output"
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
  [ "$(tmux show-option -gqv @devws_limits_claude)" = "37 12" ] || fail "Claude's limits are not recorded"

  mkdir -p "$HOME/.claude"
  printf '{"statusLine":{"type":"command","command":"jq -r .model.display_name; printf mine"}}\n' > "$HOME/.claude/settings.json"
  output="$(printf '%s' '{"model":{"display_name":"Sonnet"}}' | TMUX=test "$SCRIPTS/agent_statusline.sh")"
  [ "$output" = $'Sonnet\nmine' ] || fail "the user's own status line is not shown: $output"
  [ "$(tmux show-option -gqv @devws_limits_claude)" = "37 12" ] ||
    fail "a status line without limits forgot them"
  rm "$HOME/.claude/settings.json"

  # Another account: its limits apart, and its own status line from its folder.
  mkdir -p "$HOME/.claude-work"
  printf '{"statusLine":{"type":"command","command":"printf work-line"}}\n' > "$HOME/.claude-work/settings.json"
  output="$(printf '%s' '{"rate_limits":{"five_hour":{"used_percentage":80},"seven_day":{"used_percentage":50}}}' |
    TMUX=test DEVWS_AGENT=work CLAUDE_CONFIG_DIR="$HOME/.claude-work" "$SCRIPTS/agent_statusline.sh")"
  [ "$output" = "work-line" ] || fail "the account's own status line is not shown: $output"
  [ "$(tmux show-option -gqv @devws_limits_work)" = "80 50" ] || fail "the work account's limits are not recorded"
  [ "$(tmux show-option -gqv @devws_limits_claude)" = "37 12" ] || fail "another account overwrote Claude's limits"
  rm "$HOME/.claude-work/settings.json"
  jq -e '.statusLine.command | contains("agent_statusline.sh")' "$SCRIPTS/agent_settings.json" >/dev/null ||
    fail "devws does not start Claude Code with its status line"
}

# One column for every agent: a row each with its limits used and its month's
# cost, then the month's total.
test_one_column_for_the_agents() {
  local output costs="$TEST_ROOT/costs"
  DEVWS_TOKENS_NOW="$NOW" DEVWS_AGENTS="claude codex" "$SCRIPTS/header_tokens.sh" costs > "$costs"
  output="$(DEVWS_AGENTS="claude codex" "$SCRIPTS/header_agents.sh" "$costs" | plain)"
  [ "$output" = "$(printf '%s\n' \
    'claude 5h ▓▓▓▓░░░░░░  37% wk ▓░░░░░░░░░  12% $45+' \
    'codex  5h ▓▓▓▓░░░░░░  37% wk ░░░░░░░░░░   0% $4.20' \
    'month $49+ at API prices')" ] || fail "unexpected agent column: $output"

  # Without costs worked out yet, only the limits.
  output="$(DEVWS_AGENTS="codex" "$SCRIPTS/header_agents.sh" | plain)"
  [ "$output" = 'codex 5h ▓▓▓▓░░░░░░  37% wk ░░░░░░░░░░   0%' ] || fail "unexpected column without costs: $output"

  tmux set-option -gu @devws_limits_claude
  output="$(DEVWS_AGENTS="claude" "$SCRIPTS/header_agents.sh" | plain)"
  [ "$output" = 'claude 5h ░░░░░░░░░░    — wk ░░░░░░░░░░    —' ] || fail "an unknown limit is not empty: $output"

  # A row for each listed account with its own limits, names aligned to the
  # longest; a name that is not registered has no row.
  tmux set-option -g @devws_limits_claude "37 12"
  tmux set-option -g @devws_limits_company_work "80 50"
  output="$(DEVWS_AGENTS="claude company_work ghost" DEVWS_AGENT_company_work="claude ~/.claude-work" \
    "$SCRIPTS/header_agents.sh" | plain)"
  [ "$output" = "$(printf '%s\n' \
    'claude       5h ▓▓▓▓░░░░░░  37% wk ▓░░░░░░░░░  12%' \
    'company_work 5h ▓▓▓▓▓▓▓▓░░  80% wk ▓▓▓▓▓░░░░░  50%')" ] || fail "unexpected rows per account: $output"

  # No agent to show (none listed, or their folders gone): no made-up rows, a
  # pointer to where agents are registered.
  local hint
  hint="$(printf '%s\n' 'no agents registered' '☰ Menu › Agent › Register')"
  output="$(DEVWS_AGENTS="" "$SCRIPTS/header_agents.sh" "$costs" | plain)"
  [ "$output" = "$hint" ] || fail "no agents shows: $output"
  output="$(HOME="$TEST_ROOT/empty" DEVWS_AGENTS="claude codex" "$SCRIPTS/header_agents.sh" "$costs" | plain)"
  [ "$output" = "$hint" ] || fail "agents without their folders show: $output"
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

# ---------------------------------------------------------------- network

# The macOS sources: en0 on Wi-Fi at -60 dBm on 5 GHz, its byte counters in a
# file, a 7 ms ping; offline when the stub route has no default.
stub_network() {
  local stubs="$TEST_ROOT/network-bin"
  mkdir -p "$stubs"
  cat > "$stubs/route" <<STUB
#!/usr/bin/env bash
[ -f "$TEST_ROOT/stubs/offline" ] || printf '   route to: default\n  interface: en0\n'
STUB
  cat > "$stubs/netstat" <<STUB
#!/usr/bin/env bash
printf 'Name Mtu Network Address Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll\n'
printf 'en0 1500 <Link#11> ea:96 100 0 %s 50 0 %s 0\n' \$(cat "$TEST_ROOT/stubs/counters")
STUB
  cat > "$stubs/system_profiler" <<'STUB'
#!/usr/bin/env bash
cat <<'OUT'
      Interfaces:
        en0:
          Current Network Information:
            <redacted>:
              PHY Mode: 802.11ac
              Channel: 153 (5GHz, 80MHz)
              Signal / Noise: -60 dBm / -93 dBm
          Other Local Wi-Fi Networks:
              Signal / Noise: -80 dBm / -93 dBm
OUT
STUB
  printf '#!/usr/bin/env bash\necho 192.168.1.17\n' > "$stubs/ipconfig"
  printf '#!/usr/bin/env bash\necho "64 bytes from 1.1.1.1: icmp_seq=0 ttl=57 time=6.838 ms"\n' > "$stubs/ping"
  chmod +x "$stubs/"*
  printf '%s' "$stubs"
}

test_network() {
  local stubs state="$TEST_ROOT/network-state" output start
  stubs="$(stub_network)"
  mkdir -p "$state"
  start="$(date +%s)"
  printf '1000000 500000\n' > "$TEST_ROOT/stubs/counters"
  output="$(PATH="$stubs:$PATH" DEVWS_NETWORK_NOW="$start" "$SCRIPTS/header_network.sh" "$state")"
  [ "$output" = $'↓ —  ↑ —\nWi-Fi ▂▄▆_ -60 dBm · 5 GHz\n192.168.1.17 · ping 7 ms' ] ||
    fail "unexpected first network rows: $output"

  # 25 MB in and 1.2 MB out over 10 s.
  printf '26000000 1700000\n' > "$TEST_ROOT/stubs/counters"
  output="$(PATH="$stubs:$PATH" DEVWS_NETWORK_NOW=$((start + 10)) "$SCRIPTS/header_network.sh" "$state")"
  [ "$(sed -n 1p <<< "$output")" = '↓ 2.5 MB/s  ↑ 120 kB/s' ] || fail "unexpected speeds: $output"

  touch "$TEST_ROOT/stubs/offline"
  output="$(PATH="$stubs:$PATH" "$SCRIPTS/header_network.sh" "$state" | plain)"
  [ "$(sed -n 1p <<< "$output")" = offline ] || fail "being offline is not shown: $output"
  rm "$TEST_ROOT/stubs/offline"
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

  # Located by IP, wttr.in may name the place by its coordinates: the name of
  # the area there comes from its JSON format instead, or no name at all.
  printf '+28°C|Light rain shower|-22.830500,-43.219200|68%%|↖4km/h\n' > "$TEST_ROOT/stubs/weather"
  printf '{"nearest_area":[{"areaName":[{"value":"Bom Successo"}],"region":[{"value":"Rio de Janeiro"}]}]}\n' \
    > "$TEST_ROOT/stubs/weather.json"
  output="$("$SCRIPTS/header_weather.sh")"
  [ "$output" = $'+28°C  Light rain shower\nBom Successo\nhumidity 68% · wind ↖4km/h' ] ||
    fail "a place given by coordinates is not named: $output"
  grep -q '^https://wttr.in/-22.830500,-43.219200?format=j1' "$TEST_ROOT/stubs/curl.json.args" ||
    fail "the area is not looked up at the coordinates"
  rm "$TEST_ROOT/stubs/weather.json"
  output="$("$SCRIPTS/header_weather.sh")"
  [ "$output" = $'+28°C  Light rain shower\n\nhumidity 68% · wind ↖4km/h' ] ||
    fail "coordinates are shown when the area has no name: $output"

  rm "$TEST_ROOT/stubs/weather"
  output="$("$SCRIPTS/header_weather.sh")" || fail "being offline failed"
  [ -z "$output" ] || fail "being offline printed: $output"
}

# ---------------------------------------------------------------- layout

# Rows as the ticker passes them: the system, the agents, the network, the
# weather.
layout_rows() {
  printf '%s\n' \
    'CPU #[fg=green]▓▓▓#[fg=default]░░░░░░░  27%' 'MEM ▓▓▓▓▓▓▓▓░░  80%' 'DSK ▓▓▓▓▓▓▓▓▓▓  98%' \
    '#[bold]claude #[nobold] 5h ▓▓▓▓░░░░░░  42%  wk ▓▓▓▓▓▓░░░░  63%  $552' \
    'codex   5h ░░░░░░░░░░   2%  wk ░░░░░░░░░░   1%  $36' '#[dim]month $588 at API prices#[nodim]' \
    '↓ 2.4 MB/s  ↑ 120 kB/s' 'Wi-Fi ▂▄▆_ -60 dBm · 5 GHz' '192.168.1.17 · ping 7 ms' \
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
    " CPU ▓▓▓░░░░░░░  27%"*"│"*"claude  5h ▓▓▓▓░░░░░░  42%  wk ▓▓▓▓▓▓░░░░  63%  \$552"*"│"*"↓ 2.4 MB/s  ↑ 120 kB/s"*"│"*"+18°C  Light rain"*) ;;
    *) fail "unexpected first row: $line" ;;
  esac
  ! shows "$output" 'SYSTEM\|AGENTS\|WEATHER' || fail "the groups still have labels"
  # Spread across the width: the room left over shared between the gaps, so a
  # wide screen gets wide gaps and the last group ends at the edge.
  shows "$line" '27%  \{10,\}│  \{10,\}claude' || fail "the groups are not spread: $line"
  shows "$line" 'Light rain $' || fail "the last group does not end at the edge: $line"
  # Narrow, the gaps keep their two spaces and the widest groups give way first.
  output="$(layout_rows | "$SCRIPTS/header_layout.sh" 100 4 | plain)"
  shows "$(sed -n 1p <<< "$output")" '[^ ]  │  [^ ]' || fail "a narrow header lost its two-space gaps: $output"
  shows "$output" 'Curitiba' || fail "a narrow header cut a short group"
}

# The clock: HH:MM three rows tall, a digit three columns wide.
test_the_clock() {
  local output
  output="$(DEVWS_CLOCK_TIME=12:34 "$SCRIPTS/header_clock.sh")"
  [ "$output" = "$(printf '%s\n' \
    '▀█  ▀▀█ ▄ ▀▀█ █ █' \
    ' █  █▀▀ ▄ ▀▀█ ▀▀█' \
    '▀▀▀ ▀▀▀   ▀▀▀   ▀')" ] || fail "unexpected clock: $output"
  output="$(DEVWS_CLOCK_TIME=05:67 "$SCRIPTS/header_clock.sh")"
  [ "$(wc -l <<< "$output" | tr -d ' ')" -eq 3 ] || fail "the clock is not three rows"
  [ -z "$("$SCRIPTS/header_clock.sh" | tr -d ' ▀▄█\n')" ] || fail "the clock draws more than blocks"
}

# The clock goes first, and is left out when the groups would be cut for it.
test_the_layout_leaves_out_the_clock_when_narrow() {
  local output clock
  clock="$(DEVWS_CLOCK_TIME=12:34 "$SCRIPTS/header_clock.sh")"
  output="$({ printf '%s\n' "$clock"; layout_rows; } | "$SCRIPTS/header_layout.sh" 238 5 --optional-first | plain)"
  shows "$(sed -n 1p <<< "$output")" '^ ▀█  ▀▀█ ▄ ▀▀█ █ █ *│ *CPU' || fail "the clock is not first: $output"
  [ "$(sed -n 1p <<< "$output" | tr -cd '│' | wc -m | tr -d ' ')" -eq 4 ] || fail "the clock is not a group of its own"
  output="$({ printf '%s\n' "$clock"; layout_rows; } | "$SCRIPTS/header_layout.sh" 100 5 --optional-first | plain)"
  ! shows "$output" '▀▀█' || fail "a narrow header kept the clock: $output"
  [ "$(sed -n 1p <<< "$output")" = "$(layout_rows | "$SCRIPTS/header_layout.sh" 100 4 | plain | sed -n 1p)" ] ||
    fail "without the clock the groups are not laid out as before: $output"
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

# A row of the clock as it reads now.
header_clock_row() {
  "$SCRIPTS/header_clock.sh" | sed -n "$1p"
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
  tmux set-option -g @devws_limits_claude "40 10"
  tmux set-option -t dev-work @devws_agent_state working
  # A copy: the conversation stays readable while Codex's folder is away.
  cp "$CODEX_LOG" "$TEST_ROOT/transcript.jsonl"
  tmux set-option -t dev-work @devws_agent_transcript "$TEST_ROOT/transcript.jsonl"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t dev-work"
  # Without Codex's folder its row goes, so the agent group is short a row:
  # the groups after it must stay in their columns.
  mv "$HOME/.codex" "$HOME/.codex.away"
  "$SCRIPTS/header_ticker.sh" --start

  wait_until header_row_shows 3 'CPU ▓' || fail "the client shows no system: $(client_header)"
  wait_until header_row_shows 3 "^ *$(header_clock_row 1)  *│ *CPU" ||
    fail "the client shows no clock before the system: $(client_header)"
  wait_until header_row_shows 3 '│ *claude 5h .*│ *\(↓\|offline\).*│ *+25°C  Sunny' ||
    fail "the client shows no agent columns or weather: $(client_header)"
  wait_until header_row_shows 3 'claude 5h ▓▓▓▓░░░░░░  40% wk ▓░░░░░░░░░  10%' ||
    fail "the client shows no Claude limits: $(client_header)"
  # tmux's own line, then a blank line before the groups.
  header_row_shows 1 'dev-work' || fail "tmux's own line moved: $(client_header)"
  [ -z "$(client_header | sed -n 2p | tr -d ' ')" ] || fail "no blank line under tmux's own line: $(client_header)"
  header_row_shows 4 '│ *month \$.*│' || fail "the agent group's last row moved: $(client_header)"
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

  # One ghost for all the agents: another workspace's agent needs you, so every
  # ghost waits; once it is done the working one wins; with every agent idle it sleeps.
  ghost_colored() {
    shows "$(session_option dev-work @devws_ghost_2)$(session_option dev-work @devws_ghost_3)" "$1"
  }
  tmux new-session -d -s dev-other -x 200 -y 50 'sleep 600'
  tmux set-option -t dev-other @devws_agent_state waiting
  wait_until ghost_colored "$(option @devws_color_warn)" ||
    fail "another agent waiting does not make the ghost wait: $(session_option dev-work @devws_ghost_2)"
  tmux set-option -t dev-other @devws_agent_state done
  wait_until ghost_colored 'colour173' || fail "a working agent does not outrank a done one"
  tmux set-option -t dev-work -u @devws_agent_state
  wait_until ghost_colored "$(option @devws_color_ok)" || fail "a done agent does not show when none works"
  tmux set-option -t dev-other -u @devws_agent_state
  wait_until ghost_colored "$(option @devws_color_dim)" || fail "the ghost does not sleep with every agent idle"
  tmux kill-session -t dev-other

  local first
  first="$(ticker_pid)"
  "$SCRIPTS/header_ticker.sh" --start
  wait_until ticker_replaced "$first" || fail "a restart kept the old ticker"
  wait_until process_gone "$first" || fail "the old ticker still runs"

  local second
  second="$(ticker_pid)"
  mv "$HOME/.codex.away" "$HOME/.codex"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server
  tmux kill-server
  wait_until process_gone "$second" || fail "the ticker outlived the tmux server"
}

# SGR mouse report to the client: button;column;row, pressed then released.
click_client() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" send-keys -t client -l $'\e[<0;'"$1"';'"$2"'M'
  "$REAL_TMUX" -L "$CLIENT_SOCKET" send-keys -t client -l $'\e[<0;'"$1"';'"$2"'m'
}

header_status_is() {
  [ "$(option status)" = "$1" ]
}

ghost_frozen() {
  local before
  before="$(session_option dev-work @devws_ghost_2)"
  sleep 0.5
  [ "$(session_option dev-work @devws_ghost_2)" = "$before" ]
}

# The header hides like a pane: by script, by a click on its rows or on its
# ▾ / ▸, by C-b C-s; hidden, it is tmux's own line and the ticker rests.
redraws() {
  grep -cx "$1" "$TEST_ROOT/redraws.log" 2>/dev/null || true
}

# The ticker looks 20 times a second, but tmux only hears of a change: the
# sleeping ghost moves about twice a second, the rows when their data changes
# (the system's every 5 s, the clock's every minute).
test_the_header_redraws_only_changes() {
  tmux new-session -d -s dev-work -x 200 -y 50 'sleep 600'
  tmux source-file "$TEST_ROOT/status.conf"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t dev-work"
  "$SCRIPTS/header_ticker.sh" --start
  wait_until header_row_shows 3 'CPU' || fail "the header did not show: $(client_header)"
  sleep 1
  : > "$TEST_ROOT/redraws.log"
  sleep 4
  [ "$(redraws ghost)" -le 12 ] || fail "the sleeping ghost was redrawn $(redraws ghost) times in 4 s"
  [ "$(redraws ghost)" -ge 2 ] || fail "the sleeping ghost stopped moving"
  [ "$(redraws rows)" -le 2 ] || fail "the rows were redrawn $(redraws rows) times in 4 s"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server
  tmux kill-server
}

test_the_header_hides() {
  local line marker_column
  tmux new-session -d -s dev-work -x 200 -y 50 'sleep 600'
  tmux source-file "$TEST_ROOT/status.conf"
  sed -n '/^# devws bindings go after the reset/,$p' "$ROOT/config/tmux/tmux.conf" > "$TEST_ROOT/devws.conf"
  tmux source-file "$TEST_ROOT/devws.conf"
  tmux set-option -g mouse on
  tmux set-option -t dev-work @devws_agent_state working
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t dev-work"
  "$SCRIPTS/header_ticker.sh" --start
  wait_until header_row_shows 3 'CPU' || fail "the header did not show: $(client_header)"

  "$SCRIPTS/header_toggle.sh" hide
  header_status_is on && [ "$(option @devws_header_hidden)" = 1 ] || fail "hiding did not leave one line"
  header_row_shows 1 '▸' || fail "a hidden header shows no ▸: $(client_header)"
  wait_until ghost_frozen || fail "the ticker still draws the hidden header"
  "$SCRIPTS/header_toggle.sh" show
  header_status_is 5 && [ -z "$(option @devws_header_hidden)" ] || fail "showing did not bring back five lines"
  ! "$SCRIPTS/header_toggle.sh" sideways 2>/dev/null || fail "an unknown action was accepted"

  # A click on the rows hides them; a click on ▸ brings them back.
  click_client 20 3
  wait_until header_status_is on || fail "clicking the rows did not hide the header"
  line="$(client_header | sed -n 1p)"
  marker_column="${line%%▸*}"
  marker_column=$((${#marker_column} + 1))
  # Apart from the first click, which tmux would otherwise take as a double click.
  sleep 0.6
  click_client "$marker_column" 1
  wait_until header_status_is 5 || fail "clicking ▸ did not bring the header back: $line"
  wait_until header_row_shows 3 'CPU' || fail "the header came back empty: $(client_header)"

  "$REAL_TMUX" -L "$CLIENT_SOCKET" send-keys -t client C-b C-s
  wait_until header_status_is on || fail "C-b C-s did not hide the header"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" send-keys -t client C-b C-s
  wait_until header_status_is 5 || fail "C-b C-s did not bring the header back"

  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server
  tmux kill-server
}

test_the_ghost_has_three_rows_of_one_width
test_the_ghost_shows_its_mood
test_the_ghost_cycle
test_the_bar
test_month_costs_per_agent
test_month_costs_per_account
test_codex_limits
test_tokens_of_the_current_conversation
test_claude_limits_come_from_its_status_line
test_one_column_for_the_agents
test_system_usage
test_network
test_weather
test_the_layout_spreads_the_groups
test_the_clock
test_the_layout_leaves_out_the_clock_when_narrow
test_a_client_shows_the_header
test_the_header_redraws_only_changes
test_the_header_hides

printf 'Header tests passed.\n'
