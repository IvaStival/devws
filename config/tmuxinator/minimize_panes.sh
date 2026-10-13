#!/usr/bin/env bash
# Collapse a devws workspace's agent and terminal panes to small bars so the
# editor gets the space, or expand them again.
#
#   minimize_panes.sh [session] [toggle|restore]
#
# Minimizing swaps each agent/terminal pane with a placeholder that draws a
# short notice, and keeps the real panes running in a hidden window; a bar of
# the real pane would only show a squeezed line of its program.

set -u

script_dir="$(dirname "$0")"
session="${1:-$(tmux display-message -p '#S' 2>/dev/null)}"
action="${2:-toggle}"

# Just wide enough for the "▸ Agent" header in the full-height-editor layout.
strip_width=9
hidden_window_name="devws-minimized"

fail() {
  tmux display-message "Minimize panes: $*" 2>/dev/null
  printf 'Minimize panes: %s\n' "$*" >&2
  exit 1
}

[ -n "$session" ] || fail "no tmux session"
case "$action" in
  toggle|restore) ;;
  *) fail "unknown action: $action" ;;
esac

. "$script_dir/workspace_panes.sh"

load_workspace_panes "$session" || fail "session $session not found"
editor="$(pane_with_role editor)"
agent="$(pane_with_role agent)"
[ -n "$agent" ] || agent="$(pane_with_role minimized-agent)"
[ -n "$editor" ] || fail "editor pane not found"
[ -n "$agent" ] || fail "agent pane not found"

placeholders="$(
  printf '%s\n' "$workspace_panes" | awk -F '|' '$2 ~ /^minimized-/ { print $1 }'
)"

# A workspace restored after a reboot keeps its minimized sizes but has no
# placeholders; it is still treated as minimized so it can be expanded.
is_minimized() {
  [ -n "$placeholders" ] && return 0
  if is_side_layout "$editor" "$agent"; then
    [ "$(pane_field "$agent" 6)" -le "$strip_width" ]
  else
    [ "$(pane_field "$agent" 7)" -le 1 ]
  fi
}

# A minimized pane is too small to work in, so focus moves to the editor.
move_focus_to_editor() {
  case "$(printf '%s\n' "$workspace_panes" | awk -F '|' -v id="$(active_pane)" '$1 == id { print $2 }')" in
    agent|terminal|extra) tmux select-pane -t "$editor" ;;
  esac
}

placeholder_command() {
  printf "'%s' '%s'" "$script_dir/minimized_placeholder.sh" "$1"
}

# Creates a placeholder beside the hidden window's first pane, or, for the
# first one, splits it off the real pane and breaks it out into a new hidden
# window: new-window would trigger tmux.conf's rename prompt.
create_placeholder() {
  local real_pane="$1"
  local role="$2"
  local label="$3"
  local placeholder

  if [ -z "$hidden_window" ]; then
    placeholder="$(tmux split-window -d -t "$real_pane" -c "#{pane_current_path}" \
      -P -F '#{pane_id}' "$(placeholder_command "$label")")" || return 1
    hidden_window="$(tmux break-pane -d -s "$placeholder" -n "$hidden_window_name" \
      -P -F '#{window_id}')" || return 1
  else
    placeholder="$(tmux split-window -d -t "$hidden_window" -c "#{pane_current_path}" \
      -P -F '#{pane_id}' "$(placeholder_command "$label")")" || return 1
  fi
  tmux set-option -p -t "$placeholder" @devws_role "minimized-$role"
  tmux set-option -p -t "$placeholder" @devws_hidden_pane "$real_pane"
  tmux swap-pane -d -s "$real_pane" -t "$placeholder"
}

minimize() {
  local is_side=0 agent_name terminal extra
  is_side_layout "$editor" "$agent" && is_side=1
  agent_name="$(tmux show-options -qv -t "$session" @devws_agent)"
  terminal="$(pane_with_role terminal)"

  tmux set-option -w -t "$session:" @devws_expanded_layout \
    "$(tmux display-message -p -t "$session:" '#{window_layout}')"
  move_focus_to_editor

  hidden_window=""
  create_placeholder "$agent" agent "Agent · ${agent_name:-agent}" ||
    fail "could not hide the agent pane"
  tmux set-option -w -t "$session:" @devws_hidden_window "$hidden_window"
  if [ -n "$terminal" ]; then
    create_placeholder "$terminal" terminal "Terminal" ||
      fail "could not hide the terminal pane"
  fi
  for extra in $(extra_panes); do
    create_placeholder "$extra" extra "Terminal" ||
      fail "could not hide a terminal pane"
  done

  # The agent's placeholder now sits in the agent's slot.
  if [ "$is_side" = 1 ]; then
    tmux resize-pane -t "$(placeholder_for "$agent")" -x "$strip_width"
  else
    tmux resize-pane -t "$(placeholder_for "$agent")" -y 1
  fi
}

placeholder_for() {
  tmux list-panes -t "$session:" -F '#{pane_id} #{@devws_hidden_pane}' |
    awk -v real="$1" '$2 == real { print $1; exit }'
}

# The sizes from before minimizing come back when the panes are unchanged;
# select-layout refuses a layout whose pane count differs, and then the
# layout's default proportions are used instead.
restore() {
  local placeholder real_pane saved_layout hidden

  for placeholder in $placeholders; do
    real_pane="$(tmux show-options -pqv -t "$placeholder" @devws_hidden_pane)"
    tmux swap-pane -d -s "$real_pane" -t "$placeholder" ||
      fail "could not bring back pane $real_pane"
  done
  hidden="$(tmux show-options -wqv -t "$session:" @devws_hidden_window)"
  tmux set-option -wu -t "$session:" @devws_hidden_window
  [ -n "$hidden" ] && tmux kill-window -t "$hidden" 2>/dev/null

  load_workspace_panes "$session" || fail "session $session not found"
  agent="$(pane_with_role agent)"
  saved_layout="$(tmux show-options -wqv -t "$session:" @devws_expanded_layout)"
  tmux set-option -wu -t "$session:" @devws_expanded_layout
  if [ -n "$saved_layout" ] &&
      tmux select-layout -t "$session:" "$saved_layout" 2>/dev/null; then
    return
  fi
  if is_side_layout "$editor" "$agent"; then
    tmux resize-pane -t "$agent" -x 35%
  else
    tmux resize-pane -t "$agent" -y 50%
  fi
}

if is_minimized; then
  restore
elif [ "$action" = toggle ]; then
  minimize
fi
