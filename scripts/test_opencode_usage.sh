#!/usr/bin/env bash
# opencode accounts' usage and credit: what a pane's conversation used
# (`devws agent usage`), every response for the usage history, and the credit
# left of a Zen account (`devws agent credit`), read from opencode's own
# database (<folder>/opencode/opencode.db).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/devws-opencode.XXXXXX")" && pwd -P)"
SCRIPTS="$ROOT/config/tmuxinator"

cleanup() { rm -rf "$TEST_ROOT"; }
trap cleanup EXIT

fail() {
  printf 'opencode usage test failed: %s\n' "$*" >&2
  exit 1
}

command -v sqlite3 >/dev/null 2>&1 || {
  printf 'opencode usage tests skipped: no sqlite3\n'
  exit 0
}

export HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT/tmp/" DEVWS_ENV_FILE=/dev/null
export DEVAGENTICS_HOME="$TEST_ROOT/agentics" DEVWS_AGENTS="opencode"
mkdir -p "$HOME/.local/share/opencode" "$TMPDIR"
DB="$HOME/.local/share/opencode/opencode.db"
WORK="$TEST_ROOT/work"

# Noon today, local, so "today" is never off by one.
NOW="$(date -d "$(date +%F) 12:00:00" +%s 2>/dev/null || date -j -f '%Y-%m-%d %H:%M:%S' "$(date +%F) 12:00:00" +%s)"
TODAY="$(date +%F)"

# The two tables opencode keeps conversations in, with the columns read.
sqlite3 "$DB" "
  CREATE TABLE session (id text PRIMARY KEY, directory text NOT NULL, time_created integer NOT NULL);
  CREATE TABLE message (id text PRIMARY KEY, session_id text NOT NULL, time_created integer NOT NULL, data text NOT NULL);"
# <id> <session> <epoch> <cwd> <cost> <input> <output> <cache read> <cache write>
message() {
  sqlite3 "$DB" "INSERT INTO message VALUES ('$1', '$2', $3 * 1000, json_object(
    'role', 'assistant', 'path', json_object('cwd', '$4'), 'cost', $5, 'modelID', 'deepseek-v4.1-flash',
    'tokens', json_object('input', $6, 'output', $7, 'reasoning', 0, 'cache', json_object('read', $8, 'write', $9))));"
}
sqlite3 "$DB" "INSERT INTO session VALUES ('ses_a', '$WORK/app', $NOW * 1000), ('ses_b', '$WORK/lib', $NOW * 1000);"
message msg_1 ses_a "$NOW" "$WORK/app" 0.25 1000 200 300 0
message msg_2 ses_a "$NOW" "$WORK/app" 0.5 2000 100 0 50
message msg_old ses_b $((NOW - 10 * 86400)) "$WORK/lib" 1.75 500 10 0 0
# What the user typed is not usage.
sqlite3 "$DB" "INSERT INTO message VALUES ('msg_user', 'ses_a', $NOW * 1000, json_object('role', 'user'));"

test_a_conversations_usage() {
  local got
  got="$("$SCRIPTS/header_tokens.sh" usage "$DB#ses_a")"
  [ "$got" = "3000 300 350 0.75 0" ] || fail "usage of ses_a: '$got'"
  got="$("$SCRIPTS/header_tokens.sh" usage "$DB#ses_none")"
  [ "$got" = "0 0 0 0 0" ] || fail "usage of an empty conversation: '$got'"
}

test_every_response_for_the_history() {
  local rows
  rows="$(DEVWS_TOKENS_NOW="$NOW" "$SCRIPTS/usage_history.sh" rows)"
  printf '%s\n' "$rows" | grep -qxF "opencode	$WORK/app	msg_1	$TODAY	1000	200	300	0.25	0" ||
    fail "msg_1 missing from the history rows:
$rows"
  [ "$(printf '%s\n' "$rows" | grep -c .)" -eq 3 ] || fail "expected 3 responses:
$rows"
}

test_credit_is_unknown_until_a_balance_is_entered() {
  local got
  got="$("$SCRIPTS/agent_cli.sh" credit opencode)"
  [ -z "$got" ] || fail "credit without a balance: '$got'"
}

test_credit_left_counts_what_was_spent_since_the_balance() {
  local got
  # Entered yesterday: today's two responses (US$ 0.75) count, the older one not.
  DEVWS_TOKENS_NOW=$((NOW - 86400)) "$SCRIPTS/opencode_credit.sh" set opencode 20 >/dev/null
  got="$("$SCRIPTS/agent_cli.sh" credit opencode)"
  [ "$got" = "20.00 0.75" ] || fail "credit: '$got'"
  "$SCRIPTS/opencode_credit.sh" set opencode abc >/dev/null 2>&1 && fail "a balance that is not a number was saved"
  return 0
}

test_a_non_opencode_account_has_no_credit() {
  local got
  got="$(DEVWS_AGENTS="claude" "$SCRIPTS/agent_cli.sh" credit claude)"
  [ -z "$got" ] || fail "claude credit: '$got'"
}

test_the_headers_month_cost() {
  local got
  got="$(DEVWS_TOKENS_NOW="$NOW" "$SCRIPTS/header_tokens.sh" costs)"
  # The older response is this month's too unless the month began since.
  case "$got" in
    "opencode 0.75 0" | "opencode 2.50 0") ;;
    *) fail "costs: '$got'" ;;
  esac
}

test_a_conversations_usage
test_the_headers_month_cost
test_every_response_for_the_history
test_credit_is_unknown_until_a_balance_is_entered
test_credit_left_counts_what_was_spent_since_the_balance
test_a_non_opencode_account_has_no_credit

printf 'opencode usage tests passed\n'
