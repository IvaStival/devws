#!/usr/bin/env bash
# Exercise logical scheduling without sleeping or starting a tmux server.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/devws-scheduler.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT
sed '/^case "${1:-}" in/,$d' "$ROOT/config/tmuxinator/header_ticker.sh" > "$fixture/source"
bash -c '
  . "$1"
  state_dir="$2"
  popup_flag="$2/no-popup"
  load_settings() { show_pet=1; }
  load_ghost() { :; }
  any_focused() { return 0; }
  read_clients() { :; }
  in_background() { printf "source %s\n" "$1" >> "$state_dir/calls"; }
  draw_ghost() { printf "ghost %s\n" "$1" >> "$state_dir/calls"; }
  spin_agents() { printf "spinner %s\n" "$1" >> "$state_dir/calls"; }
  tmux() {
    case "$*" in *show-option*-gqv*@devws_header_ticker*) echo "$$" ;; esac
    return 0
  }
  waits=0
  nap() {
    printf "wait %s\n" "$1" >> "$state_dir/calls"
    waits=$((waits + 1))
    [ "$waits" -lt 4 ] || exit 0
  }
  run
' "$ROOT/config/tmuxinator/header_ticker.sh" "$fixture/source" "$fixture"
[ "$(grep -c '^wait 0.5$' "$fixture/calls" || true)" = 4 ] || { echo 'Ticker woke between display frames' >&2; exit 1; }
[ "$(grep '^ghost ' "$fixture/calls")" = $'ghost 0\nghost 5\nghost 10\nghost 15' ] || { echo 'Ghost pace changed' >&2; exit 1; }
[ "$(grep '^spinner ' "$fixture/calls")" = $'spinner 0\nspinner 1\nspinner 2\nspinner 3' ] || { echo 'Spinner pace changed' >&2; exit 1; }
[ "$(grep -c '^source layout$' "$fixture/calls")" = 2 ] || { echo 'Layout period changed' >&2; exit 1; }
[ "$(grep -c '^source tokens$' "$fixture/calls")" = 1 ] || { echo 'Token refresh period changed' >&2; exit 1; }
# A cost result arriving during an older account render must schedule a
# second render, rather than having its notification cleared by the first.
bash -c '
  . "$1"
  state_dir="$2"
  keep() { : > "$state_dir/costs.changed"; }
  tmux() { return 0; }
  update_tokens
  [ -e "$state_dir/costs.changed" ] || { echo "Cost refresh notification was lost" >&2; exit 1; }
' "$ROOT/config/tmuxinator/header_ticker.sh" "$fixture/source" "$fixture"
echo 'Header scheduler tests passed.'
