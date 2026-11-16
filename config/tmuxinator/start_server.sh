#!/usr/bin/env bash
# Starts the tmux server from $HOME when none is running. Once the server's
# own working directory is deleted, tmux opens every new pane there and
# ignores -c; a server started from a worktree that is later removed would put
# each new workspace's panes in the dead folder. A running server is kept.
#
# exit-empty is left off, or the server, with no session yet, would exit at
# once: the caller turns it back on (tmux set-option -g exit-empty on) after
# creating its sessions.

set -u

tmux list-sessions >/dev/null 2>&1 && exit 0
cd "$HOME" || exit 1
tmux start-server \; set-option -g exit-empty off
