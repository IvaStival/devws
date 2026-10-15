#!/usr/bin/env bash
# Records the agent's state on a devws workspace, in the session option
# @devws_agent_state (unset while idle), which tmux.conf's @devws_agent_badge
# and @devws_agent_icon show in the agent's header, the minimized agent strip
# and the status bar, and the sidebar shows next to each workspace.
#
#   agent_status.sh <session> <working|waiting|done|idle>
#   agent_status.sh <session id> --tick   (internal: the spinner's loop)
#
# While the agent works, one ticker per session advances the spinner frame
# (@devws_agent_frame); it stops by itself once the state changes or the
# session is gone.

set -u

session="${1:-}"
state="${2:-}"
# Absolute: the ticker is started from the tmux server's directory.
script="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
frames=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏)

# The ticker follows the session by id, which survives a rename. A second
# ticker can start while the first one is starting; the last one to register
# wins and the other stops at its next frame.
tick() {
  local session_id="$session" frame=0
  tmux set-option -t "$session_id" @devws_agent_ticker "$$" 2>/dev/null || return
  while [ "$(tmux display-message -p -t "$session_id:" '#{@devws_agent_state}|#{@devws_agent_ticker}' 2>/dev/null)" = "working|$$" ]; do
    frame=$(((frame + 1) % ${#frames[@]}))
    tmux set-option -t "$session_id" @devws_agent_frame "${frames[frame]}" 2>/dev/null || return
    sleep 0.15
  done
}

start_ticker() {
  local ticker session_id
  ticker="$(tmux show-option -qv -t "=$session:" @devws_agent_ticker)"
  [ -n "$ticker" ] && kill -0 "$ticker" 2>/dev/null && return
  session_id="$(tmux display-message -p -t "=$session:" '#{session_id}')"
  # Started by the tmux server, so it outlives the hook that asked for it.
  tmux run-shell -b "exec '$script' '$session_id' --tick >/dev/null 2>&1"
}

if [ "$state" = --tick ]; then
  tick
  exit 0
fi

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

previous="$(tmux show-option -qv -t "=$session:" @devws_agent_state)"
if [ "$state" = idle ]; then
  tmux set-option -u -t "=$session:" @devws_agent_state
  state=""
else
  tmux set-option -t "=$session:" @devws_agent_state "$state"
fi
[ "$state" = working ] && start_ticker
# The sidebar redraws its rows, so only when the state changes.
[ "$previous" = "$state" ] || "$(dirname "$script")/refresh_devws_pickers.sh" --reload
exit 0
