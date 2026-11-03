#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-job-panel.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-job-panel-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Job panel test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call goes to an isolated server, whose jobs (run-shell) inherit
# this environment: quick panel timings, the test's jobs folder, no
# notifications.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
chmod +x "$TEST_ROOT/bin/tmux"
for script in job.sh job_panel.sh workspace_panes.sh save_state.sh; do
  ln -s "$SCRIPTS/$script" "$TEST_ROOT/home/.tmuxinator/$script"
done
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT" \
  DEVWS_JOBS_DIR="$TEST_ROOT/jobs" DEVWS_JOB_PANEL_AFTER=0.3 DEVWS_JOB_PANEL_LINGER=1 \
  DEVWS_NOTIFY_COMMAND=true
unset TMUX TMUX_PANE
JOB="$HOME/.tmuxinator/job.sh"

# A workspace: the editor on top, the terminal below it, the terminal active.
tmux new-session -d -s dev-ws -n workspace -x 120 -y 40 'sleep 600'
EDITOR_PANE="$(tmux display-message -p -t dev-ws: '#{pane_id}')"
tmux set-option -p -t "$EDITOR_PANE" @devws_role editor
TERMINAL_PANE="$(tmux split-window -t "$EDITOR_PANE" -v -P -F '#{pane_id}' 'sleep 600')"
tmux set-option -p -t "$TERMINAL_PANE" @devws_role terminal
tmux select-pane -t "$TERMINAL_PANE"
# save_state.sh only records a workspace with a folder.
tmux set-option -t dev-ws @devws_root "$TEST_ROOT"

wait_until() {
  local attempt
  for attempt in $(seq 1 50); do
    "$@" && return 0
    sleep 0.1
  done
  return 1
}

panel() {
  tmux list-panes -s -t dev-ws: -F '#{pane_id}|#{@devws_role}' | awk -F '|' '$2 == "job-log" { print $1 }'
}

has_panel() {
  [ -n "$(panel)" ]
}

no_panel() {
  [ -z "$(panel)" ]
}

# A job started from the workspace's terminal.
start_job() {
  TMUX="fake,1,0" TMUX_PANE="$TERMINAL_PANE" "$JOB" start "$@"
}

state_of() {
  cat "$DEVWS_JOBS_DIR/$1/state" 2>/dev/null
}

test_a_quick_job_opens_no_panel() {
  local id
  id="$(start_job Quick -- true)"
  [ "$(cat "$DEVWS_JOBS_DIR/$id/session")" = dev-ws ] || fail "the job did not record its workspace"
  sleep 1
  no_panel || fail "a quick job opened a panel"
}

test_a_long_job_opens_a_panel_below_the_terminal() {
  local id pane
  id="$(start_job 'Build api' -- sh -c 'echo "#7 [dev 4/9] RUN composer"; sleep 2')"
  wait_until has_panel || fail "a long job opened no panel"
  pane="$(panel)"
  [ "$(tmux display-message -p -t "$pane" '#{pane_top}')" -gt "$(tmux display-message -p -t "$TERMINAL_PANE" '#{pane_top}')" ] ||
    fail "the panel is not below the terminal"
  [ "$(tmux display-message -p -t dev-ws: '#{pane_id}')" = "$TERMINAL_PANE" ] || fail "the panel took the focus"
  panel_shows() { tmux capture-pane -p -t "$pane" | grep -q 'RUN composer'; }
  wait_until panel_shows || fail "the panel does not show the log: $(tmux capture-pane -p -t "$pane")"
  titled() { tmux show-options -pqv -t "$pane" @devws_job_title | grep -q '⏵ Build api ▓▓▓▓░░░░░░ 4/9'; }
  wait_until titled || fail "the panel's header: $(tmux show-options -pqv -t "$pane" @devws_job_title)"
  # Done: it says so, then closes after the linger.
  wait_until no_panel || fail "the panel did not close after the job succeeded"
}

test_a_failure_stays_until_q() {
  local id
  id="$(start_job Broken -- sh -c 'sleep 1; exit 1')"
  wait_until has_panel || fail "a failing job opened no panel"
  failed() { [ "$(state_of "$id")" = failed ]; }
  wait_until failed || fail "the job did not fail"
  sleep 1.5
  has_panel || fail "the panel of a failure closed by itself"
  tmux send-keys -t "$(panel)" q
  wait_until no_panel || fail "q did not close the panel"
  [ -e "$DEVWS_JOBS_DIR/$id/seen" ] || fail "closing the failure's panel did not mark it seen"
}

test_a_new_job_reuses_the_panel() {
  local first second
  first="$(start_job First -- sleep 3)"
  wait_until has_panel || fail "the first job opened no panel"
  second="$(start_job Second -- sleep 3)"
  sleep 1
  [ "$(panel | grep -c .)" = 1 ] || fail "two jobs opened two panels"
  newest() { tmux show-options -pqv -t "$(panel)" @devws_job_title | grep -q Second; }
  wait_until newest || fail "the panel did not move to the newest job"
}

test_the_panel_is_not_saved_and_closes_on_request() {
  has_panel || fail "no panel to check"
  "$HOME/.tmuxinator/save_state.sh"
  ! grep -q 'job-log' "$HOME/.devws-state/sessions.tsv" || fail "the panel was saved"
  "$SCRIPTS/job_panel.sh" close dev-ws
  no_panel || fail "close left the panel"
}

test_a_quick_job_opens_no_panel
test_a_long_job_opens_a_panel_below_the_terminal
test_a_failure_stays_until_q
test_a_new_job_reuses_the_panel
test_the_panel_is_not_saved_and_closes_on_request

printf 'Job panel tests passed.\n'
