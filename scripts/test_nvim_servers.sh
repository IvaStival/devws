#!/usr/bin/env bash
# nvim_servers.sh against real headless Neovims, on sockets of their own: a
# test prefix keeps the user's editors out of reach.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVERS="$ROOT/config/tmuxinator/nvim_servers.sh"
# Short: Unix socket paths are limited to ~100 characters.
PREFIX="/tmp/devws-nvim-test-$$-"
export DEVWS_NVIM_SOCKET_PREFIX="$PREFIX" DEVWS_NVIM_GRACE=1
started=()

cleanup() {
  [ "${#started[@]}" -eq 0 ] || kill -KILL "${started[@]}" 2>/dev/null || true
  pkill -KILL -f -- "--listen $PREFIX" 2>/dev/null || true
  rm -f "$PREFIX"*
}
trap cleanup EXIT

fail() {
  printf 'Neovim servers test failed: %s\n' "$*" >&2
  exit 1
}

command -v nvim >/dev/null 2>&1 || { printf 'Neovim servers test skipped: no nvim\n'; exit 0; }

# The pid of the Neovim listening on <socket>, once it is up.
server_pid() {
  local try pid
  for try in $(seq 50); do
    pid="$(pgrep -f -- "--listen $1( |$)" | head -n 1)" && [ -n "$pid" ] && { printf '%s' "$pid"; return 0; }
    sleep 0.1
  done
  return 1
}

# A server whose UI is gone (parent pid 1), like one left by a closed pane.
# "stuck" blocks it the way the exit prompt does: busy, deaf to SIGTERM.
orphan() {
  local socket="$1" stuck="${2:-}" pid
  if [ "$stuck" = stuck ]; then
    (nvim --clean --headless --listen "$socket" --cmd 'lua vim.uv.sleep(600000)' >/dev/null 2>&1 </dev/null &)
  else
    (nvim --clean --headless --listen "$socket" >/dev/null 2>&1 </dev/null &)
  fi
  pid="$(server_pid "$socket")" || fail "Neovim did not start on $socket"
  started+=("$pid")
  [ "$(ps -o ppid= -p "$pid" | tr -d ' ')" = 1 ] || fail "the test server $pid is not an orphan"
  printf '%s' "$pid"
}

gone() {
  local try
  for try in $(seq 50); do
    kill -0 "$1" 2>/dev/null || return 0
    sleep 0.1
  done
  return 1
}

test_reap_kills_a_stuck_orphan() {
  local pid output
  pid="$(orphan "${PREFIX}stuck.sock" stuck)"
  output="$("$SERVERS" reap 2>&1)"
  gone "$pid" || fail "reap left the stuck server $pid running"
  [[ "$output" == *"killed a Neovim left stuck"*"$pid"* ]] || fail "reap did not report the kill: $output"
}

test_reap_stops_a_quiet_orphan_without_noise() {
  local pid output
  pid="$(orphan "${PREFIX}quiet.sock")"
  output="$("$SERVERS" reap 2>&1)"
  gone "$pid" || fail "reap left the orphan $pid running"
  [ -z "$output" ] || fail "a server that quit on SIGTERM was reported: $output"
}

# A server whose UI still runs, and one on a socket devws did not make, stay.
test_reap_leaves_live_and_foreign_servers() {
  local live_socket="${PREFIX}live.sock" foreign_socket="/tmp/nvim-test-$$-foreign.sock"
  local parent live foreign
  bash -c "nvim --clean --headless --listen '$live_socket' >/dev/null 2>&1 </dev/null & wait" &
  parent=$!
  started+=("$parent")
  live="$(server_pid "$live_socket")" || fail "the live server did not start"
  started+=("$live")
  (nvim --clean --headless --listen "$foreign_socket" >/dev/null 2>&1 </dev/null &)
  foreign="$(server_pid "$foreign_socket")" || fail "the foreign server did not start"
  started+=("$foreign")

  "$SERVERS" reap 2>/dev/null
  "$SERVERS" reap "$live_socket" 2>/dev/null
  kill -0 "$live" 2>/dev/null || fail "reap stopped a server whose UI is still running"
  kill -0 "$foreign" 2>/dev/null || fail "reap stopped a Neovim that is not devws's"
  kill -KILL "$live" "$parent" "$foreign" 2>/dev/null || true
  wait "$parent" 2>/dev/null || true
  rm -f "$foreign_socket"
}

# reap <socket> stops only that socket's orphan.
test_reap_one_socket() {
  local mine other
  mine="$(orphan "${PREFIX}mine.sock")"
  other="$(orphan "${PREFIX}other.sock")"
  "$SERVERS" reap "${PREFIX}mine.sock" 2>/dev/null
  gone "$mine" || fail "reap <socket> left its server running"
  kill -0 "$other" 2>/dev/null || fail "reap <socket> stopped another socket's server"
  "$SERVERS" reap 2>/dev/null
}

# stop asks over the socket, so a working server quits by itself (:qa!), and
# the socket file goes.
test_stop_quits_a_server_over_its_socket() {
  local socket="${PREFIX}stop.sock" parent pid
  bash -c "nvim --clean --headless --listen '$socket' >/dev/null 2>&1 </dev/null & wait" &
  parent=$!
  started+=("$parent")
  pid="$(server_pid "$socket")" || fail "the server did not start"
  "$SERVERS" stop "$socket" 2>/dev/null
  gone "$pid" || fail "stop left the server on $socket running"
  [ ! -e "$socket" ] || fail "stop left the socket file behind"
  wait "$parent" 2>/dev/null || true
}

test_reap_kills_a_stuck_orphan
test_reap_stops_a_quiet_orphan_without_noise
test_reap_leaves_live_and_foreign_servers
test_reap_one_socket
test_stop_quits_a_server_over_its_socket

printf 'Neovim servers tests passed.\n'
