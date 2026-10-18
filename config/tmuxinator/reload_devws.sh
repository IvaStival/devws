#!/usr/bin/env bash
# Applies a devws update to the running workspaces without restarting what you
# work in (devws reload; ./install.sh runs it when tmux is up):
#
#   - reloads ~/.tmux.conf: formats, key bindings, colors, and the header's
#     ticker, which restarts itself whenever tmux.conf is sourced;
#   - restarts every workspace's sidebar (title devws-picker) and minimized
#     placeholders (role minimized-*), which run the scripts' new versions;
#   - restarts the spinner of every workspace whose agent is working.
#
# Agent, editor, terminal and database panes keep running: what they read only
# at start (the agents' hooks, the editor config, dev.yml) applies to new or
# restarted ones. Only devws workspaces (sessions dev-*) are touched, apart
# from tmux.conf, which is global.
#
#   reload_devws.sh

set -u

scripts_dir="$(cd "$(dirname "$0")" && pwd)"

if ! tmux list-sessions >/dev/null 2>&1; then
  printf 'devws: no tmux server, nothing to reload\n'
  exit 0
fi

tmux source-file "$HOME/.tmux.conf" || {
  printf 'devws: ~/.tmux.conf has errors; fix them and reload again\n' >&2
  exit 1
}

sidebars=0
placeholders=0
spinners=0

# respawn-pane keeps the pane and its options (a placeholder's role, label and
# hidden pane); without a command it reruns the pane's own start command.
while IFS='|' read -r session pane title role; do
  case "$session" in dev-*) ;; *) continue ;; esac
  if [ "$title" = devws-picker ]; then
    tmux respawn-pane -k -t "$pane" "$scripts_dir/session_picker.sh" && sidebars=$((sidebars + 1))
  elif [ "${role#minimized-}" != "$role" ]; then
    tmux respawn-pane -k -t "$pane" && placeholders=$((placeholders + 1))
  fi
done < <(tmux list-panes -a -F '#{session_name}|#{pane_id}|#{pane_title}|#{@devws_role}')

# A spinner is stopped, then started again by agent_status.sh as for any
# working agent.
while IFS='|' read -r session state ticker; do
  case "$session" in dev-*) ;; *) continue ;; esac
  [ "$state" = working ] || continue
  [ -n "$ticker" ] && kill "$ticker" 2>/dev/null
  tmux set-option -u -t "=$session:" @devws_agent_ticker
  "$scripts_dir/agent_status.sh" "$session" working && spinners=$((spinners + 1))
done < <(tmux list-sessions -F '#{session_name}|#{@devws_agent_state}|#{@devws_agent_ticker}')

printf 'devws: reloaded tmux.conf, %s sidebar(s), %s placeholder(s), %s spinner(s)\n' \
  "$sidebars" "$placeholders" "$spinners"
