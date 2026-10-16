#!/usr/bin/env bash
# The header's clock: the time, HH:MM, in block digits three rows tall, for
# the first group of the open header (header_ticker.sh lays it out every
# second, so it turns over within a second of the minute).
#
#   ▀█  ▀▀█ ▄ ▀▀█ █ █
#    █  █▀▀ ▄ ▀▀█ ▀▀█
#   ▀▀▀ ▀▀▀   ▀▀▀   ▀
#
#   header_clock.sh
#
# DEVWS_CLOCK_TIME (HH:MM) replaces the current time, for tests.

set -u

time="${DEVWS_CLOCK_TIME:-$(date +%H:%M)}"

# Each glyph's three rows; a digit is three columns wide, the colon one.
glyph() {
  case "$1" in
    0) printf '%s\n' '█▀█' '█ █' '▀▀▀' ;;
    1) printf '%s\n' '▀█ ' ' █ ' '▀▀▀' ;;
    2) printf '%s\n' '▀▀█' '█▀▀' '▀▀▀' ;;
    3) printf '%s\n' '▀▀█' '▀▀█' '▀▀▀' ;;
    4) printf '%s\n' '█ █' '▀▀█' '  ▀' ;;
    5) printf '%s\n' '█▀▀' '▀▀█' '▀▀▀' ;;
    6) printf '%s\n' '█▀▀' '█▀█' '▀▀▀' ;;
    7) printf '%s\n' '▀▀█' '  █' '  ▀' ;;
    8) printf '%s\n' '█▀█' '█▀█' '▀▀▀' ;;
    9) printf '%s\n' '█▀█' '▀▀█' '▀▀▀' ;;
    :) printf '%s\n' '▄' '▄' ' ' ;;
  esac
}

rows=("" "" "")
for ((index = 0; index < ${#time}; index++)); do
  row=0
  while IFS= read -r part; do
    [ -z "${rows[row]}" ] || rows[row]+=" "
    rows[row]+="$part"
    row=$((row + 1))
  done < <(glyph "${time:index:1}")
done
printf '%s\n' "${rows[@]}"
