#!/usr/bin/env bash
# Every question devws asks from a list, in one look: the prompt, a header
# line saying what is asked, and a footer with the keys — Esc goes back a step
# (the caller's loop does that: Esc exits 130), Enter chooses, Tab unticks in
# a multi-select. The list comes on stdin; the choice is printed.
#
#   picker.sh <prompt> <header> [fzf options...]
#
# The caller's options come last, so they win over the shared ones (a
# --height, a --footer of its own).
#
# A pane that scrolls under a popup (an agent writing) drags the popup's cells
# along with it, and fzf only redraws what it changed: what the pane scrolled
# in would stay. So the picker is redrawn whole every second (fzf's
# clear-screen, through its socket), which touches only its own lines.

set -u

prompt="${1:?Usage: picker.sh <prompt> <header> [fzf options...]}"
header="${2:-}"
shift 2

footer='Esc back · Enter choose'
for option in "$@"; do
  [ "$option" = --multi ] && footer='Tab untick · Esc back · Enter choose'
done

# Short and in TMPDIR: a Unix socket path is limited to about 100 bytes.
socket="${TMPDIR:-/tmp}"
socket="${socket%/}/devws-picker-$$.sock"

keep_redrawn() {
  while sleep 1; do
    [ -S "$socket" ] || continue
    curl -s --max-time 1 --unix-socket "$socket" http://localhost -d 'clear-screen' >/dev/null 2>&1
  done
}
keep_redrawn &
redrawer=$!

fzf --ansi --layout=reverse --height=~60% \
  "--prompt=$prompt › " "--header=$header" \
  "--footer=$(printf '\033[2m%s\033[0m' "$footer")" \
  --info=inline-right --no-separator \
  "--listen=$socket" \
  "$@"
status=$?

kill "$redrawer" 2>/dev/null
rm -f "$socket"
exit "$status"
