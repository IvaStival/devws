#!/usr/bin/env bash
# Redraw every devws sidebar in place: the workspaces or an agent's state
# changed. Picker panes identify themselves by title, so this works across all
# sessions without relying on fixed window/pane indexes.
#
#   refresh_devws_pickers.sh [--reload]
#
# Every sidebar, on screen or not, so each is current when shown: its rows are
# drawn beside its fzf, which then reads them (session_picker.sh --redraw), so
# it never closes, flickers or stops answering keys and clicks; its cursor
# stays on its row. The mark stays when fzf is not listening yet, for it to
# redraw once it starts, and for a frozen workspace's, which redraws when it
# wakes (suspend_workspace.sh). --reload is the same; callers from before
# closed and restarted each sidebar without it.

set -u

picker="$(cd "$(dirname "$0")" && pwd)/session_picker.sh"
tmux list-panes -a -F '#{pane_title}|#{pane_id}|#{session_name}|#{@devws_frozen}|#{@devws_picker_rows}|#{@devws_picker_socket}' 2>/dev/null |
  while IFS='|' read -r pane_title pane session frozen rows_file socket; do
    [ "$pane_title" = "devws-picker" ] && [ -n "$rows_file" ] || continue
    : > "$rows_file.stale"
    [ -z "$frozen" ] && [ -S "$socket" ] || continue
    # In the background: the agent's hook that changed the state must not
    # wait on the sidebars.
    TMUX_PANE="$pane" DEVWS_PICKER_ROWS="$rows_file" DEVWS_PICKER_CURRENT="$session" \
      "$picker" --redraw "$socket" </dev/null >/dev/null 2>&1 &
  done
exit 0
