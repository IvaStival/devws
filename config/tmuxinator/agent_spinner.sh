#!/usr/bin/env bash
# One turn of the working agents' spinner, for every workspace at once: the
# frame goes to the global @devws_agent_frame, which tmux.conf's
# @devws_agent_icon shows in the agent's header and the status bar, and into
# the rows of each sidebar a client shows. A sidebar no one sees is left alone;
# it still gets every state change (refresh_devws_pickers.sh --reload), so it
# is current the moment it is shown, and the next turn spins it.
#
#   agent_spinner.sh <turn>   (header_ticker.sh sources it and calls
#                             spin_agents four times a second)
#
# With no agent working it costs one tmux call.

spinner_frames=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏)

# <turn>: the frame to show is turn % 10.
spin_agents() {
  local turn="$1" panes frame attached window_active title socket rows
  panes="$(tmux list-panes -a -F \
    '#{@devws_agent_state}|#{session_attached}|#{window_active}|#{pane_title}|#{@devws_picker_socket}|#{@devws_picker_rows}' \
    2>/dev/null)" || return 0
  case "$panes" in
    working\|* | *$'\n'working\|*) ;;
    *) return 0 ;;
  esac
  frame="${spinner_frames[turn % ${#spinner_frames[@]}]}"
  tmux set-option -g @devws_agent_frame "$frame" 2>/dev/null
  # The sidebar's rows hold the first frame (⠋) for fzf to swap.
  while IFS='|' read -r _ attached window_active title socket rows; do
    [ "$title" = devws-picker ] && [ "$attached" != 0 ] && [ "$window_active" = 1 ] || continue
    [ -S "$socket" ] && [ -n "$rows" ] || continue
    curl -s --max-time 1 --unix-socket "$socket" http://localhost \
      -d "reload-sync(sed 's/⠋/$frame/g' '$rows')" >/dev/null 2>&1
  done <<< "$panes"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  spin_agents "${1:-0}"
fi
