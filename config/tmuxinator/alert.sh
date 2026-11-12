#!/usr/bin/env bash
# The macOS alerts of devws's agents (agent_status.sh, team_alert.sh), sourced.
#
#   notify_setting                 DEVWS_NOTIFY from .env (DEVWS_ENV_FILE for
#                                  tests): 1 (default), waiting or 0
#   post_alert <title> <subtitle> <message> <session> [pane]
#                                  posts it in the background; a click goes to
#                                  <session> (and <pane>) through focus_agent.sh
#
# Clickable needs terminal-notifier (DEVWS_TERMINAL_NOTIFIER replaces it, for
# tests), else osascript shows it, not clickable. DEVWS_NOTIFY_COMMAND
# replaces both, for tests: called with the title, the subtitle, the message,
# the session and the pane.

alert_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

notify_setting() (
  env_file="${DEVWS_ENV_FILE:-$alert_dir/.env}"
  # shellcheck source=/dev/null
  [ -r "$env_file" ] && . "$env_file"
  printf '%s' "${DEVWS_NOTIFY:-1}"
)

# What a click on the alert runs: Notification Center runs it with /bin/sh,
# outside tmux and with a bare PATH, so it names the server by its socket in
# TMUX (as tmux does for its own panes) and puts tmux's folder on PATH.
alert_click_command() {
  local socket tmux_dir
  socket="$(tmux display-message -p '#{socket_path}' 2>/dev/null)"
  tmux_dir="$(dirname "$(command -v tmux)")"
  printf 'TMUX=%q PATH=%q %q %q %q' "$socket,0,0" "$tmux_dir:/usr/bin:/bin" \
    "$alert_dir/focus_agent.sh" "$1" "$2"
}

post_alert() {
  local title="$1" subtitle="$2" message="$3" session="$4" pane="${5:-}"
  local notifier="${DEVWS_TERMINAL_NOTIFIER:-terminal-notifier}"
  # In the background: whatever called this must not wait on it.
  if [ -n "${DEVWS_NOTIFY_COMMAND:-}" ]; then
    "$DEVWS_NOTIFY_COMMAND" "$title" "$subtitle" "$message" "$session" "$pane" >/dev/null 2>&1 &
  elif command -v "$notifier" >/dev/null 2>&1; then
    # A newer alert from the workspace replaces the older one.
    "$notifier" -title "$title" -subtitle "$subtitle" -message "$message" -group "devws-$session" \
      -activate com.googlecode.iterm2 -execute "$(alert_click_command "$session" "$pane")" >/dev/null 2>&1 &
  else
    # Passed as arguments, so quotes in a name can't break the script.
    osascript -e 'on run argv' \
      -e 'display notification (item 3 of argv) with title (item 1 of argv) subtitle (item 2 of argv)' \
      -e 'end run' "$title" "$subtitle" "$message" >/dev/null 2>&1 &
  fi
}
