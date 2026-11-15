#!/usr/bin/env bash
# The usage history (usage_history.sh, `devws usage`, the menu's ◷ Usage
# history): every account's logs, by workspace, agent team and account, for
# today, 7 days, 30 days and all time.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/devws-usage.XXXXXX")" && pwd -P)"
SCRIPTS="$ROOT/config/tmuxinator"

cleanup() { rm -rf "$TEST_ROOT"; }
trap cleanup EXIT

fail() {
  printf 'Usage history test failed: %s\n' "$*" >&2
  exit 1
}

export HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT/tmp/"
# .env's agents are the test's: none of the real ones.
export DEVAGENTICS_HOME="$TEST_ROOT/agentics" DEVWS_ENV_FILE=/dev/null DEVWS_AGENTS="claude codex"
mkdir -p "$HOME/.claude/projects/-app" "$HOME/.claude/projects/-team" "$HOME/.codex/sessions/2026/11/13" \
  "$TMPDIR" "$DEVAGENTICS_HOME/tasks/feat-x"
WORK="$TEST_ROOT/work"
TASKS="$DEVAGENTICS_HOME/tasks"
# feat-x's leader works in the repo's worktree; its lanes are its own folders.
# old-task is gone, but its lane's logs still name it.
printf 'id = "feat-x"\n[[repos]]\nworktree = "%s"\n[[lanes]]\nworktree = "%s"\n' \
  "$WORK/app-feat-x" "$TASKS/feat-x/lanes/back" > "$TASKS/feat-x/task.toml"

# Noon today, local, so "today" and the days before are never off by one.
NOW="$(date -j -f '%Y-%m-%d %H:%M:%S' "$(date +%F) 12:00:00" +%s)"
iso() { date -u -r "$1" +%Y-%m-%dT%H:%M:%S.000Z; }
TODAY="$(iso "$NOW")"
TEN_DAYS="$(iso $((NOW - 10 * 86400)))"
FORTY_DAYS="$(iso $((NOW - 40 * 86400)))"

# Sonnet 5.5 input is $2 a million tokens; gpt-6-astra's, $10.
claude_message() {
  printf '{"type":"assistant","cwd":"%s","timestamp":"%s","message":{"id":"%s","model":"claude-sonnet-5-5","usage":{"input_tokens":1000000,"output_tokens":0}}}\n' "$1" "$2" "$3"
}
codex_log() {
  printf '{"timestamp":"%s","type":"session_meta","payload":{"cwd":"%s"}}\n' "$TODAY" "$1"
  printf '{"timestamp":"%s","type":"turn_context","payload":{"turn_id":"t1","model":"gpt-6-astra"}}\n' "$TODAY"
  printf '{"timestamp":"%s","type":"token_usage_record","payload":{"turn_id":"t1","usage":{"input_tokens":1000000,"cached_input_tokens":0,"cache_write_input_tokens":0,"output_tokens":0}}}\n' "$TODAY"
}
{
  claude_message "$WORK/app" "$TODAY" msg_today
  claude_message "$WORK/app" "$TEN_DAYS" msg_ten_days
} > "$HOME/.claude/projects/-app/one.jsonl"
# A resumed conversation repeats msg_today: it counts once.
{
  claude_message "$WORK/app" "$TODAY" msg_today
  claude_message "$WORK/app" "$FORTY_DAYS" msg_forty_days
} > "$HOME/.claude/projects/-app/two.jsonl"
claude_message "$WORK/app-feat-x" "$TODAY" msg_leader > "$HOME/.claude/projects/-team/leader.jsonl"
codex_log "$TASKS/feat-x/lanes/back" > "$HOME/.codex/sessions/2026/11/13/lane.jsonl"
codex_log "$TASKS/old-task/lanes/front" > "$HOME/.codex/sessions/2026/11/13/gone.jsonl"

history() { DEVWS_TOKENS_NOW="$NOW" "$SCRIPTS/usage_history.sh" show; }
# A row with its spaces squeezed: "app $2.00 1.0M $2.00 1.0M …".
row() { printf '%s\n' "$1" | awk -v name="$2" '$1 == name { $1 = $1; print; exit }'; }
expect() {
  local got
  got="$(row "$1" "$2")"
  [ "$got" = "$2 $3" ] || fail "$2: expected '$3', got '$got'
$1"
}

test_every_section_by_period() {
  local output
  output="$(history)"
  # today, 7 days, 30 days, all time
  expect "$output" app '$2.00 1.0M $2.00 1.0M $4.00 2.0M $6.00 3.0M'
  expect "$output" app-feat-x '$2.00 1.0M $2.00 1.0M $2.00 1.0M $2.00 1.0M'
  expect "$output" feat-x '$12.00 2.0M $12.00 2.0M $12.00 2.0M $12.00 2.0M'
  expect "$output" old-task '$10.00 1.0M $10.00 1.0M $10.00 1.0M $10.00 1.0M'
  expect "$output" claude '$4.00 2.0M $4.00 2.0M $6.00 3.0M $8.00 4.0M'
  expect "$output" codex '$20.00 2.0M $20.00 2.0M $20.00 2.0M $20.00 2.0M'
  [ "$(printf '%s\n' "$output" | grep -c 'all agents')" = 1 ] || fail "no total row"
  printf '%s\n' "$output" | grep 'all agents' | grep -q '\$28.00 6.0M' || fail "wrong total: $output"
  ! printf '%s\n' "$output" | grep -q '^  back ' || fail "a team's lane is listed as a workspace"
}

test_an_unchanged_log_is_not_read_again() {
  local metrics="$TEST_ROOT/metrics"
  history > /dev/null
  rm -f "$metrics"
  DEVWS_TOKEN_CACHE_METRICS="$metrics" history > /dev/null
  [ ! -s "$metrics" ] || fail "unchanged logs were read again: $(cat "$metrics")"
  claude_message "$WORK/app" "$TODAY" msg_more >> "$HOME/.claude/projects/-app/one.jsonl"
  # A second later, so the changed log's modification time differs.
  touch -t "$(date -r $(($(date +%s) + 2)) +%Y%m%d%H%M.%S)" "$HOME/.claude/projects/-app/one.jsonl"
  local output
  output="$(DEVWS_TOKEN_CACHE_METRICS="$metrics" history)"
  [ "$(grep -c parse "$metrics")" = 1 ] || fail "the changed log was not read again once: $(cat "$metrics")"
  expect "$output" app '$4.00 2.0M $4.00 2.0M $6.00 3.0M $8.00 4.0M'
}

test_an_account_from_env_and_billions() {
  local env_file="$TEST_ROOT/devws.env" output
  mkdir -p "$HOME/.claude-work/projects/-work"
  printf '{"type":"assistant","cwd":"%s","timestamp":"%s","message":{"id":"msg_work","model":"claude-sonnet-5-5","usage":{"input_tokens":2000000000,"output_tokens":0}}}\n' \
    "$WORK/app" "$TODAY" > "$HOME/.claude-work/projects/-work/work.jsonl"
  printf 'DEVWS_AGENTS="claude codex claude_work"\nDEVWS_AGENT_claude_work="claude ~/.claude-work"\n' > "$env_file"
  output="$(env -u DEVWS_AGENTS DEVWS_ENV_FILE="$env_file" DEVWS_TOKENS_NOW="$NOW" "$SCRIPTS/usage_history.sh" show)"
  expect "$output" claude_work '$4000.00 2.0B $4000.00 2.0B $4000.00 2.0B $4000.00 2.0B'
  rm -rf "$HOME/.claude-work"
}

test_devws_usage_and_the_menu_item() {
  local output
  output="$(DEVWS_TOKENS_NOW="$NOW" zsh "$ROOT/bin/devws" usage </dev/null 2>&1)"
  printf '%s\n' "$output" | grep -q '^USAGE HISTORY' || fail "devws usage did not show the history: $output"
  grep -q '^tools|usage-history|◷ Usage history||show_usage_history$' "$SCRIPTS/devws_menu.sh" ||
    fail "the menu has no Usage history item"
  grep -q '^show_usage_history()' "$SCRIPTS/workspace_actions.sh" || fail "the menu item has no action"
}

test_every_section_by_period
test_an_unchanged_log_is_not_read_again
test_an_account_from_env_and_billions
test_devws_usage_and_the_menu_item

printf 'Usage history tests passed\n'
