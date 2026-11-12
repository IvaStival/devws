#!/usr/bin/env bash
# Clears the "working" state of agents whose turn was interrupted. Neither
# Claude Code nor Codex calls the Stop hook when the user interrupts a turn
# (Esc), so agent_state.sh never hears that it ended and the pane kept showing
# working until the next turn. Both record the interrupt in their transcript:
# Claude Code a user message "[Request interrupted by user…]", Codex a
# "turn_aborted" event.
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
# Lines read from the end of a transcript: enough to hold what an interrupt
# leaves after it, never the whole file.
tail_lines=50

# <transcript>: succeeds when the newest turn boundary in its tail is an
# interrupt. Boundaries: a prompt (Claude Code: a user message with text;
# Codex: task_started), the end of a turn (Codex: task_complete) and an
# interrupt. Tool results, tokens and settings lines are not boundaries.
interrupted() {
  [ -r "$1" ] || return 1
  tail -n "$tail_lines" "$1" 2>/dev/null | jq -rR '
    fromjson? |
    if .type == "event_msg" then
      .payload.type | select(. == "task_started" or . == "task_complete" or . == "turn_aborted")
    elif .type == "user" then
      .message.content |
      if type == "string" then "prompt"
      elif type == "array" then
        (map(select(.type == "text")) | first // empty | .text |
          if startswith("[Request interrupted by user") then "turn_aborted" else "prompt" end)
      else empty end
    else empty end' 2>/dev/null | tail -n 1 | grep -qx turn_aborted
}

sweep() {
  local session pane state transcript seen
  while IFS=$'\t' read -r session pane state transcript seen; do
    if [ "$state" != working ]; then
      # A sighting from a turn that ended another way must not count against
      # the next one.
      [ -z "$seen" ] || tmux set-option -p -u -t "$pane" @devws_interrupt_seen 2>/dev/null
      continue
    fi
    if [ -z "$transcript" ] || ! interrupted "$transcript"; then
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
