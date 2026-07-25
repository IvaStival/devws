#!/usr/bin/env bash
# Redraw every devws sidebar. Picker panes identify themselves by title, so
# this works across all sessions without relying on fixed window/pane indexes.
tmux list-panes -a -F '#{pane_id} #{pane_title}' 2>/dev/null |
  while read -r pane_id pane_title; do
    [ "$pane_title" = "devws-picker" ] || continue
    tmux send-keys -t "$pane_id" Escape 2>/dev/null
  done
