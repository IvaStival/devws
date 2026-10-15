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
  # Each sidebar redraws its own rows (session_picker.sh), since it alone may
  # reload its fzf.
  tmux list-panes -a -F '#{pane_title} #{@devws_picker_rows}' 2>/dev/null |
    while read -r pane_title rows_file; do
      [ "$pane_title" = "devws-picker" ] && [ -n "$rows_file" ] || continue
      : > "$rows_file.stale"
    done
  exit 0
fi

tmux list-panes -a -F '#{pane_id} #{pane_title}' 2>/dev/null |
  while read -r pane_id pane_title; do
    [ "$pane_title" = "devws-picker" ] || continue
    tmux send-keys -t "$pane_id" Escape 2>/dev/null
  done
