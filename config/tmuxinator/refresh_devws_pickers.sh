#!/usr/bin/env bash
# Redraw every devws sidebar. Picker panes identify themselves by title, so
# this works across all sessions without relying on fixed window/pane indexes.
#
#   refresh_devws_pickers.sh            restart each sidebar (the workspaces
#                                       changed)
#   refresh_devws_pickers.sh --reload   redraw each sidebar's rows in place
#                                       (an agent's state changed)

set -u

if [ "${1:-}" = --reload ]; then
  # Every sidebar, on screen or not, so each is current when shown: its fzf
  # redraws the rows (session_picker.sh --rows). The mark stays when fzf is
  # not listening yet, for it to redraw once it starts.
  picker="$(cd "$(dirname "$0")" && pwd)/session_picker.sh"
  tmux list-panes -a -F '#{pane_title}|#{@devws_picker_rows}|#{@devws_picker_socket}' 2>/dev/null |
    while IFS='|' read -r pane_title rows_file socket; do
      [ "$pane_title" = "devws-picker" ] && [ -n "$rows_file" ] || continue
      : > "$rows_file.stale"
      [ -S "$socket" ] || continue
      # In the background: the agent's hook that changed the state must not
      # wait on the sidebars.
      curl -s --max-time 2 --unix-socket "$socket" http://localhost \
        -d "reload-sync('$picker' --rows)" >/dev/null 2>&1 &
    done
  exit 0
fi

tmux list-panes -a -F '#{pane_id} #{pane_title}' 2>/dev/null |
  while read -r pane_id pane_title; do
    [ "$pane_title" = "devws-picker" ] || continue
    tmux send-keys -t "$pane_id" Escape 2>/dev/null
  done
