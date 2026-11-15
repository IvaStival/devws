#!/usr/bin/env bash
# tmux.remote.conf, which tmux.conf loads over SSH: the one-line status bar at
# the bottom is for a tmux nested in a local one (its terminal says tmux-* or
# screen-*); devws run straight over SSH keeps its header. The clipboard's
# tunnel is for both.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REMOTE_CONF="$ROOT/config/tmux/tmux.remote.conf"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-remote-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
}
trap cleanup EXIT

fail() {
  printf 'tmux remote test failed: %s\n' "$*" >&2
  exit 1
}

# <TERM>: a server started from a terminal of that type, with tmux.conf's
# header (five lines at the top), then the remote settings sourced.
start_remote_server() {
  cleanup
  TERM="$1" SSH_CLIENT='192.0.2.1 50000 22' "$REAL_TMUX" -L "$SOCKET" -f /dev/null new-session -d -s remote
  "$REAL_TMUX" -L "$SOCKET" set -g status 5 \; set -g status-position top \; set -g status-right devws-right
  "$REAL_TMUX" -L "$SOCKET" source-file "$REMOTE_CONF"
}

option() {
  "$REAL_TMUX" -L "$SOCKET" show-options -gv "$1"
}

test_devws_over_ssh_keeps_its_header() {
  start_remote_server xterm-256color
  [ "$(option status)" = 5 ] || fail "the header was cut to one line over SSH: status $(option status)"
  [ "$(option status-position)" = top ] || fail "the status bar moved: $(option status-position)"
  [ "$(option status-right)" = devws-right ] || fail "the status bar lost devws's right side: $(option status-right)"
  [ "$(option @copy_backend_remote_tunnel_port)" = 11988 ] || fail "the clipboard tunnel is not set over SSH"
}

test_a_nested_tmux_keeps_one_line_at_the_bottom() {
  local term
  for term in tmux-256color screen-256color; do
    start_remote_server "$term"
    [ "$(option status)" = on ] || fail "$term: a nested tmux kept the header: status $(option status)"
    [ "$(option status-position)" = bottom ] || fail "$term: a nested tmux's bar is not at the bottom"
    [ "$(option @copy_backend_remote_tunnel_port)" = 11988 ] || fail "$term: the clipboard tunnel is not set"
  done
}

test_devws_over_ssh_keeps_its_header
test_a_nested_tmux_keeps_one_line_at_the_bottom

printf 'tmux remote tests passed.\n'
