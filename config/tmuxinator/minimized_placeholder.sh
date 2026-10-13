#!/usr/bin/env bash
# Stands in for a minimized agent or terminal pane while the real one keeps
# running in a hidden window. Draws a short notice sized to the bar.
#
#   minimized_placeholder.sh <icon> <label>

set -u

icon="${1:-•}"
label="${2:-pane}"

draw() {
  local width
  width="$(tput cols 2>/dev/null || printf 80)"
  printf '\033[2J\033[H\033[?25l\033[2m'
  # The narrow strip of the full-height-editor layout only fits the icon.
  if (( width >= 40 )); then
    printf ' %s %s · minimized — click the header or press C-b _' "$icon" "$label"
  else
    printf ' %s' "$icon"
  fi
  printf '\033[0m'
}

trap draw WINCH
draw
# sleep runs in the background so the WINCH trap fires as soon as tmux
# resizes the bar, not only after the sleep ends.
while true; do
  sleep 3600 &
  wait $!
done
