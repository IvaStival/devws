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
# No periodic repaint: a full redraw every second made the popup flicker in
# step with the cursor. A pane scrolling under the popup no longer drags its
# cells (tmux.conf turns off scroll margins), so nothing needs repainting.

set -u

prompt="${1:?Usage: picker.sh <prompt> <header> [fzf options...]}"
header="${2:-}"
shift 2

footer='Esc back · Enter choose'
for option in "$@"; do
  [ "$option" = --multi ] && footer='Tab untick · Esc back · Enter choose'
done

exec fzf --ansi --layout=reverse --height=~60% \
  "--prompt=$prompt › " "--header=$header" \
  "--footer=$(printf '\033[2m%s\033[0m' "$footer")" \
  --info=inline-right --no-separator \
  "$@"
