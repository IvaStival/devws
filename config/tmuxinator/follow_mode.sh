#!/usr/bin/env bash
# Chooses how the editor follows the agent in a devws workspace (the session
# option @devws_follow, read by editor_open.sh):
#
#   off      the editor never moves by itself
#   follow   show each file the agent changes, at the change (the default)
#   notify   only name each change in a notification
#   focus    follow, and move the focus to the editor pane
#   explore  follow the files the agent reads too (Claude Code)
#
#   follow_mode.sh [session]          opens a menu of the modes
#   follow_mode.sh <session> <mode>   sets one

set -u

session="${1:-$(tmux display-message -p '#{session_name}')}"
mode="${2:-}"
script="$0"

label() {
  case "$1" in
    off) printf 'Off' ;;
    follow) printf 'Follow edits' ;;
    notify) printf 'Notify only' ;;
    focus) printf 'Follow edits and focus the editor' ;;
    explore) printf 'Follow edits and reads' ;;
    *) return 1 ;;
  esac
}

if [ -n "$mode" ]; then
  if ! name="$(label "$mode")"; then
    printf 'Unknown follow mode: %s (off, follow, notify, focus, explore)\n' "$mode" >&2
    exit 2
  fi
  tmux set-option -t "$session" @devws_follow "$mode"
  tmux display-message -t "$session" "Agent follow: $name"
  exit 0
fi

current="$(tmux show-option -qv -t "$session" @devws_follow)"
# Workspaces from before the modes say "on"; unset means the default.
case "$current" in
  off|notify|focus|explore) ;;
  *) current=follow ;;
esac

items=()
key=1
for choice in follow notify focus explore off; do
  marker="  "
  [ "$choice" = "$current" ] && marker="● "
  items+=("$marker$(label "$choice")" "$key" "run-shell -b \"'$script' '$session' $choice\"")
  key=$((key + 1))
done
tmux display-menu -T "#[align=centre] Agent follow " -x C -y C "${items[@]}"
