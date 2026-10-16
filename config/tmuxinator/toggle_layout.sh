#!/usr/bin/env bash
# Flip a devws workspace between its two arrangements:
#   stacked (default): editor on top, agent and terminal side by side below.
#   side:              editor full height, agent above terminal on the right.
# An open database pane follows: right of the editor when stacked, below it
# in the side layout.
#
# Panes are moved with join-pane rather than by applying a layout string, so
# the editor, agent and shell keep running, and it works whatever the number of
# panes (picker closed, extra terminals open).

set -u

script_dir="$(dirname "$0")"
session="${1:-$(tmux display-message -p '#S' 2>/dev/null)}"

fail() {
  tmux display-message "Toggle layout: $*" 2>/dev/null
  printf 'Toggle layout: %s\n' "$*" >&2
  exit 1
}

[ -n "$session" ] || fail "no tmux session"

. "$script_dir/workspace_panes.sh"

# Minimized panes are expanded first: while minimized the real panes live in a
# hidden window, and the toggle should produce full-size panes anyway.
"$script_dir/minimize_panes.sh" "$session" restore all ||
  fail "could not restore minimized panes"

load_workspace_panes "$session" || fail "session $session not found"
editor="$(pane_with_role editor)"
agent="$(pane_with_role agent)"
terminal="$(pane_with_role terminal)"
[ -n "$editor" ] || fail "editor pane not found"
# The pane leading the agent group: the agent, else the terminal (an agent
# started by hand there, or a closed agent pane). It goes first; the other,
# when there is one, follows it.
lead="$(agent_slot_pane)" || fail "no agent or terminal pane"
follower=""
[ -n "$agent" ] && follower="$terminal"
extras="$(extra_panes)"
database="$(pane_with_role database)"

move_pane() {
  tmux join-pane -d "$@" 2>/dev/null || fail "could not move pane ($*)"
}

# Extras sit beside the terminal in both arrangements. They are chained after
# one another to keep their order, and all must leave the old row/column so
# tmux collapses it and the neighbouring pane takes over the space.
move_extras_beside_terminal() {
  local previous="${follower:-$lead}"
  local extra
  for extra in $extras; do
    move_pane -h -s "$extra" -t "$previous"
    previous="$extra"
  done
}

was_side_layout=0
is_side_layout "$editor" "$lead" && was_side_layout=1

if [ "$was_side_layout" = 1 ]; then
  move_pane -v -l 50% -s "$lead" -t "$editor"
  [ -z "$follower" ] || move_pane -h -l 50% -s "$follower" -t "$lead"
else
  move_pane -h -l 35% -s "$lead" -t "$editor"
  [ -z "$follower" ] || move_pane -v -l 50% -s "$follower" -t "$lead"
fi
move_extras_beside_terminal

# Moved last, once agent and terminal have left the editor's row or column.
if [ -n "$database" ]; then
  if [ "$was_side_layout" = 1 ]; then
    move_pane -h -l 40% -s "$database" -t "$editor"
  else
    move_pane -v -l 35% -s "$database" -t "$editor"
  fi
fi

if [ -x "$script_dir/save_state.sh" ]; then
  "$script_dir/save_state.sh"
fi
