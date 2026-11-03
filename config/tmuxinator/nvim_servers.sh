#!/usr/bin/env bash
# Stops the Neovim servers devws starts for its editors, which may not stop on
# their own. `nvim --listen <socket>` is a UI that runs the editor in a child
# (`nvim --embed --listen <socket>`); when the UI goes away (its pane closed,
# the tmux server gone) the child exits, but a message on the way out leaves
# it at a "Press ENTER" prompt nobody can answer, still running its plugins'
# timers: one held 11 GB after five days. SIGTERM waits behind that prompt,
# so what does not quit on request is killed.
#
#   nvim_servers.sh stop <socket>     every Neovim on <socket>, asked to quit
#                                     over it: what an earlier editor left
#                                     there, before the editor starts on it
#   nvim_servers.sh reap [<socket>]   every devws editor server whose UI is
#                                     gone (parent pid 1), or only <socket>'s:
#                                     the editor's own, once it has exited.
#                                     A newer editor on that socket has its
#                                     UI, so it is never matched.
#
# Only devws's sockets are matched, /tmp/devws-nvim-<uid>-*.sock: Neovims you
# start yourself, New panel's among them, are never touched.

set -u

# DEVWS_NVIM_SOCKET_PREFIX replaces it, for tests.
socket_prefix="${DEVWS_NVIM_SOCKET_PREFIX:-/tmp/devws-nvim-$(id -u)-}"
# Seconds a server gets to quit on request before it is killed;
# DEVWS_NVIM_GRACE replaces it, for tests.
grace="${DEVWS_NVIM_GRACE:-3}"

# Prints the pids of the Neovims whose command line passes --listen with a
# devws socket, as "<pid> <parent pid> <socket>".
devws_servers() {
  ps -axo pid=,ppid=,command= | awk -v prefix="$socket_prefix" '
    $3 ~ /(^|\/)nvim$/ {
      for (i = 4; i < NF; i++)
        if ($i == "--listen" && index($(i + 1), prefix) == 1) { print $1, $2, $(i + 1); break }
    }'
}

alive() {
  local pid
  for pid in "$@"; do
    kill -0 "$pid" 2>/dev/null && return 0
  done
  return 1
}

# Asks the servers to quit, then kills what is left: over <socket> (:qa!)
# when given, else with SIGTERM. A frozen workspace's (suspend_workspace.sh)
# are woken first, or they could not answer.
#   stop_pids <socket or ""> <pid…>
stop_pids() {
  local socket="$1" waited=0
  shift
  stopped_by_kill=0
  [ "$#" -gt 0 ] || return 0
  kill -CONT "$@" 2>/dev/null
  if [ -z "$socket" ]; then
    kill -TERM "$@" 2>/dev/null
  elif [ -S "$socket" ] && command -v nvim >/dev/null 2>&1; then
    # In the background: a server stuck at its prompt may never answer.
    nvim --server "$socket" --remote-send '<C-\><C-N>:qa!<CR>' >/dev/null 2>&1 </dev/null &
  fi
  while alive "$@" && [ "$waited" -lt $((grace * 10)) ]; do
    sleep 0.1
    waited=$((waited + 1))
  done
  if alive "$@"; then
    kill -KILL "$@" 2>/dev/null
    stopped_by_kill=1
  fi
  return 0
}

stop() {
  local socket="$1" pids
  pids="$(devws_servers | awk -v socket="$socket" '$3 == socket { print $1 }')"
  # shellcheck disable=SC2086
  stop_pids "$socket" $pids
  rm -f "$socket"
}

reap() {
  local only="${1:-}" pid ppid socket
  while read -r pid ppid socket; do
    [ "$ppid" = 1 ] || continue
    [ -z "$only" ] || [ "$socket" = "$only" ] || continue
    # Not over its socket: a newer editor may have taken the path over, and
    # :qa! sent there would close that one.
    stop_pids "" "$pid"
    # Only when it outlived the request: one that quit by itself is no news.
    if [ "$stopped_by_kill" = 1 ]; then
      printf 'devws: killed a Neovim left stuck without its editor (pid %s, %s)\n' "$pid" "$socket" >&2
    fi
  done < <(devws_servers)
}

case "${1:-}" in
  stop)
    [ -n "${2:-}" ] || { printf 'Usage: nvim_servers.sh stop <socket>\n' >&2; exit 2; }
    stop "$2"
    ;;
  reap) reap "${2:-}" ;;
  *)
    printf 'Usage: nvim_servers.sh stop <socket> | reap [<socket>]\n' >&2
    exit 2
    ;;
esac
