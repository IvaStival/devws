#!/usr/bin/env bash
# Pane lookups shared by the devws workspace scripts. Source this file, then
# call load_workspace_panes with a session name before using the helpers.

# Workspaces created before the terminal pane marked itself only carry the
# role in the pane title, so the title is accepted as a fallback for it.
# Fields are tab-separated: a pane title can hold any other character (Codex
# names its pane after the task, "Improve the guide | Parla").
load_workspace_panes() {
  local tab=$'\t'
  workspace_panes="$(
    tmux list-panes -t "$1:" \
      -F "#{pane_id}${tab}#{@devws_role}${tab}#{pane_title}${tab}#{pane_top}${tab}#{pane_left}${tab}#{pane_width}${tab}#{pane_height}${tab}#{pane_active}" \
      2>/dev/null
  )"
}

pane_with_role() {
  printf '%s\n' "$workspace_panes" | awk -F '\t' -v role="$1" '
    $2 == role { print $1; exit }
    $3 == role && fallback == "" { fallback = $1 }
    END { if (fallback != "") print fallback }'
}

# The agent group's extra panels, which travel with it: the panes inside the
# group's area — from the agent column's left edge right (side layout), or
# from the agent row's top down (stacked) — that New panel opened (roles
# "extra", "extra-agent", "extra-editor") or that have no devws role (a shell
# split off the terminal by hand). A panel beside or below the editor (New
# panel there, or a Markdown viewer, C-b %) stays out. Without an agent slot
# to tell the area, New panel's ones all count. Not the sidebar, nor a pane
# that only carries its role in its title.
extra_panes() {
  local editor slot side=0 edge
  editor="$(pane_with_role editor)"
  slot="$(agent_slot_pane)"
  if [ -n "$editor" ] && [ -n "$slot" ]; then
    is_side_layout "$editor" "$slot" && side=1
    if [ "$side" = 1 ]; then
      edge="$(pane_field "$slot" 5)"
    else
      edge="$(pane_field "$slot" 4)"
    fi
  fi
  printf '%s\n' "$workspace_panes" | awk -F '\t' -v side="$side" -v edge="${edge:-}" '
    $2 ~ /^extra(-agent|-editor)?$/ && edge == "" { print $1; next }
    $2 !~ /^(extra(-agent|-editor)?)?$/ || ($2 == "" && $3 ~ /^(devws-picker|editor|agent|terminal|database)$/) || edge == "" { next }
    side == 1 && $5 + 0 >= edge + 0 { print $1; next }
    side == 0 && $4 + 0 >= edge + 0 { print $1 }'
}

# Fields: 4 top, 5 left, 6 width, 7 height, 8 active.
pane_field() {
  printf '%s\n' "$workspace_panes" | awk -F '\t' -v id="$1" -v field="$2" '
    $1 == id { print $field; exit }'
}

active_pane() {
  printf '%s\n' "$workspace_panes" | awk -F '\t' '$8 == 1 { print $1; exit }'
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
