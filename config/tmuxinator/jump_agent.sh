#!/usr/bin/env bash
# Takes a client to the agent that needs the user (C-b a, or "Go to waiting
# agent" in the devws menu): the workspace whose agent is waiting, the one
# that has waited longest first, else one whose agent is done. Pressed again,
# it goes on to the next one, wrapping round. The agent pane in that state is
# focused — the main agent or one New panel added — its group restored first
# when minimized.
#
#   jump_agent.sh [client]
#
# The order comes from agent_status.sh's @devws_agent_state_since.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
client="${1:-}"

# shellcheck source=workspace_panes.sh
. "$script_dir/workspace_panes.sh"

message() {
  tmux display-message ${client:+-c "$client"} "$1" 2>/dev/null
}

# The workspaces in <state>, the longest in it first.
workspaces_in() {
  tmux list-sessions -F '#{session_name}|#{@devws_agent_state}|#{@devws_agent_state_since}' 2>/dev/null |
    awk -F '|' -v state="$1" '$1 ~ /^dev-/ && $2 == state { print ($3 == "" ? 0 : $3) "|" $1 }' |
    sort -t '|' -k1,1n -k2,2 | cut -d '|' -f 2-
}

# The one after <current> in <list>, wrapping round; the first when <current>
# is not in it.
next_after() {
  local current="$1" list="$2"
  printf '%s\n' "$list" | awk -v current="$current" '
    NR == 1 { first = $0 }
    found { print; printed = 1; exit }
    $0 == current { found = 1 }
    END { if (!printed) print first }'
}

current="$(tmux display-message ${client:+-c "$client"} -p '#{client_session}' 2>/dev/null)"
wanted=waiting
candidates="$(workspaces_in waiting)"
if [ -z "$candidates" ]; then
  wanted=done
  candidates="$(workspaces_in done)"
fi
if [ -z "$candidates" ]; then
  message "No agent needs you"
  exit 0
fi
target="$(next_after "$current" "$candidates")"

"$script_dir/minimize_panes.sh" "$target" restore agents >/dev/null 2>&1
load_workspace_panes "$target"
# The agent pane in the wanted state, else the main agent.
agent="$(tmux list-panes -t "=$target:" -F '#{pane_id} #{@devws_role} #{@devws_pane_state}' 2>/dev/null |
  awk -v wanted="$wanted" '$2 ~ /^(agent|extra-agent)$/ && $3 == wanted { print $1; exit }')"
[ -n "$agent" ] || agent="$(pane_with_role agent)"

tmux switch-client ${client:+-c "$client"} -t "=$target" 2>/dev/null || {
  message "Could not switch to $target"
  exit 1
}
if [ -n "$agent" ]; then
  tmux select-window -t "$agent" 2>/dev/null
  tmux select-pane -t "$agent" 2>/dev/null
fi
exit 0
