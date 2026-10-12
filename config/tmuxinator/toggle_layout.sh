#!/usr/bin/env bash
# Flip a devws workspace between its two arrangements:
#   stacked (default): editor on top, agent and terminal side by side below.
#   side:              editor full height, agent above terminal on the right.
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

# Workspaces created before the terminal pane marked itself only carry the
# role in the pane title, so the title is accepted as a fallback for it.
panes="$(
  tmux list-panes -t "$session:" \
    -F '#{pane_id}|#{@devws_role}|#{pane_title}|#{pane_top}|#{pane_left}' \
    2>/dev/null
)" || fail "session $session not found"

pane_with_role() {
  printf '%s\n' "$panes" | awk -F '|' -v role="$1" '
    $2 == role { print $1; exit }
    $3 == role && fallback == "" { fallback = $1 }
    END { if (fallback != "") print fallback }'
}

pane_field() {
  printf '%s\n' "$panes" | awk -F '|' -v id="$1" -v field="$2" '
    $1 == id { print $field; exit }'
}

editor="$(pane_with_role editor)"
agent="$(pane_with_role agent)"
terminal="$(pane_with_role terminal)"
[ -n "$editor" ] || fail "editor pane not found"
[ -n "$agent" ] || fail "agent pane not found"
[ -n "$terminal" ] || fail "terminal pane not found"
extras="$(printf '%s\n' "$panes" | awk -F '|' '$2 == "extra" { print $1 }')"

move_pane() {
  tmux join-pane -d "$@" 2>/dev/null || fail "could not move pane ($*)"
}

# Extras sit beside the terminal in both arrangements. They are chained after
# one another to keep their order, and all must leave the old row/column so
# tmux collapses it and the neighbouring pane takes over the space.
move_extras_beside_terminal() {
  local previous="$terminal"
  local extra
  for extra in $extras; do
    move_pane -h -s "$extra" -t "$previous"
    previous="$extra"
  done
}

is_side_layout() {
  [ "$(pane_field "$agent" 4)" = "$(pane_field "$editor" 4)" ] &&
    [ "$(pane_field "$agent" 5)" -gt "$(pane_field "$editor" 5)" ]
}

if is_side_layout; then
  move_pane -v -l 50% -s "$agent" -t "$editor"
  move_pane -h -l 50% -s "$terminal" -t "$agent"
else
  move_pane -h -l 35% -s "$agent" -t "$editor"
  move_pane -v -l 50% -s "$terminal" -t "$agent"
fi
move_extras_beside_terminal

if [ -x "$script_dir/save_state.sh" ]; then
  "$script_dir/save_state.sh"
fi
