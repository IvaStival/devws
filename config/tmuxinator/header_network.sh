#!/usr/bin/env bash
# The header's network column (macOS sources), three rows:
#
#   ↓ 2.4 MB/s  ↑ 120 kB/s
#   Wi-Fi ▂▄▆_ -60 dBm · 5 GHz
#   192.168.1.17 · ping 7 ms
#
#   header_network.sh <state folder>
#
# Speeds compare the default interface's byte counters with the previous call
# (kept in the state folder). The Wi-Fi signal (system_profiler, which takes
# seconds) and the ping (to 1.1.1.1) refresh at most once a minute. macOS hides
# the Wi-Fi network's name from apps without Location access, so it is not
# shown. Offline, the rows say so.

set -u

state_dir="${1:?Usage: header_network.sh <state folder>}"
counters_file="$state_dir/network.counters"
link_file="$state_dir/network.link"
link_interval=60

interface="$(route -n get default 2>/dev/null | awk '/interface:/ { print $2 }')"
if [ -z "$interface" ]; then
  printf '#[dim]offline#[nodim]\n\n\n'
  exit 0
fi

# bytes a second, as 980 B/s, 12 kB/s, 2.4 MB/s
rate() {
  awk -v bytes="$1" 'BEGIN {
    if (bytes < 1000) printf "%d B/s", bytes
    else if (bytes < 1000000) printf "%d kB/s", bytes / 1000
    else printf "%.1f MB/s", bytes / 1000000 }'
}

# The interface's received and sent bytes, from its link-level line.
read -r received sent < <(netstat -ibn -I "$interface" 2>/dev/null |
  awk 'NR > 1 && $3 ~ /^<Link/ { print $7, $10; exit }')
# DEVWS_NETWORK_NOW (epoch seconds) replaces the current time, for tests.
now="${DEVWS_NETWORK_NOW:-$(date +%s)}"
speeds="↓ —  ↑ —"
if [ -n "${received:-}" ]; then
  if read -r previous_interface previous_time previous_received previous_sent 2>/dev/null < "$counters_file" &&
    [ "$previous_interface" = "$interface" ] && [ "$now" -gt "$previous_time" ] &&
    [ "$received" -ge "$previous_received" ] && [ "$sent" -ge "$previous_sent" ]; then
    elapsed=$((now - previous_time))
    speeds="↓ $(rate $(((received - previous_received) / elapsed)))  ↑ $(rate $(((sent - previous_sent) / elapsed)))"
  fi
  printf '%s %s %s %s\n' "$interface" "$now" "$received" "$sent" > "$counters_file"
fi

# Signal strength as four bars, from dBm: -50 and up all lit, -80 and down one.
signal_bars() {
  awk -v dbm="$1" 'BEGIN {
    lit = (dbm >= -55) ? 4 : (dbm >= -65) ? 3 : (dbm >= -75) ? 2 : 1
    split("▂ ▄ ▆ █", bars, " ")
    for (bar = 1; bar <= 4; bar++) printf "%s", (bar <= lit ? bars[bar] : "_") }'
}

link_rows() {
  local wifi signal band address ping
  wifi="$(system_profiler SPAirPortDataType 2>/dev/null |
    awk '/Current Network Information:/ { current = 1 }
         current && /Signal \/ Noise:/ { signal = $4 }
         current && /Channel:/ { band = $3; gsub(/[(,]/, "", band) }
         current && /Other Local Wi-Fi Networks:/ { exit }
         END { if (signal != "") print signal, band }')"
  if [ -n "$wifi" ]; then
    read -r signal band <<< "$wifi"
    printf 'Wi-Fi %s %s dBm%s\n' "$(signal_bars "$signal")" "$signal" "${band:+ · ${band/GHz/ GHz}}"
  else
    printf '%s\n' "$interface"
  fi
  address="$(ipconfig getifaddr "$interface" 2>/dev/null)"
  ping="$(ping -c 1 -t 2 1.1.1.1 2>/dev/null | awk -F'time=' '/time=/ { split($2, ms, " "); printf "%.0f", ms[1] }')"
  printf '%s · %s\n' "${address:-no address}" "${ping:+ping $ping ms}"
}

link_age=$((now - $(stat -f %m "$link_file" 2>/dev/null || echo 0)))
if [ "$link_age" -ge "$link_interval" ]; then
  link_rows > "$link_file.new" && mv "$link_file.new" "$link_file"
fi
printf '%s\n' "$speeds"
cat "$link_file" 2>/dev/null
