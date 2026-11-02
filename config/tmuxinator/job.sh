#!/usr/bin/env bash
# Background jobs: long work (a build, a push) runs apart from what started
# it, so the devws menu closes at once and nothing waits on it. The header
# shows them where the network and weather are (header_ticker.sh), and a click
# there opens the log.
#
#   job.sh start <title> [--lock <name>]... -- <command> [args]
#                         starts the job; prints its id. Jobs holding the same
#                         lock (a jungleforge app) run one after another
#   job.sh note <text> [--error]   a message in the same place: 10 s, or until
#                         its log is opened for an error
#   job.sh list           "<id> <state> <title>", newest first
#   job.sh log [id]       pages the log, following it (the failed one first,
#                         else the newest); a failure counts as seen after
#   job.sh open [client] [id]   the log in a large popup (a click on the
#                         header, or on a job's notification)
#   job.sh rows <width>   the header's three rows while a job is active
#                         (queued, running, done < 10 s ago, failed and not
#                         seen); nothing otherwise
#   job.sh summary        "⏵ 2 jobs · ✗ 1 failed", for tmux's top line
#   job.sh header <width> the summary's line, then the rows: what the header
#                         ticker reads each second, in one call
#   job.sh prune          forgets jobs older than 7 days (start does it too)
#   job.sh run <id>       (internal) the job itself
#
# A job lives in ~/.devws-state/jobs/<id>/ (DEVWS_JOBS_DIR): title, command,
# locks, state (queued, running, ok, failed), its times, exit code and log.
# It never reads input: stdin is /dev/null.
#
# When a job ends it posts a macOS notification — a failure always, a success
# when it ran over 30 s (DEVWS_JOB_NOTIFY_AFTER) — which opens its log when
# clicked. DEVWS_NOTIFY=0 in .env turns them off, as for the agents
# (agent_status.sh); DEVWS_NOTIFY_COMMAND replaces the notifier, for tests:
# called with the title, the message and the job's id.

set -u

script="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
jobs_dir="${DEVWS_JOBS_DIR:-$HOME/.devws-state/jobs}"
shown_after_end=10
kept_for=$((7 * 86400))

die() {
  printf 'devws job: %s\n' "$1" >&2
  exit 2
}

now() {
  date +%s
}

field() {
  cat "$jobs_dir/$1/$2" 2>/dev/null
}

new_job() {
  local id
  id="$(date +%Y%m%d%H%M%S)-$$-$RANDOM"
  mkdir -p "$jobs_dir/$id" || die "cannot create $jobs_dir/$id"
  printf '%s\n' "$1" > "$jobs_dir/$id/title"
  now > "$jobs_dir/$id/queued_at"
  printf '%s' "$id"
}

start() {
  local title="$1" id
  local -a locks=()
  shift
  while [ "$#" -gt 0 ] && [ "$1" != -- ]; do
    case "$1" in
      --lock) [ -n "${2:-}" ] || die "--lock needs a name"; locks+=("$2"); shift 2 ;;
      *) die "unknown option: $1" ;;
    esac
  done
  [ "${1:-}" = -- ] && [ "$#" -ge 2 ] || die "usage: job.sh start <title> [--lock <name>]... -- <command>"
  shift
  prune
  id="$(new_job "$title")"
  printf '%q ' "$@" > "$jobs_dir/$id/command"
  [ "${#locks[@]}" -eq 0 ] || printf '%s\n' "${locks[@]}" | LC_ALL=C sort -u > "$jobs_dir/$id/locks"
  printf 'queued\n' > "$jobs_dir/$id/state"
  # Inside tmux the server runs it, so closing the menu's popup cannot end it.
  # The jobs folder goes along: the server has its own environment.
  if [ -n "${TMUX:-}" ] &&
      tmux run-shell -b "DEVWS_JOBS_DIR=$(printf '%q' "$jobs_dir") $(printf '%q ' "$script" run "$id") >/dev/null 2>&1" 2>/dev/null; then
    :
  else
    nohup "$script" run "$id" </dev/null >/dev/null 2>&1 &
  fi
  printf '%s\n' "$id"
}

# A lock is a folder holding its owner's pid; one whose owner is gone is
# taken over. Locks are taken in name order, so two jobs never wait on each
# other.
take_locks() {
  local id="$1" name folder owner
  [ -f "$jobs_dir/$id/locks" ] || return 0
  mkdir -p "$jobs_dir/.locks"
  while IFS= read -r name; do
    folder="$jobs_dir/.locks/$(printf '%s' "$name" | tr -c 'A-Za-z0-9_.-' _)"
    until mkdir "$folder" 2>/dev/null; do
      owner="$(cat "$folder/pid" 2>/dev/null)"
      if [ -n "$owner" ] && ! kill -0 "$owner" 2>/dev/null; then
        rm -rf "$folder"
        continue
      fi
      sleep 0.5
    done
    printf '%s\n' "$$" > "$folder/pid"
    printf '%s\n' "$folder" >> "$jobs_dir/$id/held"
  done < "$jobs_dir/$id/locks"
}

release_locks() {
  local folder
  [ -f "$jobs_dir/$1/held" ] || return 0
  while IFS= read -r folder; do
    [ "$(cat "$folder/pid" 2>/dev/null)" = "$$" ] && rm -rf "$folder"
  done < "$jobs_dir/$1/held"
  rm -f "$jobs_dir/$1/held"
}

run() {
  local id="$1" status
  [ -f "$jobs_dir/$id/command" ] || die "no such job: $id"
  trap 'release_locks "$id"' EXIT
  take_locks "$id"
  now > "$jobs_dir/$id/started_at"
  printf 'running\n' > "$jobs_dir/$id/state"
  bash -c "$(cat "$jobs_dir/$id/command")" </dev/null > "$jobs_dir/$id/log" 2>&1
  status=$?
  printf '%s\n' "$status" > "$jobs_dir/$id/exit"
  now > "$jobs_dir/$id/ended_at"
  if [ "$status" -eq 0 ]; then
    printf 'ok\n' > "$jobs_dir/$id/state"
  else
    printf 'failed\n' > "$jobs_dir/$id/state"
  fi
  notify_end "$id"
}

notify_end() {
  local id="$1" setting state took title message click
  setting="$(
    env_file="${DEVWS_ENV_FILE:-$(dirname "$script")/.env}"
    # shellcheck source=/dev/null
    [ -r "$env_file" ] && . "$env_file"
    printf '%s' "${DEVWS_NOTIFY:-1}"
  )"
  [ "$setting" != 0 ] || return 0
  state="$(field "$id" state)"
  took=$(($(field "$id" ended_at) - $(field "$id" started_at)))
  if [ "$state" = failed ]; then
    title="✗ $(field "$id" title)"
    message="failed after $(elapsed "$(field "$id" started_at)") — click for the log"
  elif [ "$took" -gt "${DEVWS_JOB_NOTIFY_AFTER:-30}" ]; then
    title="✓ $(field "$id" title)"
    message="done in $(elapsed "$(field "$id" started_at)")"
  else
    return 0
  fi
  if [ -n "${DEVWS_NOTIFY_COMMAND:-}" ]; then
    "$DEVWS_NOTIFY_COMMAND" "$title" "$message" "$id" >/dev/null 2>&1
  elif command -v terminal-notifier >/dev/null 2>&1; then
    # Clicked, it opens the log on the tmux server the job ran under.
    click="$(printf 'TMUX=%q PATH=%q %q open "" %q' "${TMUX:-}" "$PATH" "$script" "$id")"
    terminal-notifier -title "$title" -message "$message" -group "devws-job-$id" \
      -activate com.googlecode.iterm2 -execute "$click" >/dev/null 2>&1
  else
    osascript -e 'on run argv' -e 'display notification (item 2 of argv) with title (item 1 of argv)' \
      -e 'end run' "$title" "$message" >/dev/null 2>&1
  fi
}

note() {
  local text="$1" id
  id="$(new_job "$text")"
  printf 'note\n' > "$jobs_dir/$id/kind"
  cp "$jobs_dir/$id/queued_at" "$jobs_dir/$id/ended_at"
  printf '%s\n' "$text" > "$jobs_dir/$id/log"
  if [ "${2:-}" = --error ]; then
    printf 'failed\n' > "$jobs_dir/$id/state"
  else
    printf 'ok\n' > "$jobs_dir/$id/state"
  fi
  printf '%s\n' "$id"
}

# Ids, newest first.
ids() {
  [ -d "$jobs_dir" ] || return 0
  ls -1 "$jobs_dir" 2>/dev/null | grep -v '^\.' | LC_ALL=C sort -r
}

# The newest ones only, for what the header reads every second: older jobs
# are long finished.
recent_ids() {
  ids | head -n 20
}

# Still to show in the header.
active() {
  local id="$1" ended
  case "$(field "$id" state)" in
    queued | running) return 0 ;;
    # A failure stays until its log is opened, and goes then.
    failed) [ ! -e "$jobs_dir/$id/seen" ]; return ;;
  esac
  ended="$(field "$id" ended_at)"
  [ -n "$ended" ] && [ $(($(now) - ended)) -lt "$shown_after_end" ]
}

elapsed() {
  local seconds=$(($(now) - $1))
  printf '%d:%02d' $((seconds / 60)) $((seconds % 60))
}

# The log's last lines as the terminal would show them: after a carriage
# return only the text written last stays, and control sequences go.
log_tail() {
  tail -n 200 "$1" 2>/dev/null |
    awk -F '\r' '{ print $NF }' |
    sed -E $'s/\x1b\\[[0-9;?]*[ -\\/]*[@-~]//g; s/\x1b[()][A-Za-z0-9]//g' |
    tr -d '\000-\010\013-\037' |
    grep -v '^[[:space:]]*$' | tail -n "$2"
}

# A build's step count (Docker's "#7 [dev 4/9] RUN …") as " ▓▓▓▓░░░░░░ 4/9";
# nothing for a log without one.
progress() {
  local step done total cell bar=""
  step="$(tail -n 200 "$1" 2>/dev/null | grep -oE '\[[^]]*[0-9]+/[0-9]+\]' | tail -n 1 |
    grep -oE '[0-9]+/[0-9]+')"
  [ -n "$step" ] || return 0
  done="${step%/*}"
  total="${step#*/}"
  [ "$total" -gt 0 ] || return 0
  for ((cell = 0; cell < 10; cell++)); do
    if [ "$cell" -lt $((done * 10 / total)) ]; then bar+='▓'; else bar+='░'; fi
  done
  printf ' %s %s' "$bar" "$step"
}

# Cut to <width> characters, then "#" doubled: tmux would read the log's own
# "#[…]" as styles.
fit() {
  local text="${1:0:$2}"
  printf '%s' "${text//#/##}"
}

rows() {
  local width="$1" id title entries="" newest_running="" newest_failed="" line count=0
  for id in $(recent_ids); do
    active "$id" || continue
    title="$(field "$id" title)"
    case "$(field "$id" state)" in
      queued) entries+="… $title queued  " ;;
      running)
        entries+="⏵ $title$(progress "$jobs_dir/$id/log") $(elapsed "$(field "$id" started_at)")  "
        [ -n "$newest_running" ] || newest_running="$id"
        ;;
      ok)
        if [ "$(field "$id" kind)" = note ]; then entries+="• $title  "; else entries+="✓ $title  "; fi
        ;;
      failed)
        if [ "$(field "$id" kind)" = note ]; then entries+="✗ $title  "; else entries+="✗ $title failed  "; fi
        [ -n "$newest_failed" ] || newest_failed="$id"
        ;;
    esac
  done
  [ -n "$entries" ] || return 0
  fit "${entries%  }" "$width"
  printf '\n'
  if [ -n "$newest_running" ]; then
    while IFS= read -r line; do
      fit "  $line" "$width"
      printf '\n'
      count=$((count + 1))
    done < <(log_tail "$jobs_dir/$newest_running/log" 2)
  elif [ -n "$newest_failed" ] && [ "$(field "$newest_failed" kind)" != note ]; then
    fit "  $(log_tail "$jobs_dir/$newest_failed/log" 1)" "$width"
    printf '\n'
    fit '  click here for the log' "$width"
    printf '\n'
    count=2
  fi
  while [ "$count" -lt 2 ]; do
    printf '\n'
    count=$((count + 1))
  done
}

summary() {
  local id running=0 failed=0 out=""
  for id in $(recent_ids); do
    case "$(field "$id" state)" in
      queued | running) running=$((running + 1)) ;;
      failed) [ -e "$jobs_dir/$id/seen" ] || failed=$((failed + 1)) ;;
    esac
  done
  [ "$running" -eq 0 ] || out="⏵ $running job$([ "$running" -eq 1 ] || printf s)"
  [ "$failed" -eq 0 ] || out="${out:+$out · }✗ $failed failed"
  printf '%s' "$out"
}

# The job a click means: an unseen failure, else the newest running, else
# the newest.
relevant() {
  local id state
  for state in failed running; do
    for id in $(ids); do
      [ "$(field "$id" state)" = "$state" ] || continue
      [ "$state" = failed ] && [ -e "$jobs_dir/$id/seen" ] && continue
      printf '%s' "$id"
      return 0
    done
  done
  ids | head -n 1
}

log() {
  local id="${1:-}"
  [ -n "$id" ] || id="$(relevant)"
  [ -n "$id" ] && [ -d "$jobs_dir/$id" ] || die "no job to show"
  : > "$jobs_dir/$id/seen"
  printf '%s · %s\n\n' "$(field "$id" title)" "$(field "$id" state)"
  # shellcheck disable=SC2086
  ${DEVWS_PAGER:-less -R +F} "$jobs_dir/$id/log"
}

open() {
  local client="${1:-}" id="${2:-}"
  [ -n "$id" ] || id="$(relevant)"
  [ -n "$id" ] || { tmux display-message ${client:+-c "$client"} "No jobs yet"; return 0; }
  tmux display-popup ${client:+-c "$client"} -E -w 90% -h 80% -T " $(field "$id" title) — q closes " \
    "$(printf '%q ' "$script" log "$id")"
}

list() {
  local id
  for id in $(ids); do
    printf '%s\t%s\t%s\n' "$id" "$(field "$id" state)" "$(field "$id" title)"
  done
}

prune() {
  local id queued
  for id in $(ids); do
    case "$(field "$id" state)" in queued | running) continue ;; esac
    queued="$(field "$id" queued_at)"
    [ -n "$queued" ] && [ $(($(now) - queued)) -ge "$kept_for" ] && rm -rf "${jobs_dir:?}/$id"
  done
  return 0
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  start) [ -n "${1:-}" ] || die "no title given"; start "$@" ;;
  run) [ -n "${1:-}" ] || die "no job given"; run "$1" ;;
  note) [ -n "${1:-}" ] || die "no text given"; note "$1" "${2:-}" ;;
  list) list ;;
  log) log "${1:-}" ;;
  open) open "${1:-}" "${2:-}" ;;
  rows) rows "${1:-80}" ;;
  summary) summary ;;
  header)
    summary
    printf '\n'
    rows "${1:-60}"
    ;;
  prune) prune ;;
  *) die "usage: job.sh <start|note|list|log|open|rows|summary|header|prune> …" ;;
esac
