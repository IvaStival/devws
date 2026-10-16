#!/usr/bin/env bash
# The weather for the header, from wttr.in, as three rows:
#
#   +18°C  Light rain
#   Curitiba
#   humidity 70% · wind ↙12km/h
#
# Located by the machine's IP unless DEVWS_WEATHER_LOCATION (in .env) names a
# place. Prints nothing when offline or when wttr.in does not know the place,
# so header_ticker.sh keeps showing the last weather it had.

set -u

location="${DEVWS_WEATHER_LOCATION:-}"
# Temperature, condition, place, humidity and wind, split by "|" (%7C). Text,
# not wttr's emoji: emoji widths differ between terminals.
report="$(curl -fsS --max-time 5 "https://wttr.in/${location// /+}?format=%t%7C%C%7C%l%7C%h%7C%w" 2>/dev/null)" || exit 0
# Anything else (an error page, "Unknown location") has no temperature.
case "$report" in
  *°*'|'*) ;;
  *) exit 0 ;;
esac
IFS='|' read -r temperature condition place humidity wind <<< "$(printf '%s' "$report" | head -n 1)"
printf '%s  %s\n' "$temperature" "$condition"
# Located by IP, wttr.in may give the place as its coordinates; its JSON format
# names the area there. Without a name, the row is left empty.
if [[ "$place" =~ ^-?[0-9.]+,-?[0-9.]+$ ]]; then
  place="$(curl -fsS --max-time 5 "https://wttr.in/$place?format=j1" 2>/dev/null |
    jq -r '.nearest_area[0] | .areaName[0].value // .region[0].value // empty' 2>/dev/null)"
fi
# The place without its region or country.
printf '%s\n' "${place%%,*}"
printf 'humidity %s · wind %s\n' "$humidity" "$wind"
