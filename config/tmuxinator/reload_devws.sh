#!/usr/bin/env bash
# Applies a devws update to the running workspaces without restarting what you
# work in (devws reload; ./install.sh runs it when tmux is up):
#
#   - reloads ~/.tmux.conf: formats, key bindings, colors, and the header's
#     ticker, which restarts itself whenever tmux.conf is sourced and turns
#     the working agents' spinner (agent_spinner.sh);
#   - restarts every workspace's sidebar (title devws-picker) and minimized
#     placeholders (role minimized-*), which run the scripts' new versions;
#   - stops the spinner an older devws ran for each working agent, now the
#     header's ticker's job;
#   - first, brings back every frozen workspace (suspend_workspace.sh).
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

# Frozen workspaces (suspend_workspace.sh) come back first: a frozen sidebar
# or placeholder respawned now would leave its old processes stopped.
"$scripts_dir/suspend_workspace.sh" resume --all

# respawn-pane keeps the pane and its options (a placeholder's role, label and
# hidden pane); without a command it reruns the pane's own start command. The
# title comes last: it can hold a "|" (Codex names its pane after the task).
while IFS='|' read -r session pane role title; do
  case "$session" in dev-*) ;; *) continue ;; esac
  if [ "$title" = devws-picker ]; then
    tmux respawn-pane -k -t "$pane" "$scripts_dir/session_picker.sh" && sidebars=$((sidebars + 1))
  elif [ "${role#minimized-}" != "$role" ]; then
    tmux respawn-pane -k -t "$pane" && placeholders=$((placeholders + 1))
  fi
done < <(tmux list-panes -a -F '#{session_name}|#{pane_id}|#{@devws_role}|#{pane_title}')

# An older devws's spinner: its process, and the frame it left on the session,
# which would hide the global one.
while IFS='|' read -r session ticker; do
  case "$session" in dev-*) ;; *) continue ;; esac
  [ -n "$ticker" ] || continue
  kill "$ticker" 2>/dev/null
  tmux set-option -qu -t "=$session:" @devws_agent_ticker
  tmux set-option -qu -t "=$session:" @devws_agent_frame
done < <(tmux list-sessions -F '#{session_name}|#{@devws_agent_ticker}')

printf 'devws: reloaded tmux.conf, %s sidebar(s), %s placeholder(s)\n' \
  "$sidebars" "$placeholders"
