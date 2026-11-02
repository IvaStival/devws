#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-job.XXXXXX")"
JOB="$ROOT/config/tmuxinator/job.sh"

cleanup() {
  # Jobs still running would write into the removed folder.
  pkill -f "$TEST_ROOT" 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Job test failed: %s\n' "$*" >&2
  exit 1
}

export DEVWS_JOBS_DIR="$TEST_ROOT/jobs" DEVWS_PAGER=cat
# A failing job would post a real notification.
export DEVWS_NOTIFY_COMMAND=true
unset TMUX TMUX_PANE

job() {
  "$JOB" "$@"
}

# Captured first: with pipefail, grep -q stopping early would fail the pipe.
rows_show() {
  grep -q -- "$1" <<<"$(job rows 80)"
}

state_of() {
  cat "$DEVWS_JOBS_DIR/$1/state" 2>/dev/null
}

# <id> <state>: waits up to 5 s.
wait_for() {
  local attempt
  for attempt in $(seq 1 50); do
    [ "$(state_of "$1")" = "$2" ] && return 0
    sleep 0.1
  done
  fail "job $1 is $(state_of "$1"), not $2"
}

# Moves a job's end (or a note's time) back by <seconds>.
age() {
  local file
  for file in ended_at queued_at; do
    [ -f "$DEVWS_JOBS_DIR/$1/$file" ] || continue
    printf '%s\n' $(($(cat "$DEVWS_JOBS_DIR/$1/$file") - $2)) > "$DEVWS_JOBS_DIR/$1/$file"
  done
}

test_a_job_returns_at_once_and_records_its_end() {
  local started id
  started="$(date +%s)"
  id="$(job start 'Build app' -- sh -c 'echo building; sleep 1; echo built')"
  [ $(($(date +%s) - started)) -le 1 ] || fail "start waited for the job"
  wait_for "$id" ok
  [ "$(cat "$DEVWS_JOBS_DIR/$id/log")" = $'building\nbuilt' ] || fail "log: $(cat "$DEVWS_JOBS_DIR/$id/log")"
  [ "$(cat "$DEVWS_JOBS_DIR/$id/exit")" = 0 ] || fail "exit code not recorded"

  id="$(job start 'Broken' -- sh -c 'exit 3')"
  wait_for "$id" failed
  [ "$(cat "$DEVWS_JOBS_DIR/$id/exit")" = 3 ] || fail "the failure's exit code was not kept"
  FAILED="$id"
}

test_arguments_reach_the_command_unchanged() {
  local id
  id="$(job start 'Quoted' -- printf '%s|' 'two words' "it's" '$HOME')"
  wait_for "$id" ok
  [ "$(cat "$DEVWS_JOBS_DIR/$id/log")" = "two words|it's|\$HOME|" ] ||
    fail "arguments changed: $(cat "$DEVWS_JOBS_DIR/$id/log")"
}

test_jobs_on_one_lock_run_in_order() {
  local first second
  first="$(job start 'First' --lock app -- sh -c "sleep 1; echo first >> '$TEST_ROOT/order'")"
  wait_for "$first" running
  second="$(job start 'Second' --lock app -- sh -c "echo second >> '$TEST_ROOT/order'")"
  sleep 0.3
  [ "$(state_of "$second")" = queued ] || fail "the second job did not wait: $(state_of "$second")"
  rows_show 'Second queued' || fail "the queued job is not shown: $(job rows 80)"
  wait_for "$second" ok
  [ "$(cat "$TEST_ROOT/order")" = $'first\nsecond' ] || fail "order: $(cat "$TEST_ROOT/order")"
}

test_rows_show_running_jobs_and_their_last_lines() {
  local id rows
  id="$(job start 'Build web' -- sh -c 'printf "step 1\n\033[32mstep #2\033[0m\rstep 3 of 9\n"; sleep 2')"
  wait_for "$id" running
  sleep 0.3
  rows="$(job rows 80)"
  [ "$(wc -l <<<"$rows" | tr -d ' ')" = 3 ] || fail "not three rows: $rows"
  grep -q '⏵ Build web 0:0' <<<"$(sed -n 1p <<<"$rows")" || fail "row 1: $(sed -n 1p <<<"$rows")"
  [ "$(sed -n 2p <<<"$rows")" = '  step 1' ] || fail "row 2: $(sed -n 2p <<<"$rows")"
  # The carriage return keeps the last text; control sequences go.
  [ "$(sed -n 3p <<<"$rows")" = '  step 3 of 9' ] || fail "row 3: $(sed -n 3p <<<"$rows")"
  [ "$(job summary)" = '⏵ 1 job · ✗ 1 failed' ] || fail "summary: $(job summary)"
  wait_for "$id" ok
}

test_rows_are_cut_and_escaped_for_tmux() {
  local id
  id="$(job start 'Hash' -- sh -c 'echo "#[fg=red] 0123456789012345678901234567890"; sleep 2')"
  wait_for "$id" running
  sleep 0.3
  [ "$(job rows 20 | sed -n 2p)" = '  ##[fg=red] 01234567' ] || fail "cut/escaped: $(job rows 20 | sed -n 2p)"
  wait_for "$id" ok
}

test_a_success_leaves_after_ten_seconds_a_failure_when_seen() {
  local id
  for id in $(ls "$DEVWS_JOBS_DIR" | grep -v locks); do
    [ "$(state_of "$id")" = ok ] && age "$id" 11
  done
  rows_show "Broken failed" || fail "the failure left: $(job rows 80)"
  ! rows_show "Build app" || fail "an old success is still shown"
  job log "$FAILED" >/dev/null
  [ -z "$(job rows 80)" ] || fail "rows remain once everything is seen: $(job rows 80)"
  [ -z "$(job summary)" ] || fail "summary remains: $(job summary)"
}

test_notes_show_for_ten_seconds_errors_until_seen() {
  local id
  job note 'Tests passed' >/dev/null
  rows_show 'Tests passed' || fail "the note is not shown"
  id="$(job note 'Deploy failed' --error)"
  for note in $(ls "$DEVWS_JOBS_DIR" | grep -v locks); do age "$note" 11; done
  ! rows_show 'Tests passed' || fail "an old note is still shown"
  rows_show 'Deploy failed' || fail "an error note left before being seen"
  job log "$id" >/dev/null
  [ -z "$(job rows 80)" ] || fail "the seen error note remains"
}

# A Docker build's step count becomes a bar on the job's row.
test_a_build_shows_its_progress() {
  local id
  id="$(job start 'Build api' -- sh -c 'echo "#7 [dev 4/9] RUN composer install"; sleep 2')"
  wait_for "$id" running
  sleep 0.3
  grep -q '⏵ Build api ▓▓▓▓░░░░░░ 4/9 0:0' <<<"$(job rows 120 | sed -n 1p)" ||
    fail "no progress bar: $(job rows 120 | sed -n 1p)"
  wait_for "$id" ok
}

# A failure always notifies; a success only after a long run (30 s, here 1 s);
# DEVWS_NOTIFY=0 none.
test_jobs_notify_when_they_end() {
  local id
  export DEVWS_NOTIFY_COMMAND="$TEST_ROOT/notify" DEVWS_JOB_NOTIFY_AFTER=1
  printf '#!/usr/bin/env bash\nprintf "%%s|" "$@" >> "%s/notified"; echo >> "%s/notified"\n' "$TEST_ROOT" "$TEST_ROOT" \
    > "$TEST_ROOT/notify"
  chmod +x "$TEST_ROOT/notify"

  # The job notifies just after it records its end: waited for, up to 5 s.
  notified() {
    local attempt
    for attempt in $(seq 1 50); do
      grep -q "$1" "$TEST_ROOT/notified" 2>/dev/null && return 0
      sleep 0.1
    done
    return 1
  }
  id="$(job start 'Broken build' -- sh -c 'exit 1')"
  notified "^✗ Broken build|failed.*|$id|" || fail "a failure did not notify: $(cat "$TEST_ROOT/notified" 2>/dev/null)"

  id="$(job start 'Quick' -- true)"
  wait_for "$id" ok
  sleep 0.3
  ! grep -q 'Quick' "$TEST_ROOT/notified" || fail "a quick success notified"

  # Times are whole seconds: 2.5 s is at least 2, over the 1 s here.
  id="$(job start 'Long build' -- sleep 2.5)"
  wait_for "$id" ok
  notified "^✓ Long build|" || fail "a long success did not notify"

  printf 'DEVWS_NOTIFY=0\n' > "$TEST_ROOT/quiet.env"
  id="$(DEVWS_ENV_FILE="$TEST_ROOT/quiet.env" job start 'Silenced' -- sh -c 'exit 1')"
  wait_for "$id" failed
  sleep 0.3
  ! grep -q 'Silenced' "$TEST_ROOT/notified" || fail "DEVWS_NOTIFY=0 still notified"
  export DEVWS_NOTIFY_COMMAND=true
  unset DEVWS_JOB_NOTIFY_AFTER
}

test_old_jobs_are_pruned() {
  local id
  id="$(job note 'Ancient')"
  age "$id" $((8 * 86400))
  job prune
  [ ! -e "$DEVWS_JOBS_DIR/$id" ] || fail "a job older than 7 days was kept"
  grep -q 'Tests passed' <<<"$(job list)" || fail "a recent job was pruned"
}

test_a_job_returns_at_once_and_records_its_end
test_arguments_reach_the_command_unchanged
test_jobs_on_one_lock_run_in_order
test_rows_show_running_jobs_and_their_last_lines
test_rows_are_cut_and_escaped_for_tmux
test_a_success_leaves_after_ten_seconds_a_failure_when_seen
test_notes_show_for_ten_seconds_errors_until_seen
test_a_build_shows_its_progress
test_jobs_notify_when_they_end
test_old_jobs_are_pruned

printf 'Job tests passed.\n'
