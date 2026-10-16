#!/usr/bin/env bash
# Opens the devws database pane beside the editor, or focuses it when it is
# already open (expanding it first if it was minimized).
#
#   open_database.sh [session] [--no-focus|--reconnect]
#
# --reconnect restarts an open pane, so it picks up a newly chosen folder.
#
# Placement follows the layout: with the agent and terminal on the right, the
# database goes below the editor; with them below, it goes to the editor's
# right.

set -u

# Absolute, because the runner path is handed to a pane started in the
# project folder.
script_dir="$(cd "$(dirname "$0")" && pwd)"
session="${1:-$(tmux display-message -p '#S' 2>/dev/null)}"
focus=1
reconnect=0
case "${2:-}" in
  --no-focus) focus=0 ;;
  --reconnect) reconnect=1 ;;
esac

fail() {
  tmux display-message "Open database: $*" 2>/dev/null
  printf 'Open database: %s\n' "$*" >&2
  exit 1
}

[ -n "$session" ] || fail "no tmux session"

. "$script_dir/workspace_panes.sh"

load_workspace_panes "$session" || fail "session $session not found"
if [ -n "$(pane_with_role minimized-database)" ]; then
  "$script_dir/minimize_panes.sh" "$session" restore database ||
    fail "could not expand the database pane"
  load_workspace_panes "$session" || fail "session $session not found"
fi

root="$(tmux show-options -qv -t "$session" @devws_root)"
runner="$(printf '%q %q' "$script_dir/database_runner.sh" "${root:-$PWD}")"

database="$(pane_with_role database)"
if [ -n "$database" ]; then
  if [ "$reconnect" = 1 ]; then
    tmux respawn-pane -k -t "$database" -c "${root:-$PWD}" "$runner" ||
      fail "could not reconnect the database pane"
  fi
  [ "$focus" = 1 ] && tmux select-pane -t "$database"
  exit 0
fi
# Nothing open to reconnect; the next open uses the new folder anyway.
[ "$reconnect" = 1 ] && exit 0

editor="$(pane_with_role editor)"
[ -n "$editor" ] || fail "editor pane not found"
# The pane that tells the layout: the agent, or a minimized agent's
# placeholder, which keeps its slot. A closed agent pane (or an agent started
# by hand in the terminal) leaves the terminal, which shares its column or row.
neighbour="$(pane_with_role agent)"
[ -n "$neighbour" ] || neighbour="$(pane_with_role minimized-agent)"
[ -n "$neighbour" ] || neighbour="$(pane_with_role terminal)"
[ -n "$neighbour" ] || neighbour="$(pane_with_role minimized-terminal)"

split_options=(-t "$editor" -c "${root:-$PWD}")
[ "$focus" = 1 ] || split_options+=(-d)

# With nothing beside the editor, the database goes to its right.
if [ -n "$neighbour" ] && is_side_layout "$editor" "$neighbour"; then
  split_options+=(-v -l 35%)
else
  split_options+=(-h -l 40%)
fi
database="$(tmux split-window "${split_options[@]}" -P -F '#{pane_id}' "$runner")" ||
  fail "could not open the database pane"
# Tagged here as well as by the runner, so the save below already sees it.
tmux set-option -p -t "$database" @devws_role database

if [ -x "$script_dir/save_state.sh" ]; then
  "$script_dir/save_state.sh"
fi
