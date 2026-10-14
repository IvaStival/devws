#!/usr/bin/env bash
# Turns following the agent in the editor off or on for a devws workspace:
# while it is off, the agent's edits no longer move the editor (the agent can
# still open files on request).
#
#   toggle_follow.sh [session]

set -u

session="${1:-$(tmux display-message -p '#{session_name}')}"
# Workspaces start with following on; an unset option counts as on.
if [ "$(tmux show-option -qv -t "$session" @devws_follow)" = "off" ]; then
  state=on
else
  state=off
fi
tmux set-option -t "$session" @devws_follow "$state"
tmux display-message -t "$session" "Following the agent in the editor: $state"
