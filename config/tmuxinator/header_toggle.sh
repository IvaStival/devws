#!/usr/bin/env bash
# Hides the header's rows, leaving tmux's own line (workspace, windows, clock),
# or brings them back: the header's minimize, like the panes' one. Bound to
# C-b C-s and a click on the header (tmux.conf), and in the devws menu.
#
#   header_toggle.sh [toggle|hide|show]
#
# The global option @devws_header_hidden records it, so a reload of tmux.conf
# keeps the header hidden, and header_ticker.sh rests meanwhile.

set -u

action="${1:-toggle}"
lines="$(tmux show-option -gqv @devws_header_lines)"
lines="${lines:-5}"

if [ "$action" = toggle ]; then
  if [ -n "$(tmux show-option -gqv @devws_header_hidden)" ]; then
    action=show
  else
    action=hide
  fi
fi

case "$action" in
  hide) tmux set-option -g @devws_header_hidden 1 \; set-option -g status on ;;
  show) tmux set-option -gu @devws_header_hidden \; set-option -g status "$lines" ;;
  *)
    printf 'Unknown header action: %s (toggle, hide, show)\n' "$action" >&2
    exit 2
    ;;
esac
