#!/usr/bin/env bash
# A header bar, in the style of the Claude Code statusline: ▓▓▓▓▓▓░░░░, all of
# it in the level's color: green, yellow from 60% used, red from 85%.
#
#   header_bar.sh <filled %> [<used %>] [<cells>]   10 cells unless given
#
# The two differ for what is shown as left: a limit 30% used is drawn 70%
# filled, in the color of 30% used. DEVWS_COLOR_OK, DEVWS_COLOR_WARN and
# DEVWS_COLOR_STRESS replace the colors.

set -u

filled_percent="${1:?Usage: header_bar.sh <filled %> <used %>}"
used_percent="${2:-$filled_percent}"
width="${3:-10}"

if [ "$used_percent" -lt 60 ]; then
  color="${DEVWS_COLOR_OK:-green}"
elif [ "$used_percent" -lt 85 ]; then
  color="${DEVWS_COLOR_WARN:-yellow}"
else
  color="${DEVWS_COLOR_STRESS:-red}"
fi

cells=$(((filled_percent * width + 50) / 100))
[ "$cells" -gt "$width" ] && cells="$width"
[ "$cells" -lt 0 ] && cells=0
bar=""
for ((cell = 0; cell < width; cell++)); do
  if [ "$cell" -lt "$cells" ]; then
    bar+='▓'
  else
    bar+='░'
  fi
done
printf '#[fg=%s]%s#[fg=default]' "$color" "$bar"
