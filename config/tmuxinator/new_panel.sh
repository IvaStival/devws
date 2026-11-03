#!/usr/bin/env bash
# Opens a panel right of, left of, below or above the workspace's focused pane
# (New panel in the devws menu, C-b N). Prints the new pane's id.
#
#   new_panel.sh <session> <position> agent <name>   an extra agent: started as
#                                                    the main one is
#                                                    (agent_runner.sh --extra),
#                                                    with its own state
#   new_panel.sh <session> <position> terminal       a shell
#   new_panel.sh <session> <position> nvim           your Neovim (LazyVim) in
#                                                    the project, not linked to
#                                                    the agent
#
# <position> is right, left, below or above. Options after them:
#
#   --beside <pane>        split that pane (in this workspace) instead of the
#                          focused one
#   --args-file <file>     more arguments for an agent (agent_runner.sh)
#   --no-focus             leave the focus where it is
#
# A panel in the agent group's area (beside the terminal, say) minimizes and
# moves with the group; one by the editor stays where it is (extra_panes).
# A minimized group is expanded first, so nothing stays hidden. The workspace
# is saved afterwards, so a restore brings the panel back.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
session="${1:-}"
position="${2:-}"
kind="${3:-}"
[ "$#" -ge 3 ] && shift 3 || set --
name=""
case "${1:-}" in
  "" | --*) ;;
  *)
    name="$1"
    shift
    ;;
esac
beside=""
args_file=""
no_focus=""

fail() {
  tmux display-message "New panel: $*" 2>/dev/null
  printf 'New panel: %s\n' "$*" >&2
  exit 1
}

# shellcheck source=/dev/null
[ -r "$script_dir/.env" ] && . "$script_dir/.env"
# shellcheck source=agents.sh
. "$script_dir/agents.sh"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --beside | --args-file)
      [ "$#" -ge 2 ] || fail "$1 needs a value"
      if [ "$1" = --beside ]; then beside="$2"; else args_file="$2"; fi
      shift
      ;;
    --no-focus) no_focus=1 ;;
    *) fail "unknown option: $1" ;;
  esac
  shift
done

[ -n "$session" ] || fail "no workspace"
# -b puts the new pane before the focused one: left of it, or above it.
case "$position" in
  right) split=(-h) ;;
  left) split=(-h -b) ;;
  below) split=(-v) ;;
  above) split=(-v -b) ;;
  *) fail "unknown position: $position (right, left, below, above)" ;;
esac
case "$kind" in
  agent)
    agent_kind "$name" >/dev/null 2>&1 || fail "unknown agent: $name (register it in the devws menu)"
    command="$(printf '%q %q --extra' "$script_dir/agent_runner.sh" "$name")"
    [ -z "$args_file" ] || command="$command $(printf -- '--args-file %q' "$args_file")"
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
if [ -n "$beside" ]; then
  # tmux answers a pane that is gone with an empty line, and a pane of another
  # workspace would be split without anyone meaning to.
  [ "$(tmux display-message -p -t "$beside" '#{pane_id} #{session_name}' 2>/dev/null)" = "$beside $session" ] ||
    fail "$beside is not a pane of $session"
  focused="$beside"
# From the sidebar's menu the sidebar has the focus: the panel goes by the
# terminal rather than splitting the narrow sidebar.
elif [ "$(tmux display-message -p -t "$focused" '#{pane_title}' 2>/dev/null)" = devws-picker ]; then
  focused="$(tmux list-panes -t "=$session:" -F '#{pane_id} #{@devws_role}' 2>/dev/null |
    awk '$2 == "terminal" { print $1; exit }')"
  [ -n "$focused" ] || fail "no terminal pane to open it by"
fi
root="$(tmux show-options -qv -t "=$session:" @devws_root 2>/dev/null)"

# The role is set here as well as by agent_runner.sh, so the save below
# already sees it.
pane="$(tmux split-window ${no_focus:+-d} "${split[@]}" -t "$focused" -c "${root:-$PWD}" -P -F '#{pane_id}' ${command:+"$command"})" ||
  fail "could not open the panel"
tmux set-option -p -t "$pane" @devws_role "$role"
[ "$kind" = agent ] && tmux set-option -p -t "$pane" @devws_agent "$name"

"$script_dir/save_state.sh"
printf '%s\n' "$pane"
