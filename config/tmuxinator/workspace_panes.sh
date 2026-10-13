#!/usr/bin/env bash
# Pane lookups shared by the devws workspace scripts. Source this file, then
# call load_workspace_panes with a session name before using the helpers.

# Workspaces created before the terminal pane marked itself only carry the
# role in the pane title, so the title is accepted as a fallback for it.
load_workspace_panes() {
  workspace_panes="$(
    tmux list-panes -t "$1:" \
      -F '#{pane_id}|#{@devws_role}|#{pane_title}|#{pane_top}|#{pane_left}|#{pane_width}|#{pane_height}|#{pane_active}' \
      2>/dev/null
  )"
}

pane_with_role() {
  printf '%s\n' "$workspace_panes" | awk -F '|' -v role="$1" '
    $2 == role { print $1; exit }
    $3 == role && fallback == "" { fallback = $1 }
    END { if (fallback != "") print fallback }'
}

extra_panes() {
  printf '%s\n' "$workspace_panes" | awk -F '|' '$2 == "extra" { print $1 }'
}

# Fields: 4 top, 5 left, 6 width, 7 height, 8 active.
pane_field() {
  printf '%s\n' "$workspace_panes" | awk -F '|' -v id="$1" -v field="$2" '
    $1 == id { print $field; exit }'
}

active_pane() {
  printf '%s\n' "$workspace_panes" | awk -F '|' '$8 == 1 { print $1; exit }'
}

# Side layout: the agent sits at the editor's top, to its right.
is_side_layout() {
  local editor="$1"
  local agent="$2"
  [ "$(pane_field "$agent" 4)" = "$(pane_field "$editor" 4)" ] &&
    [ "$(pane_field "$agent" 5)" -gt "$(pane_field "$editor" 5)" ]
}
