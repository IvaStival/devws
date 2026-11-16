#!/usr/bin/env bash
# The machine's usage, for the header's SYSTEM group. Prints three
# tmux-styled rows: CPU, memory, then disk, battery and load. Linux reads
# /proc and /sys (DEVWS_PROC, DEVWS_SYS in tests); macOS, which has no /proc,
# its own tools.
#
#   header_system.sh <cpu history file>
#
# The history file keeps the last 12 CPU samples for the sparkline. A source
# that is missing (no battery) leaves its part out. The level colors come from
# DEVWS_COLOR_OK, DEVWS_COLOR_WARN and DEVWS_COLOR_STRESS (header_bar.sh).

set -u
# A decimal point whatever the user's locale (pt_BR would print 6,4G).
# LC_ALL would override LC_NUMERIC: its characters stay, its numbers go.
if [ -n "${LC_ALL:-}" ]; then
  export LC_CTYPE="$LC_ALL"
  unset LC_ALL
fi
export LC_NUMERIC=C

history_file="${1:?Usage: header_system.sh <cpu history file>}"
history_size=12
proc="${DEVWS_PROC:-/proc}"
sys="${DEVWS_SYS:-/sys}"
linux=""
[ -r "$proc/stat" ] && linux=1

# The bar (header_bar.sh, in the level's color) and the percentage: ▓▓▓░░░░░░░  30%
meter() {
  printf '%s %3s%%' "$("$(dirname "$0")/header_bar.sh" "$1")" "$1"
}

# Linux: the busy share of the CPU ticks since the last sample, kept beside
# the history (since boot the first time). iowait counts as idle.
linux_cpu_percent() {
  local sample_file="$history_file.stat" now previous
  now="$(awk '/^cpu / { idle = $5 + $6; total = 0; for (field = 2; field <= NF; field++) total += $field; print total, idle; exit }' "$proc/stat")"
  previous="$(cat "$sample_file" 2>/dev/null || echo "0 0")"
  printf '%s\n' "$now" > "$sample_file"
  awk -v now="$now" -v previous="$previous" 'BEGIN {
    split(now, current, " "); split(previous, before, " ")
    total = current[1] - before[1]; idle = current[2] - before[2]
    if (total <= 0) { print 0; exit }
    print int((total - idle) * 100 / total + 0.5) }'
}

# Linux: what is not available is in use, as free(1) counts it.
linux_memory_usage() {
  awk '/^MemTotal:/ { total = $2 } /^MemAvailable:/ { available = $2 } END {
    used = total - available
    printf "%d %.1f/%.1fG\n", used * 100 / total + 0.5, used / 1048576, total / 1048576 }' "$proc/meminfo"
}

linux_battery_percent() {
  local capacity
  for capacity in "$sys"/class/power_supply/BAT*/capacity; do
    [ -r "$capacity" ] && { cat "$capacity"; return; }
  done
}

cpu_percent() {
  [ -n "$linux" ] && { linux_cpu_percent; return; }
  local cpus
  cpus="$(sysctl -n hw.ncpu)"
  ps -A -o %cpu= | awk -v cpus="$cpus" '{ total += $1 } END {
    percent = int(total / cpus + 0.5); print (percent > 100 ? 100 : percent) }'
}

# Memory in use as Activity Monitor counts it: app (active), wired and
# compressed pages. Prints "<percent> <used>/<total>G".
memory_usage() {
  [ -n "$linux" ] && { linux_memory_usage; return; }
  local total
  total="$(sysctl -n hw.memsize)"
  vm_stat | awk -v total="$total" '
    /page size of/ { page = $8 }
    /^Pages active:/ || /^Pages wired down:/ || /^Pages occupied by compressor:/ {
      gsub(/\./, "", $NF); pages += $NF }
    END {
      used = pages * page
      printf "%d %.1f/%.1fG\n", used * 100 / total, used / 1073741824, total / 1073741824 }'
}

# On macOS / is a small read-only system volume; the files live on the data
# volume. Used space is what is not available, as Finder counts it.
disk_percent() {
  local volume=/
  [ -d /System/Volumes/Data ] && volume=/System/Volumes/Data
  df -k "$volume" | awk 'NR == 2 { print int(($2 - $4) * 100 / $2 + 0.5) }'
}

battery_percent() {
  [ -n "$linux" ] && { linux_battery_percent; return; }
  pmset -g batt 2>/dev/null | awk '/InternalBattery/ { match($0, /[0-9]+%/); print substr($0, RSTART, RLENGTH - 1) }'
}

load_average() {
  [ -n "$linux" ] && { awk '{ print $1 }' "$proc/loadavg"; return; }
  sysctl -n vm.loadavg | awk '{ print $2 }'
}

# Appends the sample and prints the sparkline of the kept ones.
sparkline() {
  local samples bars=(▁ ▂ ▃ ▄ ▅ ▆ ▇ █) line="" sample
  samples="$( { cat "$history_file" 2>/dev/null; printf ' %s' "$1"; } | tr -s ' ' '\n' | grep . | tail -n "$history_size" | tr '\n' ' ')"
  printf '%s\n' "$samples" > "$history_file"
  for sample in $samples; do
    line+="${bars[sample * 7 / 100]}"
  done
  printf '%s' "$line"
}

cpu="$(cpu_percent)"
read -r memory memory_amount <<< "$(memory_usage)"
printf 'CPU %s  #[dim]%s#[nodim]\n' "$(meter "$cpu")" "$(sparkline "$cpu")"
printf 'MEM %s  %s\n' "$(meter "$memory")" "$memory_amount"

machine="DSK $(meter "$(disk_percent)")"
battery="$(battery_percent)"
[ -n "$battery" ] && machine+="  BAT ${battery}%"
machine+="  LOAD $(load_average)"
printf '%s\n' "$machine"
