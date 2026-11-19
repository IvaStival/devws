#!/usr/bin/env bash
# Clears the "working" state of agents whose turn was interrupted. The
# programs call no Stop hook when the user interrupts a turn (Esc), so
# agent_state.sh never hears that it ended and the pane kept showing working
# until the next turn. Each program's module knows how its interrupt shows
# (interrupted, agents/*/agent.sh): in its transcript, or on its pane.
#
#   agent_interrupt.sh sweep   (header_ticker.sh, every 5 s)
#                              for each working agent pane, read the end of
#                              its transcript (@devws_transcript, recorded by
#                              agent_state.sh); when the newest turn boundary
#                              there is an interrupt, mark the agent idle
#
# Idle, not done: an interrupted turn is not finished work, so it posts no
# "done" alert. The hook sets working as a prompt is submitted, a moment
# before the transcript records it, so one sighting could undo a new turn: an
# interrupt counts once two sweeps in a row see it (@devws_interrupt_seen).

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=agents.sh
. "$script_dir/agents.sh"

# <transcript> <pane>: succeeds when a program's module sees an interrupt
# there; each knows only its own transcripts and panes.
interrupted() {
  local kind
  while IFS= read -r kind; do
    agent_hook "$kind" interrupted "$1" "$2" && return 0
  done < <(agent_kinds)
  return 1
}

sweep() {
  local session pane state transcript seen found
  while IFS=$'\t' read -r session pane state transcript seen; do
    if [ "$state" != working ]; then
      # A sighting from a turn that ended another way must not count against
      # the next one.
      [ -z "$seen" ] || tmux set-option -p -u -t "$pane" @devws_interrupt_seen 2>/dev/null
      continue
    fi
    found=""
    [ -z "$transcript" ] || ! interrupted "$transcript" "$pane" || found=1
    if [ -z "$found" ]; then
      [ -z "$seen" ] || tmux set-option -p -u -t "$pane" @devws_interrupt_seen 2>/dev/null
      continue
    fi
    if [ -z "$seen" ]; then
      tmux set-option -p -t "$pane" @devws_interrupt_seen 1 2>/dev/null
      continue
    fi
    tmux set-option -p -u -t "$pane" @devws_interrupt_seen 2>/dev/null
    "$script_dir/agent_status.sh" "$session" idle "$pane" >/dev/null 2>&1
  done < <(
    # One tmux call; only panes that are working or carry a sighting go on.
    tmux list-panes -a -F "#{session_name}	#{pane_id}	#{@devws_pane_state}	#{@devws_transcript}	#{@devws_interrupt_seen}" 2>/dev/null |
      awk -F '\t' '$3 == "working" || $5 != ""'
  )
}

case "${1:-}" in
  sweep) sweep ;;
  *)
    printf 'Usage: agent_interrupt.sh sweep\n' >&2
    exit 2
    ;;
esac
