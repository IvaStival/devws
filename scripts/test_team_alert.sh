#!/usr/bin/env bash
# team_alert.sh sweep: one alert when a whole agent team has stopped, none
# while it works, waits on you, or has orders queued.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="${TMPDIR:-/tmp}"
TEST_ROOT="$(mktemp -d "${tmp_dir%/}/devws-team-alert.XXXXXX")"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-team-alert-test-$$"
CLIENT_SOCKET="devws-team-alert-client-$$"

cleanup() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server 2>/dev/null || true
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Team alert test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
cat > "$TEST_ROOT/bin/notify" <<EOF
#!/usr/bin/env bash
printf '%s|%s|%s|%s|%s\n' "\$1" "\$2" "\$3" "\$4" "\$5" >> "$TEST_ROOT/notifications"
EOF
chmod +x "$TEST_ROOT/bin/"*
ln -s "$ROOT/config/tmuxinator" "$TEST_ROOT/home/.tmuxinator"
: > "$TEST_ROOT/devws.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT" \
  DEVWS_NOTIFY_COMMAND="$TEST_ROOT/bin/notify" DEVWS_ENV_FILE="$TEST_ROOT/devws.env" \
  DEVAGENTICS_HOME="$TEST_ROOT/agentics"
unset TMUX
SCRIPTS="$HOME/.tmuxinator"
TASK_DIR="$DEVAGENTICS_HOME/tasks/feat-x"
mkdir -p "$TASK_DIR"
printf 'id = "feat-x"\nbranch = "feat/x"\nworkspace = "dev-app-feat-x"\n' > "$TASK_DIR/task.toml"

# The leader's workspace and the floor's, each with one agent pane.
for workspace in dev-app-feat-x dev-app-feat-x-floor; do
  tmux new-session -d -s "$workspace" -x 200 -y 50 'sleep 600'
  tmux set-option -t "$workspace" @devws_task feat-x
done
tmux set-option -p -t dev-app-feat-x: @devws_role agent
tmux set-option -p -t dev-app-feat-x-floor: @devws_role extra-agent
LEADER="$(tmux display-message -p -t dev-app-feat-x: '#{pane_id}')"
HAMMER="$(tmux display-message -p -t dev-app-feat-x-floor: '#{pane_id}')"
# A workspace without a task never takes part.
tmux new-session -d -s dev-solo -x 200 -y 50 'sleep 600'
tmux set-option -p -t dev-solo: @devws_role agent
tmux set-option -p -t dev-solo: @devws_pane_state done

# <paused> <queued> <steps…>: the task's state.json; a step is "<id>:<status>".
task_state() {
  local paused="$1" queued="$2" steps="" step
  shift 2
  for step in "$@"; do
    steps+="${steps:+,}{\"id\":\"${step%%:*}\",\"title\":\"t\",\"status\":\"${step#*:}\",\"attempts\":1}"
  done
  printf '{"paused":%s,"queue":[%s],"steps":[%s]}\n' "$paused" \
    "$([ "$queued" = 0 ] || printf '{"to":"hammer-1","text":"go"}')" "$steps" > "$TASK_DIR/state.json"
}

pane_states() {
  tmux set-option -p -t "$LEADER" @devws_pane_state "$1"
  tmux set-option -p -t "$HAMMER" @devws_pane_state "$2"
}

option() {
  tmux show-option -qv -t '=dev-app-feat-x:' "$1"
}

# The team has been quiet for <seconds>.
quiet_for() {
  tmux set-option -t dev-app-feat-x @devws_team_quiet_since $(($(date +%s) - $1))
}

# Alerts are posted in the background: a sweep waits until its notifier
# processes are gone.
sweep() {
  local attempt
  "$SCRIPTS/team_alert.sh" sweep || fail "the sweep failed"
  for attempt in $(seq 1 25); do
    pgrep -f "$TEST_ROOT/bin/notify" >/dev/null || break
    sleep 0.2
  done
  sleep 0.3
}

notifications() {
  cat "$TEST_ROOT/notifications" 2>/dev/null
}

alerts() {
  notifications | grep -c 'team stopped' || true
}

reset() {
  rm -f "$TEST_ROOT/notifications"
  tmux set-option -u -t dev-app-feat-x @devws_team_quiet_since 2>/dev/null || true
  tmux set-option -u -t dev-app-feat-x @devws_team_alerted 2>/dev/null || true
  : > "$DEVWS_ENV_FILE"
}

test_a_team_quiet_for_20_seconds_alerts_once() {
  reset
  task_state false 0 s1:accepted s2:reported
  pane_states done done
  sweep
  [ -n "$(option @devws_team_quiet_since)" ] || fail "the quiet spell was not recorded"
  [ "$(alerts)" = 0 ] || fail "alerted before 20 s: $(notifications)"
  quiet_for 21
  sweep
  [ "$(alerts)" = 1 ] || fail "no alert after 20 s quiet: $(notifications)"
  [ "$(notifications)" = "feat-x ✓ team stopped|feat/x|The team is idle: nobody working, nothing queued|dev-app-feat-x|$LEADER" ] ||
    fail "alert: $(notifications)"
  sweep
  [ "$(alerts)" = 1 ] || fail "the same quiet spell alerted again: $(notifications)"
}

test_a_busy_team_does_not_alert() {
  local states
  for states in "working done" "done working" "waiting done" "done waiting"; do
    reset
    task_state false 0
    # shellcheck disable=SC2086
    pane_states $states
    quiet_for 60
    sweep
    [ "$(alerts)" = 0 ] || fail "alerted with agents '$states': $(notifications)"
    [ -z "$(option @devws_team_quiet_since)" ] || fail "a busy team kept its quiet spell"
  done
}

test_a_queued_order_counts_unless_paused() {
  reset
  task_state false 1
  pane_states done done
  quiet_for 60
  sweep
  [ "$(alerts)" = 0 ] || fail "alerted with an order queued: $(notifications)"
  task_state true 1
  sweep
  quiet_for 21
  sweep
  notifications | grep -q '|The team is paused|' || fail "a paused team did not alert: $(notifications)"
}

test_work_then_quiet_again_alerts_again() {
  reset
  task_state false 0 s1:accepted
  pane_states done done
  quiet_for 21
  sweep
  pane_states done working
  sweep
  [ -z "$(option @devws_team_alerted)" ] || fail "work did not end the quiet spell"
  pane_states done done
  sweep
  quiet_for 21
  sweep
  [ "$(alerts)" = 2 ] || fail "the second quiet spell did not alert: $(notifications)"
  notifications | tail -n 1 | grep -q '|All steps accepted|' || fail "reason: $(notifications | tail -n 1)"
}

test_no_alert_while_a_team_workspace_is_on_screen_or_turned_off() {
  reset
  task_state false 0
  pane_states done done
  TMUX='' script -q /dev/null "$REAL_TMUX" -L "$SOCKET" attach -t dev-app-feat-x-floor >/dev/null 2>&1 &
  local attempt
  for attempt in $(seq 1 30); do
    [ -n "$(tmux list-clients 2>/dev/null)" ] && break
    sleep 0.2
  done
  quiet_for 21
  sweep
  [ "$(alerts)" = 0 ] || fail "alerted while the floor is on screen: $(notifications)"
  tmux detach-client -a -s dev-app-feat-x-floor 2>/dev/null || tmux detach-client -s dev-app-feat-x-floor 2>/dev/null || true
  for attempt in $(seq 1 30); do
    [ -z "$(tmux list-clients 2>/dev/null)" ] && break
    sleep 0.2
  done
  reset
  printf 'DEVWS_NOTIFY=waiting\n' > "$DEVWS_ENV_FILE"
  quiet_for 21
  sweep
  [ "$(alerts)" = 0 ] || fail "DEVWS_NOTIFY=waiting let the team alert: $(notifications)"
}

test_a_team_quiet_for_20_seconds_alerts_once
test_a_busy_team_does_not_alert
test_a_queued_order_counts_unless_paused
test_work_then_quiet_again_alerts_again
test_no_alert_while_a_team_workspace_is_on_screen_or_turned_off

printf 'Team alert tests passed.\n'
