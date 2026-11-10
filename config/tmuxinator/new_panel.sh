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
#   --label <text>         the panel's header says it instead of "Agent"
#                          (Backend · claude)
#   --place team           the team area of an agent team (devws agentic): the
#                          first panel takes the right half of the window, full
#                          height; the next ones split the area into a grid.
#                          <position> is ignored. Only for agents.
#   --group <name>         an agent team's group: a workspace of its own
#                          beside this one (dev-app-floor), four agents to a
#                          group, the next ones in "<name> 2"
#                          (team_groups.sh). <position> is ignored. Only for
#                          agents.
#   --folder <dir>         start it in that folder instead of the workspace's
#                          (an agent team's lane: a worktree of its own)
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
label=""
place=""
group=""
folder=""
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
    --beside | --args-file | --label | --place | --group | --folder)
      [ "$#" -ge 2 ] || fail "$1 needs a value"
      case "$1" in
        --beside) beside="$2" ;;
        --args-file) args_file="$2" ;;
        --label) label="$2" ;;
        --place) place="$2" ;;
        --group) group="$2" ;;
        --folder) folder="$2" ;;
      esac
      shift
      ;;
    --no-focus) no_focus=1 ;;
    *) fail "unknown option: $1" ;;
  esac
  shift
done

[ -n "$session" ] || fail "no workspace"
case "$place" in
  "" | team) ;;
  *) fail "unknown place: $place (team)" ;;
esac
[ -z "$place" ] || [ "$kind" = agent ] || fail "--place $place is for agents"
[ -z "$place" ] || [ -z "$beside" ] || fail "--place and --beside both say where it goes"
[ -z "$group" ] || [ "$kind" = agent ] || fail "--group is for agents"
[ -z "$group" ] || [ -z "$place$beside" ] || fail "--group and --place or --beside both say where it goes"
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
if [ -n "$group" ]; then
  # A group is a workspace of its own: no pane here to split by.
  :
elif [ -n "$place" ]; then
  # The team area is placed by the window, not by the focus (which is on the
  # sidebar after a workspace opens): any pane but the sidebar is the target.
  focused="$(tmux list-panes -t "=$session:" -F '#{pane_id} #{pane_title}' 2>/dev/null |
    awk '$2 != "devws-picker" { print $1; exit }')"
  [ -n "$focused" ] || fail "no pane to place the team by"
elif [ -n "$beside" ]; then
  # tmux answers a pane that is gone with an empty line, and a pane of another
  # workspace would be split without anyone meaning to.
  [ "$(tmux display-message -p -t "$beside" '#{pane_id} #{session_name}' 2>/dev/null)" = "$beside $session" ] ||
    fail "$beside is not a pane of $session"
  focused="$beside"
# From the sidebar's menu the sidebar has the focus: the panel goes by the
# terminal rather than splitting the narrow sidebar. An agent team's
# workspaces have no terminal: then any pane but the sidebar.
elif [ "$(tmux display-message -p -t "$focused" '#{pane_title}' 2>/dev/null)" = devws-picker ]; then
  focused="$(tmux list-panes -t "=$session:" -F '#{pane_id}	#{@devws_role}	#{pane_title}' 2>/dev/null |
    awk -F '\t' '$2 == "terminal" { print $1; exit }
      $3 != "devws-picker" && other == "" { other = $1 }
      END { if (other != "") print other }' | head -n 1)"
  [ -n "$focused" ] || fail "no pane but the sidebar to open it by"
fi
root="$(tmux show-options -qv -t "=$session:" @devws_root 2>/dev/null)"
if [ -n "$folder" ]; then
  [ -d "$folder" ] || fail "no folder $folder"
  root="$folder"
fi

# The role is set here as well as by agent_runner.sh, so the save below
# already sees it.
if [ -n "$group" ]; then
  pane="$("$script_dir/team_groups.sh" open "$session" "$group" "${root:-$PWD}" "$command")" ||
    fail "could not open the panel in the group $group"
elif [ "$place" = team ]; then
  # The team area is the panels marked team. The first spans the window's
  # height at the right, half of what the sidebar leaves; the next ones split
  # the largest panel in two: side by side when it is at least twice as wide
  # as tall (a terminal cell is about twice as tall as wide), else one above
  # the other.
  team_panel="$(tmux list-panes -t "=$session:" -F '#{pane_id} #{@devws_place} #{pane_width} #{pane_height}' 2>/dev/null |
    awk '$2 == "team" && $3 * $4 > best { best = $3 * $4; id = $1; wide = ($3 >= 2 * $4) }
      END { if (id != "") print id, wide ? "-h" : "-v" }')"
  # With no agent yet, the first goes under the board (the pane marked
  # anchor), when the workspace has one: most of the column, the board keeps
  # the top.
  anchor="$(tmux list-panes -t "=$session:" -F '#{pane_id} #{@devws_place}' 2>/dev/null |
    awk '$2 == "anchor" { print $1; exit }')"
  if [ -z "$team_panel" ] && [ -n "$anchor" ]; then
    split=(-d -v -l 60%)
    split_target="$anchor"
  elif [ -z "$team_panel" ]; then
    window_width="$(tmux display-message -p -t "=$session:" '#{window_width}' 2>/dev/null)"
    read -r sidebar_pane sidebar_width <<< "$(tmux list-panes -t "=$session:" -F '#{pane_title} #{pane_id} #{pane_width}' 2>/dev/null |
      awk '$1 == "devws-picker" { print $2, $3; exit }')"
    split=(-d -f -h -l $(( (window_width - ${sidebar_width:-0}) / 2 )))
    split_target="$focused"
  else
    split=(-d "${team_panel#* }")
    split_target="${team_panel%% *}"
  fi
  pane="$(tmux split-window "${split[@]}" -t "$split_target" -c "${root:-$PWD}" -P -F '#{pane_id}' ${command:+"$command"})" ||
    fail "could not open the panel"
  tmux set-option -p -t "$pane" @devws_place team
  # A full-height split scales every pane, the sidebar too: it keeps its width.
  [ -z "${sidebar_pane:-}" ] || tmux resize-pane -t "$sidebar_pane" -x "$sidebar_width"
else
  pane="$(tmux split-window ${no_focus:+-d} "${split[@]}" -t "$focused" -c "${root:-$PWD}" -P -F '#{pane_id}' ${command:+"$command"})" ||
    fail "could not open the panel"
fi
tmux set-option -p -t "$pane" @devws_role "$role"
[ "$kind" = agent ] && tmux set-option -p -t "$pane" @devws_agent "$name"
[ -z "$label" ] || tmux set-option -p -t "$pane" @devws_label "$label"

"$script_dir/save_state.sh"
printf '%s\n' "$pane"
