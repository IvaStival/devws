#!/usr/bin/env bash
# One turn of the working agents' spinner, for every workspace at once: the
# frame goes to the global @devws_agent_frame, which tmux.conf's
# @devws_agent_icon shows in the agent's header and the status bar. The
# sidebar's ⠋ holds still: turning it made fzf draw its whole list again, and
# the terminal repaint it, for one character; the sidebar redraws when a
# state changes (refresh_devws_pickers.sh --reload).
#
#   agent_spinner.sh <turn>   (header_ticker.sh sources it and calls
#                             spin_agents twice a second)
#
# With no agent working it costs one tmux call.

spinner_frames=(⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏)
# Present while a devws popup (the menu) is open (devws_menu.sh): a redraw
# then shows the panes behind it for an instant, so the spinner holds still.
spinner_popup_flag="${TMPDIR:-/tmp}"
spinner_popup_flag="${spinner_popup_flag%/}/devws-popup-$(id -u)"

# <turn>: the frame to show is turn % 10.
spin_agents() {
  local turn="$1" states
  [ -e "$spinner_popup_flag" ] && return 0
  states="$(tmux list-panes -a -F '#{@devws_agent_state}' 2>/dev/null)" || return 0
  grep -qx working <<<"$states" || return 0
  tmux set-option -g @devws_agent_frame "${spinner_frames[turn % ${#spinner_frames[@]}]}" 2>/dev/null
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  spin_agents "${1:-0}"
fi
