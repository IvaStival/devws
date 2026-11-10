#!/usr/bin/env bash
# One turn of the working agents' spinner, for every workspace at once: the
# frame goes to the global @devws_agent_frame, which tmux.conf's
# @devws_agent_icon shows in the agent's header and the status bar. Sidebars
# on screen turn theirs too: fzf reloads its drawn rows with the frame put in
# (spin_sidebars), without building the rows again.
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
  spin_sidebars "${spinner_frames[turn % ${#spinner_frames[@]}]}"
}

# The sidebars someone sees (their window is shown in an attached workspace)
# that show a spinner reload their rows file with <frame> in each spinner's
# place. Each turn makes the terminal repaint the sidebar, so a sidebar nobody
# sees holds still; the file keeps ⠋ for the next real redraw.
spin_sidebars() {
  local frame="$1" title attached window_active rows_file socket
  tmux list-panes -a -F '#{pane_title}|#{session_attached}|#{window_active}|#{@devws_picker_rows}|#{@devws_picker_socket}' 2>/dev/null |
    while IFS='|' read -r title attached window_active rows_file socket; do
      [ "$title" = devws-picker ] && [ "$attached" != 0 ] && [ "$window_active" = 1 ] || continue
      # Matched as bytes, whatever the locale: the frames are U+2807..U+283C,
      # three bytes each.
      [ -S "$socket" ] && LC_ALL=C grep -aqF "${spinner_frames[@]/#/-e}" "$rows_file" 2>/dev/null || continue
      curl -s --max-time 1 --unix-socket "$socket" http://localhost \
        -d "reload-sync(perl -pe 's/\\xe2\\xa0[\\x8b\\x99\\xb9\\xb8\\xbc\\xb4\\xa6\\xa7\\x87\\x8f]/$frame/g' '$rows_file')" \
        >/dev/null 2>&1 &
    done
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  spin_agents "${1:-0}"
fi
