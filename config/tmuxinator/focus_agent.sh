#!/usr/bin/env bash
# Takes a client to an agent pane of a workspace: <pane> when it is still
# there, else the workspace's main agent. A minimized agent group is restored
# first. Used by jump_agent.sh (C-b a) and by a clicked agent alert
# (agent_status.sh), which runs it outside tmux: tmux then finds the server
# through TMUX, set to the server's socket.
#
#   focus_agent.sh <session> [pane] [client]   without a client, the one
#                                               used last
#
# A workspace that has gone, or no client attached, leaves everything as it is.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
session="${1:-}"
pane="${2:-}"
client="${3:-}"

# shellcheck source=workspace_panes.sh
. "$script_dir/workspace_panes.sh"

[ -n "$session" ] && tmux has-session -t "=$session" 2>/dev/null || exit 0
if [ -z "$client" ]; then
  client="$(tmux list-clients -F '#{client_activity} #{client_name}' 2>/dev/null |
    sort -rn | awk 'NR == 1 { print $2 }')"
fi
[ -n "$client" ] || exit 0

"$script_dir/minimize_panes.sh" "$session" restore agents >/dev/null 2>&1
load_workspace_panes "$session"
[ -n "$pane" ] && printf '%s\n' "$workspace_panes" | cut -f 1 | grep -qxF -- "$pane" ||
  pane="$(pane_with_role agent)"

tmux switch-client -c "$client" -t "=$session" 2>/dev/null || exit 1
if [ -n "$pane" ]; then
  tmux select-window -t "$pane" 2>/dev/null
  tmux select-pane -t "$pane" 2>/dev/null
fi
exit 0
