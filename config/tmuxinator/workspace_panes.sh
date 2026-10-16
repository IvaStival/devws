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

# Extra terminals, which travel with the agent group: those devws opened
# (role "extra"), and panes with no devws role, such as a shell split off by
# hand. Not the sidebar, nor a pane that only carries its role in its title.
extra_panes() {
  printf '%s\n' "$workspace_panes" | awk -F '|' '
    $2 == "extra" { print $1 }
    $2 == "" && $3 !~ /^(devws-picker|editor|agent|terminal|database)$/ { print $1 }'
}

# Fields: 4 top, 5 left, 6 width, 7 height, 8 active.
pane_field() {
  printf '%s\n' "$workspace_panes" | awk -F '|' -v id="$1" -v field="$2" '
    $1 == id { print $field; exit }'
}

active_pane() {
  printf '%s\n' "$workspace_panes" | awk -F '|' '$8 == 1 { print $1; exit }'
}

# The pane in the agent's slot, which tells the layout and leads the agent
# group: the agent, else the terminal — a workspace whose agent pane was
# closed, or whose agent was started by hand in the terminal. A minimized pane
# counts as its placeholder, which keeps the slot.
agent_slot_pane() {
  local role pane
  for role in agent minimized-agent terminal minimized-terminal; do
    pane="$(pane_with_role "$role")"
    if [ -n "$pane" ]; then
      printf '%s' "$pane"
      return 0
    fi
  done
  return 1
}

# Side layout: the agent sits at the editor's top, to its right.
is_side_layout() {
  local editor="$1"
  local agent="$2"
  [ "$(pane_field "$agent" 4)" = "$(pane_field "$editor" 4)" ] &&
    [ "$(pane_field "$agent" 5)" -gt "$(pane_field "$editor" 5)" ]
}
