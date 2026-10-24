#!/usr/bin/env bash
# Records an agent's state on its pane, and the workspace's: a workspace can
# hold several agents (the main one, and extra ones from New panel).
#
# Each agent pane keeps its own state in the pane option @devws_pane_state
# (unset while idle), which its header shows (tmux.conf's @devws_pane_icon and
# @devws_pane_badge). The workspace's @devws_agent_state (unset while idle)
# sums its agents up —
# waiting, else working, else done — for the minimized strip, the status bar,
# the sidebar and the header's ghost. When that state started goes in
# @devws_agent_state_since (epoch seconds), which jump_agent.sh orders the
# waiting agents by.
#
# When an agent starts waiting or is done in a workspace no client shows, a
# macOS notification says so. DEVWS_NOTIFY in .env: 1 (default) for both,
# "waiting" for waiting only, 0 for none. DEVWS_NOTIFY_COMMAND replaces the
# notifier, for tests: it is called with the title and the message.
#
#   agent_status.sh <session> <working|waiting|done|idle> [pane]
#                                  the agent in <pane>; without one, the
#                                  workspace's main agent (role agent)
#
# The spinner shown while an agent works turns in header_ticker.sh, one clock
# for every workspace (agent_spinner.sh).

set -u

session="${1:-}"
state="${2:-}"
pane="${3:-}"
script="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"

case "$state" in
  working|waiting|done|idle) ;;
  *)
    printf 'Unknown agent state: %s (working, waiting, done, idle)\n' "$state" >&2
    exit 2
    ;;
esac
if ! tmux has-session -t "=$session" 2>/dev/null; then
  printf 'No devws workspace named: %s\n' "$session" >&2
  exit 1
fi

# Whether a client shows the workspace, where the user sees the state already.
on_screen() {
  tmux list-clients -F '#{client_session}' 2>/dev/null | grep -qxF "$session"
}

notify() {
  local title="${session#dev-}" message agent setting
  # DEVWS_ENV_FILE replaces .env, for tests.
  setting="$(
    env_file="${DEVWS_ENV_FILE:-$(dirname "$script")/.env}"
    # shellcheck source=/dev/null
    [ -r "$env_file" ] && . "$env_file"
    printf '%s' "${DEVWS_NOTIFY:-1}"
  )"
  case "$setting:$state" in
    0:*|waiting:done) return 0 ;;
  esac
  on_screen && return 0
  agent="$(tmux show-option -qv -t "=$session:" @devws_agent)"
  case "$state" in
    waiting) message="${agent:-the agent} needs you" ;;
    done) message="${agent:-the agent} is done" ;;
  esac
  # In the background: the hook that called this must not wait on it.
  if [ -n "${DEVWS_NOTIFY_COMMAND:-}" ]; then
    "$DEVWS_NOTIFY_COMMAND" "$title" "$message" >/dev/null 2>&1 &
  else
    # Passed as arguments, so quotes in a name can't break the script.
    osascript -e 'on run argv' -e 'display notification (item 2 of argv) with title (item 1 of argv)' \
      -e 'end run' "devws · $title" "$message" >/dev/null 2>&1 &
  fi
}

# The workspace's agent panes, also those hidden while minimized.
agent_panes() {
  tmux list-panes -s -t "=$session" -F '#{pane_id} #{@devws_role}' 2>/dev/null |
    awk '$2 ~ /^(agent|extra-agent)$/ { print $1 }'
}

# The workspace's state from its agents' own ones: waiting, else working,
# else done, else idle.
summary() {
  local states="" agent
  for agent in $(agent_panes); do
    states+=" $(tmux show-options -pqv -t "$agent" @devws_pane_state 2>/dev/null) "
  done
  for candidate in waiting working done; do
    case "$states" in *" $candidate "*) printf '%s' "$candidate"; return ;; esac
  done
  printf 'idle'
}

[ -n "$pane" ] || pane="$(tmux list-panes -s -t "=$session" -F '#{pane_id} #{@devws_role}' 2>/dev/null |
  awk '$2 == "agent" { print $1; exit }')"
if [ -n "$pane" ]; then
  if [ "$state" = idle ]; then
    tmux set-option -p -u -t "$pane" @devws_pane_state 2>/dev/null
  else
    tmux set-option -p -t "$pane" @devws_pane_state "$state" 2>/dev/null
  fi
  state="$(summary)"
fi

previous="$(tmux show-option -qv -t "=$session:" @devws_agent_state)"
if [ "$state" = idle ]; then
  tmux set-option -u -t "=$session:" @devws_agent_state
  tmux set-option -u -t "=$session:" @devws_agent_state_since
  state=""
else
  tmux set-option -t "=$session:" @devws_agent_state "$state"
  [ "$previous" = "$state" ] || tmux set-option -t "=$session:" @devws_agent_state_since "$(date +%s)"
fi
# The sidebar redraws its rows, and a notification goes out, only when the
# state changes.
if [ "$previous" != "$state" ]; then
  # The frame is global (agent_spinner.sh); one left on the session by an
  # older devws, with a ticker per workspace, would hide it.
  tmux set-option -qu -t "=$session:" @devws_agent_frame
  "$(dirname "$script")/refresh_devws_pickers.sh" --reload
  case "$state" in
    waiting|done) notify ;;
  esac
fi
exit 0
