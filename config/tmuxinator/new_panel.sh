#!/usr/bin/env bash
# Opens a panel beside or below the workspace's focused pane (New panel in the
# devws menu, C-b N). Prints the new pane's id.
#
#   new_panel.sh <session> <side|below> agent <name>   an extra agent: started
#                                                      as the main one is
#                                                      (agent_runner.sh --extra),
#                                                      with its own state
#   new_panel.sh <session> <side|below> terminal       a shell
#   new_panel.sh <session> <side|below> nvim           your Neovim (LazyVim) in
#                                                      the project, not linked
#                                                      to the agent
#
# A panel in the agent group's area (beside the terminal, say) minimizes and
# moves with the group; one beside the editor stays where it is (extra_panes).
# A minimized group is expanded first, so nothing stays hidden. The workspace
# is saved afterwards, so a restore brings the panel back.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
session="${1:-}"
position="${2:-}"
kind="${3:-}"
name="${4:-}"

fail() {
  tmux display-message "New panel: $*" 2>/dev/null
  printf 'New panel: %s\n' "$*" >&2
  exit 1
}

# shellcheck source=/dev/null
[ -r "$script_dir/.env" ] && . "$script_dir/.env"
# shellcheck source=agents.sh
. "$script_dir/agents.sh"

[ -n "$session" ] || fail "no workspace"
case "$position" in
  side) split=-h ;;
  below) split=-v ;;
  *) fail "unknown position: $position (side, below)" ;;
esac
case "$kind" in
  agent)
    agent_kind "$name" >/dev/null 2>&1 || fail "unknown agent: $name (register it in the devws menu)"
    command="$(printf '%q %q --extra' "$script_dir/agent_runner.sh" "$name")"
    role=extra-agent
    ;;
  terminal)
    command=""
    role=extra
    ;;
  nvim)
    command -v nvim >/dev/null 2>&1 || fail "Neovim is not installed"
    command="nvim ."
    role=extra-editor
    ;;
  *) fail "unknown kind: $kind (agent, terminal, nvim)" ;;
esac

# While minimized the group's panes live in a hidden window.
"$script_dir/minimize_panes.sh" "$session" restore agents >/dev/null ||
  fail "could not expand the agent group"
focused="$(tmux display-message -p -t "=$session:" '#{pane_id}' 2>/dev/null)"
[ -n "$focused" ] || fail "workspace $session not found"
# From the sidebar's menu the sidebar has the focus: the panel goes by the
# terminal rather than splitting the narrow sidebar.
if [ "$(tmux display-message -p -t "$focused" '#{pane_title}' 2>/dev/null)" = devws-picker ]; then
  focused="$(tmux list-panes -t "=$session:" -F '#{pane_id} #{@devws_role}' 2>/dev/null |
    awk '$2 == "terminal" { print $1; exit }')"
  [ -n "$focused" ] || fail "no terminal pane to open it by"
fi
root="$(tmux show-options -qv -t "=$session:" @devws_root 2>/dev/null)"

# The role is set here as well as by agent_runner.sh, so the save below
# already sees it.
pane="$(tmux split-window "$split" -t "$focused" -c "${root:-$PWD}" -P -F '#{pane_id}' ${command:+"$command"})" ||
  fail "could not open the panel"
tmux set-option -p -t "$pane" @devws_role "$role"
[ "$kind" = agent ] && tmux set-option -p -t "$pane" @devws_agent "$name"

"$script_dir/save_state.sh"
printf '%s\n' "$pane"
