#!/usr/bin/env bash
# Records the agent's state on a devws workspace, in the session option
# @devws_agent_state (unset while idle), which tmux.conf's @devws_agent_badge
# and @devws_agent_icon show in the agent's header, the minimized agent strip
# and the status bar, and the sidebar shows next to each workspace. When the
# state started goes in @devws_agent_state_since (epoch seconds), which
# jump_agent.sh orders the waiting agents by.
#
# When an agent starts waiting or is done in a workspace no client shows, a
# macOS notification says so. DEVWS_NOTIFY in .env: 1 (default) for both,
# "waiting" for waiting only, 0 for none. DEVWS_NOTIFY_COMMAND replaces the
# notifier, for tests: it is called with the title and the message.
#
#   agent_status.sh <session> <working|waiting|done|idle>
#
# The spinner shown while an agent works turns in header_ticker.sh, one clock
# for every workspace (agent_spinner.sh).

set -u

session="${1:-}"
state="${2:-}"
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
