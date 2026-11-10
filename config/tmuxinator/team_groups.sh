#!/usr/bin/env bash
# An agent team's groups: workspaces of their own beside the team's workspace,
# which keeps the leader and the board. A group has a name (Seniors, Floor)
# and holds a few agents; when it is full the next is "<name> 2". Its
# workspace is the team workspace's name and the group's (dev-app-floor,
# dev-app-floor-2), with the same task, so the sidebar lists it under the task.
#
#   team_groups.sh open <session> <group> <folder> [command]
#                         a new agent panel of <session>'s team, in the group,
#                         running command in folder; prints its pane id. The
#                         focus stays where it is.
#   team_groups.sh place <pane> [group]
#                         puts a pane in its group (the one it was opened in, or
#                         this one)
#   team_groups.sh go <pane>
#                         shows the pane: its group's workspace, focused on it
#   team_groups.sh tidy <session>
#                         a group workspace's agents tiled beside its sidebar,
#                         or, with none left, the workspace closed (devws agent
#                         stop runs it)
#   team_groups.sh peek <pane>
#                         brings the pane under the board in the team's
#                         workspace; peeking it again, or another one, sends it
#                         back to its group
#
# A group workspace carries @devws_group (its name) and @devws_team_of (the
# team's workspace); a pane carries both too, to find its way back. Like every
# workspace it has the sidebar, on the left; a group whose last agent leaves
# closes.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=workspace_panes.sh
. "$script_dir/workspace_panes.sh"

# How many agents a group's workspace holds.
per_group="${DEVWS_TEAM_GROUP_SIZE:-4}"

fail() {
  printf 'devws team groups: %s\n' "$*" >&2
  exit 1
}

pane_session() {
  tmux display-message -p -t "$1" '#{session_name}' 2>/dev/null
}

# A group's workspace has the sidebar every workspace has (marked
# @devws_sidebar), to go back to the others.
sidebar_of() {
  tmux list-panes -t "=$1:" -F '#{pane_id} #{@devws_sidebar}' 2>/dev/null | awk '$2 == 1 { print $1; exit }'
}

agents_of() {
  tmux list-panes -t "=$1:" -F '#{pane_id} #{@devws_sidebar}' 2>/dev/null | awk '$2 != 1 { print $1 }'
}

# The agents tiled, the sidebar on their left, full height, at a workspace's
# sidebar width (dev.yml): it steps out to a window of its own while they
# tile. A workspace without one gets it.
arrange() {
  local session="$1" first sidebar
  first="$(agents_of "$session" | head -n 1)"
  [ -n "$first" ] || return 0
  sidebar="$(sidebar_of "$session")"
  [ -z "$sidebar" ] || tmux break-pane -d -s "$sidebar"
  tmux select-layout -t "$(tmux display-message -p -t "$first" '#{window_id}')" tiled
  if [ -n "$sidebar" ]; then
    tmux join-pane -d -f -h -b -l 25 -s "$sidebar" -t "$first"
  else
    sidebar="$(tmux split-window -d -f -h -b -l 25 -t "$first" -c "$(tmux display-message -p -t "$first" '#{pane_current_path}')" \
      -P -F '#{pane_id}' "$script_dir/session_picker.sh")" || return 0
    tmux set-option -p -t "$sidebar" @devws_sidebar 1
  fi
}

# A group workspace left with no agent closes (its clients go to another,
# detach-on-destroy off); else its agents are tiled again.
tidy() {
  local session="$1"
  [ -n "$(tmux show-option -qv -t "=$session:" @devws_group 2>/dev/null)" ] || return 0
  if [ -z "$(agents_of "$session")" ]; then
    tmux kill-session -t "=$session" 2>/dev/null
  else
    arrange "$session"
  fi
  return 0
}

# The first workspace of that group of the team with room; nothing when all
# are full or there is none.
group_with_room() {
  local team="$1" group="$2" session
  while IFS='|' read -r session; do
    [ "$(tmux show-option -qv -t "=$session:" @devws_team_of)" = "$team" ] || continue
    [ "$(tmux show-option -qv -t "=$session:" @devws_group)" = "$group" ] || continue
    [ "$(agents_of "$session" | grep -c .)" -lt "$per_group" ] || continue
    printf '%s' "$session"
    return 0
  done < <(tmux list-sessions -F '#{session_name}' 2>/dev/null)
}

# The name for one more workspace of the group: dev-app-floor, then
# dev-app-floor-2, dev-app-floor-3…
new_group_session() {
  local base number=2
  base="$1-$(printf '%s' "$2" | tr '[:upper:] ' '[:lower:]-')"
  tmux has-session -t "=$base" 2>/dev/null || { printf '%s' "$base"; return; }
  while tmux has-session -t "=$base-$number" 2>/dev/null; do
    number=$((number + 1))
  done
  printf '%s-%s' "$base" "$number"
}

# A group's workspace looks and is grouped as the team's: its task (the
# sidebar), its template (restore leaves it to devAgentics), its folder and
# header. Closing its last panel moves the client on, not out of tmux.
dress_group() {
  local session="$1" team="$2" group="$3" option value window
  for option in @devws_task @devws_template @devws_root @devws_agent status-position status-left-length status-left; do
    value="$(tmux show-option -qv -t "=$team:" "$option" 2>/dev/null)"
    [ -z "$value" ] || tmux set-option -t "=$session:" "$option" "$value"
  done
  tmux set-option -t "=$session:" @devws_editor "agent team · $group"
  tmux set-option -t "=$session:" @devws_group "$group"
  tmux set-option -t "=$session:" @devws_team_of "$team"
  tmux set-option -t "=$session:" detach-on-destroy off
  window="$(tmux display-message -p -t "=$session:" '#{window_id}')"
  tmux set-option -w -t "$window" @devws_tab "$group"
  tmux set-option -w -t "$window" automatic-rename off
  tmux set-option -w -t "$window" allow-rename off
  tmux set-option -w -t "$window" pane-border-status top
  tmux set-option -w -t "$window" pane-border-indicators off
}

# The size a new group workspace starts at: the team's window's.
team_size() {
  tmux display-message -p -t "$(main_window "$1")" '#{window_width} #{window_height}' 2>/dev/null
}

open() {
  local team="$1" group="$2" folder="$3" command="${4:-}" session largest pane width height
  session="$(group_with_room "$team" "$group")"
  if [ -n "$session" ]; then
    largest="$(tmux list-panes -t "=$session:" -F '#{pane_id} #{pane_width} #{pane_height} #{@devws_sidebar}' |
      awk '$4 != 1 && $2 * $3 > best { best = $2 * $3; id = $1 } END { print id }')"
    pane="$(tmux split-window -d -t "$largest" -c "$folder" -P -F '#{pane_id}' ${command:+"$command"})" ||
      fail "could not open a panel in $group"
  else
    session="$(new_group_session "$team" "$group")"
    read -r width height <<< "$(team_size "$team")"
    pane="$(tmux new-session -d -s "$session" -n "$group" -x "${width:-200}" -y "${height:-50}" -c "$folder" \
      -P -F '#{pane_id}' ${command:+"$command"})" || fail "could not open the workspace $session"
    [ -n "$pane" ] || fail "could not open the workspace $session"
    dress_group "$session" "$team" "$group"
  fi
  arrange "$session"
  tmux set-option -p -t "$pane" @devws_tab "$group"
  tmux set-option -p -t "$pane" @devws_team_of "$team"
  printf '%s\n' "$pane"
}

place() {
  local pane="$1" group="${2:-}" team session width height
  [ -n "$group" ] || group="$(tmux show-options -pqv -t "$pane" @devws_tab 2>/dev/null)"
  [ -n "$group" ] || fail "$pane has no group"
  team="$(tmux show-options -pqv -t "$pane" @devws_team_of 2>/dev/null)"
  [ -n "$team" ] || fail "$pane is not an agent team's"
  # Already in a workspace of that group: nothing to do.
  [ "$(tmux show-option -qv -t "$(pane_session "$pane")" @devws_group 2>/dev/null)" = "$group" ] && return 0
  session="$(group_with_room "$team" "$group")"
  if [ -n "$session" ]; then
    tmux join-pane -d -s "$pane" -t "$(agents_of "$session" | head -n 1)" ||
      fail "could not move $pane to $group"
  else
    # A workspace is made around a placeholder, which the pane then replaces.
    session="$(new_group_session "$team" "$group")"
    read -r width height <<< "$(team_size "$team")"
    tmux new-session -d -s "$session" -n "$group" -x "${width:-200}" -y "${height:-50}" 'sleep 5' ||
      fail "could not open the workspace $session"
    local placeholder
    placeholder="$(tmux list-panes -t "=$session:" -F '#{pane_id}' | head -1)"
    tmux join-pane -d -s "$pane" -t "$placeholder" || fail "could not move $pane to $session"
    # Before dress_group's hooks, which would tidy at the same time.
    tmux kill-pane -t "$placeholder"
    dress_group "$session" "$team" "$group"
  fi
  arrange "$session"
  tmux set-option -p -t "$pane" @devws_tab "$group"
  tmux set-option -p -u -t "$pane" @devws_peeked 2>/dev/null || true
  mark_tab_alert "$pane"
}

go() {
  local pane="$1"
  tmux display-message -p -t "$pane" '#{pane_id}' >/dev/null 2>&1 || fail "no pane $pane"
  # From the sidebar or the board a client switches; without one (a script)
  # the workspace still comes to show the pane.
  tmux switch-client -t "$pane" 2>/dev/null
  tmux select-window -t "$pane" && tmux select-pane -t "$pane"
}

peek() {
  local pane="$1" team main peeked board group_window group_session
  team="$(tmux show-options -pqv -t "$pane" @devws_team_of 2>/dev/null)"
  [ -n "$team" ] || fail "$pane is not an agent team's"
  main="$(main_window "$team")"
  # Whatever is peeked goes back first; peeking the same one again only that.
  for peeked in $(tmux list-panes -t "$main" -F '#{pane_id} #{@devws_peeked}' 2>/dev/null | awk '$2 == 1 { print $1 }'); do
    place "$peeked"
    [ "$peeked" = "$pane" ] && return 0
  done
  # Under the board, which keeps the top of its column; else beside the
  # workspace's last panel.
  group_window="$(tmux display-message -p -t "$pane" '#{window_id}')"
  group_session="$(pane_session "$pane")"
  board="$(tmux list-panes -t "$main" -F '#{pane_id} #{@devws_role}' 2>/dev/null | awk '$2 == "team-log" { print $1; exit }')"
  if [ -n "$board" ]; then
    tmux join-pane -d -v -l 60% -s "$pane" -t "$board" || fail "could not bring $pane in"
  else
    tmux join-pane -d -h -s "$pane" -t "$(tmux list-panes -t "$main" -F '#{pane_id}' | tail -1)" ||
      fail "could not bring $pane in"
  fi
  tmux set-option -p -t "$pane" @devws_peeked 1
  # The group it left may have no one waiting now, or no one at all.
  mark_tab_alert "$group_window"
  tidy "$group_session"
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  open) [ "$#" -ge 3 ] || fail "usage: team_groups.sh open <session> <group> <folder> [command]"; open "$@" ;;
  place) [ "$#" -ge 1 ] || fail "usage: team_groups.sh place <pane> [group]"; place "$@" ;;
  go) [ "$#" -eq 1 ] || fail "usage: team_groups.sh go <pane>"; go "$1" ;;
  peek) [ "$#" -eq 1 ] || fail "usage: team_groups.sh peek <pane>"; peek "$1" ;;
  tidy) [ "$#" -eq 1 ] || fail "usage: team_groups.sh tidy <session>"; tidy "$1" ;;
  *) fail "usage: team_groups.sh open|place|go|peek|tidy …" ;;
esac
