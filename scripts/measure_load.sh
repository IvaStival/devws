#!/usr/bin/env bash
# Measures what devws costs while it runs: the CPU each part used over a few
# seconds, as a share of one core, from two process snapshots. Per workspace,
# by kind of pane (agent, editor, sidebar, terminal), with whether it is
# frozen (@devws_frozen) and whether its processes really are stopped; then
# the shared parts: the tmux server, the header ticker and iTerm2.
#
#   scripts/measure_load.sh [seconds]   default 20
#
# Run it outside any sandbox that hides other processes.

set -euo pipefail

# Named modes add isolated hook probes and live latency/rendering comparisons.
# Numeric arguments retain the original per-workspace CPU report.
case "${1:-}" in
  live|hooks|rendering|tokens|state)
    command -v python3 >/dev/null || { echo 'measure_load: python3 is required for named modes' >&2; exit 1; }
    exec python3 "$(dirname "$0")/measure_load.py" "$@"
    ;;
  --help)
    printf 'Usage: %s [seconds] | live | hooks | rendering | tokens | state [options]\n' "$0"
    exit 0
    ;;
esac

seconds="${1:-20}"
case "$seconds" in '' | *[!0-9]* | 0) printf 'Usage: %s [seconds]\n' "$0" >&2; exit 2 ;; esac
tmux list-sessions >/dev/null 2>&1 || { printf 'No tmux server is running.\n' >&2; exit 1; }

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/devws-load.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

snapshot() {
  ps -axo pid=,ppid=,stat=,time= > "$1"
}

tab=$'\t'
tmux list-panes -a -F "#{session_name}${tab}#{@devws_frozen}${tab}#{@devws_role}${tab}#{pane_title}${tab}#{pane_pid}" \
  > "$work_dir/panes"
server="$(tmux display-message -p '#{pid}')"
ticker="$(pgrep -f 'header_ticker.sh --run' | head -n 1 || true)"
terminal_app="$(pgrep -x iTerm2 | head -n 1 || true)"

snapshot "$work_dir/before"
sleep "$seconds"
snapshot "$work_dir/after"

printf 'CPU over %s s, as %% of one core\n\n' "$seconds"
awk -F '\t' -v seconds="$seconds" -v server="$server" -v ticker="$ticker" -v terminal_app="$terminal_app" '
  # ps prints [[dd-]hh:]mm:ss.cc; minutes may exceed 59.
  function centiseconds(time,    parts, count, days) {
    days = 0
    if (time ~ /-/) { split(time, parts, "-"); days = parts[1]; time = parts[2] }
    count = split(time, parts, ":")
    if (count == 2) return ((days * 24 * 60 + parts[1]) * 60 + parts[2]) * 100
    return (((days * 24 + parts[1]) * 60 + parts[2]) * 60 + parts[3]) * 100
  }
  # A pane process and everything below it: the CPU it used, how many
  # processes, and how many are stopped.
  function tree(pid,    queue, head, tail, list, count, i, current) {
    tree_cpu = 0; tree_count = 0; tree_stopped = 0
    queue[1] = pid; tail = 1
    for (head = 1; head <= tail; head++) {
      current = queue[head]
      if (!(current in stat)) continue
      tree_count++
      if (stat[current] ~ /^T/) tree_stopped++
      if (current in used) tree_cpu += used[current]
      count = split(children[current], list, " ")
      for (i = 1; i <= count; i++) queue[++tail] = list[i]
    }
  }
  function kind(role, title) {
    if (title == "devws-picker") return "sidebar"
    if (role ~ /agent$/) return "agent"
    if (role ~ /editor$/) return "editor"
    if (role ~ /terminal$/ || role ~ /^(minimized-)?extra$/) return "terminal"
    if (role ~ /database$/) return "database"
    return "other"
  }
  FILENAME ~ /before$/ { split($0, field, " "); start[field[1]] = centiseconds(field[4]); next }
  FILENAME ~ /after$/ {
    split($0, field, " ")
    stat[field[1]] = field[3]
    children[field[2]] = children[field[2]] " " field[1]
    if (field[1] in start) used[field[1]] = centiseconds(field[4]) - start[field[1]]
    next
  }
  {
    session = $1; flag[session] = $2
    pane_kind = kind($3, $4)
    tree($5)
    cpu[session, pane_kind] += tree_cpu
    processes[session] += tree_count
    stopped[session] += tree_stopped
    sessions[session] = 1
  }
  END {
    split("agent editor sidebar terminal other", kinds, " ")
    printf "%-26s %-7s %-9s", "workspace", "frozen", "stopped"
    for (i = 1; i <= 5; i++) printf " %8s", kinds[i]
    printf "\n"
    for (session in sessions) {
      printf "%-26s %-7s %4d/%-4d", session, (flag[session] == "" ? "no" : "yes"), stopped[session], processes[session]
      for (i = 1; i <= 5; i++) printf " %7.1f%%", cpu[session, kinds[i]] / seconds
      printf "\n"
    }
    printf "\n"
    if (server in used) printf "%-26s %7.1f%%\n", "tmux server", used[server] / seconds
    if (ticker in used) { tree(ticker); printf "%-26s %7.1f%%\n", "header ticker", tree_cpu / seconds }
    if (terminal_app in used) printf "%-26s %7.1f%%\n", "iTerm2", used[terminal_app] / seconds
  }' "$work_dir/before" "$work_dir/after" "$work_dir/panes"
