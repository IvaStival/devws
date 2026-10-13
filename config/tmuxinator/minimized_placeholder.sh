#!/usr/bin/env bash
# Stands in for a minimized agent or terminal pane while the real one keeps
# running in a hidden window. Draws a short notice when the bar is wide enough.
#
#   minimized_placeholder.sh <label>

set -u

label="${1:-pane}"

# tmux sends focus-in/out sequences to the pane (focus-events is on); nothing
# reads them here, so without this the terminal would echo them as "^[[I".
stty -echo 2>/dev/null

draw() {
  local width
  width="$(tput cols 2>/dev/null || printf 80)"
  printf '\033[2J\033[H\033[?25l'
  # The narrow strip of the full-height-editor layout has its header label
  # already, so it is left blank.
  if (( width >= 40 )); then
    printf '\033[2m %s · minimized — click the header or press C-b _\033[0m' "$label"
  fi
}

trap draw WINCH
draw
# sleep runs in the background so the WINCH trap fires as soon as tmux
# resizes the bar, not only after the sleep ends.
while true; do
  sleep 3600 &
  wait $!
done
