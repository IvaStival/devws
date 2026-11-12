#!/usr/bin/env bash
# One alert when a whole agent team has stopped. Its agents turn done after
# every order, so they alert only when they need you (agent_status.sh); this
# tells when nothing is left going on.
#
#   team_alert.sh sweep   (header_ticker.sh, every 5 s)
#
# A team (the workspaces sharing an @devws_task: the leader's, the seniors',
# the floor's) is quiet while none of its agents works or waits and no order
# is queued for one; a paused task's queue does not count, since nothing will
# be delivered. Quiet for 20 s, so the gap between two orders does not count,
# it alerts once: "<task> ✓ team stopped", with why (all steps accepted,
# paused, or idle). A click goes to the leader. The spell is kept on the
# leader's workspace (@devws_team_quiet_since, @devws_team_alerted) and ends
# when the team works again.
#
# No alert while a client shows one of the team's workspaces, nor unless
# DEVWS_NOTIFY is 1 (alert.sh). The task's files are read directly
# (DEVAGENTICS_HOME, else ~/.devagentics): no devagentics call every 5 s.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=alert.sh
. "$script_dir/alert.sh"
tasks_dir="${DEVAGENTICS_HOME:-$HOME/.devagentics}/tasks"
quiet_after=20

# <task.toml> <key>: a top-level string value.
toml_value() {
  sed -n "s/^$2 = \"\\(.*\\)\"\$/\\1/p" "$1" 2>/dev/null | head -n 1
}

# <task>: whether a client shows one of its workspaces.
on_screen() {
  local shown
  for shown in $(tmux list-clients -F '#{client_session}' 2>/dev/null); do
    [ "$(tmux show-option -qv -t "=$shown:" @devws_task 2>/dev/null)" = "$1" ] && return 0
  done
  return 1
}

# Ends the leader workspace's quiet spell, if it has one.
busy() {
  [ -z "$(tmux show-option -qv -t "=$1:" @devws_team_quiet_since 2>/dev/null)" ] ||
    tmux set-option -u -t "=$1:" @devws_team_quiet_since 2>/dev/null
  [ -z "$(tmux show-option -qv -t "=$1:" @devws_team_alerted 2>/dev/null)" ] ||
    tmux set-option -u -t "=$1:" @devws_team_alerted 2>/dev/null
}

# <task> <busy: 1 or 0>
check_task() {
  local task="$1" agents_busy="$2" dir leader paused queued accepted since now reason leader_pane
  dir="$tasks_dir/$task"
  leader="$(toml_value "$dir/task.toml" workspace)"
  [ -n "$leader" ] && tmux has-session -t "=$leader" 2>/dev/null || return 0
  if [ "$agents_busy" = 1 ]; then
    busy "$leader"
    return 0
  fi
  IFS=$'\t' read -r paused queued accepted < <(
    jq -r '[(.paused // false), ((.queue // []) | length),
            ((.steps // []) as $steps | ($steps | length) > 0 and ($steps | all(.status == "accepted")))]
           | @tsv' "$dir/state.json" 2>/dev/null
  ) || return 0
  if [ "$queued" != 0 ] && [ "$paused" != true ]; then
    busy "$leader"
    return 0
  fi
  now="$(date +%s)"
  since="$(tmux show-option -qv -t "=$leader:" @devws_team_quiet_since 2>/dev/null)"
  if [ -z "$since" ]; then
    tmux set-option -t "=$leader:" @devws_team_quiet_since "$now"
    return 0
  fi
  [ -z "$(tmux show-option -qv -t "=$leader:" @devws_team_alerted 2>/dev/null)" ] || return 0
  [ $((now - since)) -ge "$quiet_after" ] || return 0
  # Once per spell, also when it is not posted: you saw it, or asked for none.
  tmux set-option -t "=$leader:" @devws_team_alerted 1
  [ "$(notify_setting)" = 1 ] || return 0
  on_screen "$task" && return 0
  if [ "$accepted" = true ]; then
    reason="All steps accepted"
  elif [ "$paused" = true ]; then
    reason="The team is paused"
  else
    reason="The team is idle: nobody working, nothing queued"
  fi
  leader_pane="$(tmux list-panes -s -t "=$leader" -F '#{pane_id} #{@devws_role}' 2>/dev/null |
    awk '$2 == "agent" { print $1; exit }')"
  post_alert "$task ✓ team stopped" "$(toml_value "$dir/task.toml" branch)" "$reason" "$leader" "$leader_pane"
}

sweep() {
  local task agents_busy
  # One tmux call: per task, whether any of its agents works or waits.
  while IFS=$'\t' read -r task agents_busy; do
    check_task "$task" "$agents_busy"
  done < <(
    tmux list-panes -a -F "#{@devws_task}	#{@devws_role}	#{@devws_pane_state}" 2>/dev/null |
      awk -F '\t' '$1 != "" && $2 ~ /^(agent|extra-agent)$/ {
        if (!($1 in busy)) busy[$1] = 0
        if ($3 == "working" || $3 == "waiting") busy[$1] = 1
      }
      END { for (task in busy) printf "%s\t%s\n", task, busy[task] }'
  )
}

case "${1:-}" in
  sweep) sweep ;;
  *)
    printf 'Usage: team_alert.sh sweep\n' >&2
    exit 2
    ;;
esac
