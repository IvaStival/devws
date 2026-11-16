#!/usr/bin/env bash
# Starts one benchmark run: prepares it (bench_team.sh), sets it up
# (bench_setup.sh), opens its team without switching your tmux client,
# confirms Claude's folder-trust prompt and sends the fixture's first message
# (SCRIPT.md) once the leader's first turn is over. Prints
# "<name> <task> <session> <leader pane>".
#
#   BENCH_SOURCE=<repo> bench_start.sh <fixture> <name> <on|off>

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[[ $# -eq 3 && ( "$3" == on || "$3" == off ) ]] || {
  printf 'Usage: BENCH_SOURCE=<repo> %s <fixture> <name> <on|off>\n' "$(basename "$0")" >&2
  exit 2
}
fixture="$1" name="$2" graph="$3"
folder="$HOME/Projects/devws-bench/$fixture-$name"
# The first message: the quoted block of SCRIPT.md's first answer.
message="$(sed -n 's/^   > //p' "$ROOT/agentic/bench/fixtures/$fixture/SCRIPT.md")"
[[ -n "$message" ]] || { printf '%s has no first message in SCRIPT.md\n' "$fixture" >&2; exit 1; }

"$ROOT/scripts/bench_team.sh" "$fixture" reviewed "$folder" >/dev/null
task="$("$ROOT/scripts/bench_setup.sh" "$folder" "$fixture" "$graph" | tail -1)"
session="$(env -u TMUX devws agentic --leader claude "$folder" | tail -1)"
pane="$(tmux list-panes -s -t "=$session" -F '#{pane_id} #{@devws_role}' | awk '$2 == "agent" { print $1 }')"

for _ in $(seq 60); do
  tmux capture-pane -p -t "$pane" | grep -q "Yes, I trust this folder" && break
  sleep 1
done
tmux send-keys -t "$pane" Down
sleep 0.3
tmux send-keys -t "$pane" Enter
# Claude prints "<verb> for <time>" when a turn ends.
for _ in $(seq 180); do
  sleep 2
  tmux capture-pane -p -t "$pane" | grep -qE "[A-Z][a-zé]+ed for [0-9]" && break
done
sleep 2
tmux send-keys -t "$pane" C-u
sleep 0.2
tmux send-keys -t "$pane" -l "$message"
sleep 0.5
tmux send-keys -t "$pane" Enter
printf '%s %s %s %s\n' "$name" "$task" "$session" "$pane"
