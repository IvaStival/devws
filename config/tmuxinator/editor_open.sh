#!/usr/bin/env bash
# Shows a file in the workspace's Neovim, at a line or at some text, without
# taking the tmux focus. The agent runs it to show the user something; its edit
# hooks run it with --follow, which does nothing while following is off.
#
#   editor_open.sh [--follow] <file>[:<line>] [<text to find>]

set -u

follow=0
if [ "${1:-}" = "--follow" ]; then
  follow=1
  shift
fi
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
if [ "$follow" -eq 1 ] && [ "$(tmux show-option -qv -t "$session" @devws_follow)" = "off" ]; then
  exit 0
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

expression="v:lua.require'devws.follow'.show($(quote "$file"), $line, $(quote "${text%%$'\n'*}"))"
# In the background, and given up after a few seconds: Neovim may be waiting
# on a prompt, and neither the agent nor a pile of these calls should wait for it.
(
  nvim --server "$socket" --remote-expr "$expression" >/dev/null 2>&1 </dev/null &
  call=$!
  sleep 5
  kill "$call" 2>/dev/null
) >/dev/null 2>&1 </dev/null &
exit 0
