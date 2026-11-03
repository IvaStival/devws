#!/usr/bin/env bash
# Marks the pane as the workspace editor pane, then runs the selected editor
# and waits for it rather than replacing itself, so the editor can be frozen
# without the pane's shell noticing (agent_runner.sh says why).

set -u

config="$HOME/.tmuxinator/.env"
[ -r "$config" ] && . "$config"

editor="${1:-${DEVWS_DEFAULT_EDITOR:-nvim}}"
allowed=0
for candidate in ${DEVWS_EDITORS:-nvim vim code zed}; do
  if [ "$candidate" = "$editor" ]; then
    allowed=1
    break
  fi
done

if [ "$allowed" -ne 1 ]; then
  printf 'Unsupported devws editor: %s\n' "$editor" >&2
  exit 2
fi
if ! command -v "$editor" >/dev/null 2>&1; then
  printf 'Editor is not installed or not on PATH: %s\n' "$editor" >&2
  exit 127
fi

tmux set-option -p -t "$TMUX_PANE" @devws_role editor
session="$(tmux display-message -p -t "$TMUX_PANE" '#{session_name}')"
case "$editor" in
  code|zed)
    tmux set-option -u -t "$session" @devws_editor_socket
    "$editor" --wait .
    ;;
  nvim)
    # The agent reaches Neovim through this socket (editor_open.sh). Unix
    # socket paths are limited to ~100 characters, and TMPDIR is long on macOS.
    # DEVWS_NVIM_SOCKET_PREFIX replaces the prefix, for tests.
    socket="${DEVWS_NVIM_SOCKET_PREFIX:-/tmp/devws-nvim-$(id -u)-}$(printf '%s' "$session" | tr -c 'A-Za-z0-9_.-' '_' | cut -c1-40).sock"
    servers="$(dirname "$0")/nvim_servers.sh"
    # What an earlier editor left on the socket would take it over; servers
    # other workspaces' editors left stuck (a tmux server that went away runs
    # no reap of theirs) go in the background, not to hold the editor up.
    "$servers" stop "$socket"
    "$servers" reap >/dev/null 2>&1 </dev/null &
    tmux set-option -t "$session" @devws_editor_socket "$socket"
    nvim --listen "$socket" .
    # Its server may outlive it, stuck at an exit prompt.
    "$servers" reap "$socket"
    ;;
  *)
    tmux set-option -u -t "$session" @devws_editor_socket
    "$editor" .
    ;;
esac
