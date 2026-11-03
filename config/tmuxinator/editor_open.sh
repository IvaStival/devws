#!/usr/bin/env bash
# Shows a file in the workspace's Neovim, at a line or at some text, without
# taking the tmux focus. The agent runs it to show the user something; its
# hooks run it with --follow (an edit) or --follow-read (a file it read), which
# then do what the workspace's follow mode (follow_mode.sh) says.
#
#   editor_open.sh [--follow|--follow-read] <file>[:<line>] [<text to find>]
#   editor_open.sh --follow-changes <file>:<line>   an edit found by snapshot
#                                   (agent_changes.sh): its ranges on stdin,
#                                   "<file>\t<first>\t<last>", all marked,
#                                   <file> shown
#   editor_open.sh --clear-marks    the user's new prompt: no marks any more

set -u

follow=0
event=""
ranges=""
case "${1:-}" in
  --follow) follow=1; event=edit; shift ;;
  --follow-read) follow=1; event=read; shift ;;
  --follow-changes) follow=1; event=edit; ranges="$(cat)"; shift ;;
  --clear-marks) follow=1; event=clear; set -- clear ;;
esac
target="${1:-}"
text="${2:-}"

# Following must never disturb the agent, so it gives up silently; a request
# from the agent says why it could not be shown.
give_up() {
  [ "$follow" -eq 1 ] || printf 'devws: %s\n' "$1" >&2
  exit "$((1 - follow))"
}

[ -n "$target" ] || give_up "usage: editor_open.sh <file>[:<line>]"
[ -n "${TMUX_PANE:-}" ] || give_up "not in a devws workspace"
session="$(tmux display-message -p -t "$TMUX_PANE" '#{session_name}' 2>/dev/null)" ||
  give_up "not in a devws workspace"
socket="$(tmux show-option -qv -t "$session" @devws_editor_socket 2>/dev/null)"
[ -n "$socket" ] && [ -S "$socket" ] ||
  give_up "the workspace editor is not linked (only Neovim is)"

notify_only=false
focus_editor=0
if [ "$follow" -eq 1 ]; then
  mode="$(tmux show-option -qv -t "$session" @devws_follow)"
  case "$mode" in
    off) exit 0 ;;
    notify) notify_only=true ;;
    focus) focus_editor=1 ;;
    explore) ;;
    # follow, "on" from workspaces older than the modes, or unset.
    *) mode=follow ;;
  esac
  # Only "follow edits and reads" shows the files the agent reads.
  [ "$event" != read ] || [ "$mode" = explore ] || exit 0
fi

file="$target"
line=0
if [[ ! -e "$target" && "$target" =~ ^(.+):([0-9]+)$ ]]; then
  file="${BASH_REMATCH[1]}"
  line="${BASH_REMATCH[2]}"
fi
case "$file" in
  /*) ;;
  *) file="$PWD/$file" ;;
esac

# Vim string literals: a single quote is written twice.
quote() {
  printf "'%s'" "${1//\'/\'\'}"
}

# Stamped now, in microseconds, so Neovim can drop a request that arrives after
# a newer one (the calls below race in the background). macOS's bash 3.2 has
# no EPOCHREALTIME; without perl the requests go unstamped and all are shown.
stamp="$(perl -MTime::HiRes=time -e 'printf "%d", time * 1000000' 2>/dev/null)" || stamp=""
if [ "$event" = clear ]; then
  expression="v:lua.require'devws.follow'.clear_marks()"
  focus_editor=0
elif [ -n "$ranges" ]; then
  # The ranges as a Vim list: [['<file>', <first>, <last>], ...].
  list=""
  while IFS=$'\t' read -r range_file first last; do
    [ -n "$range_file" ] && [ -n "$first" ] && [ -n "$last" ] || continue
    list+="${list:+, }[$(quote "$range_file"), $first, $last]"
  done <<<"$ranges"
  expression="v:lua.require'devws.follow'.show_changes([$list], $(quote "$file"), $line, v:$notify_only${stamp:+, $stamp})"
else
  expression="v:lua.require'devws.follow'.show($(quote "$file"), $line, $(quote "${text%%$'\n'*}"), v:$notify_only${stamp:+, $stamp})"
fi
# In the background, and given up after a few seconds: Neovim may be waiting
# on a prompt, and neither the agent nor a pile of these calls should wait for it.
(
  nvim --server "$socket" --remote-expr "$expression" >/dev/null 2>&1 </dev/null &
  call=$!
  sleep 5
  kill "$call" 2>/dev/null
) >/dev/null 2>&1 </dev/null &

if [ "$focus_editor" -eq 1 ]; then
  . "$(dirname "$0")/workspace_panes.sh"
  load_workspace_panes "$session"
  editor="$(pane_with_role editor)"
  [ -z "$editor" ] || tmux select-pane -t "$editor"
fi
exit 0
