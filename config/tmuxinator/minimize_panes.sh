#!/usr/bin/env bash
# Collapse a group of devws workspace panes to small bars so the editor gets
# the space, or expand them again.
#
#   minimize_panes.sh [session] [toggle|restore] [agents|database|all]
#
# Groups: "agents" is the agent, terminal and extra terminals; "database" is
# the database pane. Each hides on its own; "all" is only for restore.
#
# Minimizing swaps each pane with a placeholder that draws a short notice, and
# keeps the real panes running in a hidden window; a bar of the real pane would
# only show a squeezed line of its program.

set -u

script_dir="$(dirname "$0")"
session="${1:-$(tmux display-message -p '#S' 2>/dev/null)}"
action="${2:-toggle}"
group="${3:-agents}"

# Just wide enough for the "▸ Agent" header when a group sits beside the editor.
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
case "$group" in
  agents|database) ;;
  all) [ "$action" = restore ] || fail "only restore applies to all groups" ;;
  *) fail "unknown group: $group" ;;
esac

. "$script_dir/workspace_panes.sh"

load_workspace_panes "$session" || fail "session $session not found"
editor="$(pane_with_role editor)"
[ -n "$editor" ] || fail "editor pane not found"

# The pane that decides a group's slot and size: the agent, or the database.
# Without an agent pane (an agent started by hand in the terminal, or a closed
# agent pane), the terminal leads the agent group.
lead_role() {
  case "$1" in
    agents)
      if [ -n "$(pane_with_role agent)$(pane_with_role minimized-agent)" ]; then
        printf 'agent'
      else
        printf 'terminal'
      fi
      ;;
    database) printf 'database' ;;
  esac
}

role_of() {
  printf '%s\n' "$workspace_panes" | awk -F '\t' -v id="$1" '$1 == id { print $2 }'
}

# A group beside the editor (same top row) shrinks in width; one below it
# shrinks in height.
is_beside_editor() {
  [ "$(pane_field "$1" 4)" = "$(pane_field "$editor" 4)" ]
}

# Also true for a workspace restored after a reboot, which keeps its
# minimized sizes but has no placeholders, so it can still be expanded.
is_minimized() {
  local lead
  [ -n "$(pane_with_role "minimized-$(lead_role "$1")")" ] && return 0
  lead="$(pane_with_role "$(lead_role "$1")")"
  [ -n "$lead" ] || return 1
  if is_beside_editor "$lead"; then
    [ "$(pane_field "$lead" 6)" -le "$strip_width" ]
  else
    [ "$(pane_field "$lead" 7)" -le 1 ]
  fi
}

group_members() {
  case "$1" in
    agents)
      pane_with_role agent
      pane_with_role terminal
      extra_panes
      ;;
    database) pane_with_role database ;;
  esac
}

group_placeholders() {
  case "$1" in
    agents) printf '%s\n' "$workspace_panes" |
      awk -F '\t' '$2 ~ /^minimized-(agent|terminal|extra|extra-agent|extra-editor)$/ { print $1 }' ;;
    database) pane_with_role minimized-database ;;
  esac
}

# <role> <pane>: an extra agent is named by its own pane's agent.
placeholder_label() {
  case "$1" in
    agent) printf 'Agent · %s' "$(tmux show-options -qv -t "$session" @devws_agent)" ;;
    extra-agent) printf 'Agent · %s' "$(tmux show-options -pqv -t "$2" @devws_agent)" ;;
    extra-editor) printf 'Neovim' ;;
    terminal|extra) printf 'Terminal' ;;
    database) printf 'Database' ;;
  esac
}

placeholder_command() {
  printf '%q %q' "$script_dir/minimized_placeholder.sh" "$1"
}

hidden_window() {
  local window
  window="$(tmux show-options -wqv -t "$session:" @devws_hidden_window)"
  [ -n "$window" ] && tmux list-panes -t "$window" >/dev/null 2>&1 && printf '%s' "$window"
}

# Creates the placeholder in the hidden window and swaps it with the real
# pane. The hidden window itself is made by splitting a placeholder off the
# real pane and breaking it out, which keeps the window in this session.
hide_pane() {
  local real_pane="$1"
  local role placeholder window
  role="$(role_of "$real_pane")"
  # A pane with no devws role (a shell split off by hand) goes as an extra
  # terminal, so restore finds its placeholder.
  role="${role:-extra}"
  window="$(hidden_window)"

  if [ -z "$window" ]; then
    placeholder="$(tmux split-window -d -t "$real_pane" -c "#{pane_current_path}" \
      -P -F '#{pane_id}' "$(placeholder_command "$(placeholder_label "$role" "$real_pane")")")" || return 1
    # -a -t places the window in this session; without a target tmux uses
    # the most recently active session, which may be another workspace.
    window="$(tmux break-pane -d -a -s "$placeholder" -t "$session:" \
      -n "$hidden_window_name" -P -F '#{window_id}')" || return 1
    tmux set-option -w -t "$session:" @devws_hidden_window "$window"
  else
    placeholder="$(tmux split-window -d -t "$window" -c "#{pane_current_path}" \
      -P -F '#{pane_id}' "$(placeholder_command "$(placeholder_label "$role" "$real_pane")")")" || return 1
  fi
  tmux set-option -p -t "$placeholder" @devws_role "minimized-$role"
  tmux set-option -p -t "$placeholder" @devws_hidden_pane "$real_pane"
  # An extra agent's name goes along, so saving a minimized workspace keeps it.
  [ "$role" = extra-agent ] &&
    tmux set-option -p -t "$placeholder" @devws_agent "$(tmux show-options -pqv -t "$real_pane" @devws_agent)"
  tmux swap-pane -d -s "$real_pane" -t "$placeholder"
}

minimize() {
  local lead size_option member
  lead="$(pane_with_role "$(lead_role "$group")")"
  [ -n "$lead" ] || fail "no $(lead_role "$group") pane to minimize"

  # The slot's size is kept so restore brings it back without touching other
  # groups.
  if is_beside_editor "$lead"; then
    size_option="x:$(pane_field "$lead" 6)"
  else
    size_option="y:$(pane_field "$lead" 7)"
  fi
  tmux set-option -w -t "$session:" "@devws_restore_$group" "$size_option"

  # A minimized pane is too small to work in, so focus moves to the editor.
  group_members "$group" | grep -qx "$(active_pane)" && tmux select-pane -t "$editor"

  for member in $(group_members "$group"); do
    hide_pane "$member" || fail "could not hide pane $member"
  done

  # The lead's placeholder now sits in the lead's slot.
  load_workspace_panes "$session" || fail "session $session not found"
  lead="$(pane_with_role "minimized-$(lead_role "$group")")"
  if [ "${size_option%%:*}" = x ]; then
    tmux resize-pane -t "$lead" -x "$strip_width"
  else
    tmux resize-pane -t "$lead" -y 1
  fi
}

restore_group() {
  local restore_group="$1"
  local placeholder real_pane size lead

  for placeholder in $(group_placeholders "$restore_group"); do
    real_pane="$(tmux show-options -pqv -t "$placeholder" @devws_hidden_pane)"
    tmux swap-pane -d -s "$real_pane" -t "$placeholder" ||
      fail "could not bring back pane $real_pane"
    # Swapped into the hidden window; the window closes with its last pane.
    tmux kill-pane -t "$placeholder"
  done
  [ -n "$(hidden_window)" ] || tmux set-option -wu -t "$session:" @devws_hidden_window

  load_workspace_panes "$session" || fail "session $session not found"
  lead="$(pane_with_role "$(lead_role "$restore_group")")"
  [ -n "$lead" ] || return 0
  size="$(tmux show-options -wqv -t "$session:" "@devws_restore_$restore_group")"
  tmux set-option -wu -t "$session:" "@devws_restore_$restore_group"
  if [ -z "$size" ]; then
    # No recorded size, e.g. after a reboot: use the layout's proportions.
    if is_beside_editor "$lead"; then
      [ "$restore_group" = agents ] && size="x:35%" || size="x:40%"
    else
      [ "$restore_group" = agents ] && size="y:50%" || size="y:35%"
    fi
  fi
  tmux resize-pane -t "$lead" "-${size%%:*}" "${size#*:}"
}

restore_if_minimized() {
  if is_minimized "$1"; then
    restore_group "$1"
    load_workspace_panes "$session" || fail "session $session not found"
  fi
}

case "$group:$action" in
  all:restore)
    restore_if_minimized agents
    restore_if_minimized database
    ;;
  *:restore) restore_if_minimized "$group" ;;
  *:toggle)
    if is_minimized "$group"; then
      restore_group "$group"
    else
      minimize
    fi
    ;;
esac
