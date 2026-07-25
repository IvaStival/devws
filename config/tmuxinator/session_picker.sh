#!/usr/bin/env bash
# Persistent devws sidebar: workspace switcher plus workspace commands.

set -u

config="$HOME/.tmuxinator/.env"
[ -r "$config" ] && . "$config"

# Always title this script's own pane. Without an explicit target tmux titles
# the client's active pane, which may be the editor or terminal.
tmux select-pane -t "$TMUX_PANE" -T devws-picker 2>/dev/null

dev_sessions() {
  tmux list-sessions -F '#{session_name}' 2>/dev/null |
    awk '/^dev-/'
}

workspace_rows() {
  local current="$1"
  local session agent root branch state

  while IFS='|' read -r session agent root; do
    case "$session" in dev-*) ;; *) continue ;; esac
    agent="${agent:-unknown}"

    branch=""
    state="  "
    if [ -n "$root" ] && git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
      branch="$(git -C "$root" branch --show-current 2>/dev/null)"
      [ -n "$branch" ] || branch="detached"
      if [ -n "$(git -C "$root" status --porcelain 2>/dev/null)" ]; then
        state="\033[33m●\033[0m "
      else
        state="\033[32m✓\033[0m "
      fi
      branch="  $branch"
    fi

    if [ "$session" = "$current" ]; then
      printf 'workspace:%s\t%b\033[1;30;46m\033[1;33;46m*\033[1;30;46m %s  [%s]%s\033[0m\n' \
        "$session" "$state" "$session" "$agent" "$branch"
    else
      printf 'workspace:%s\t%b  %s  [%s]%s\n' \
        "$session" "$state" "$session" "$agent" "$branch"
    fi
  done < <(tmux list-sessions -F '#{session_name}|#{@devws_agent}|#{@devws_root}' 2>/dev/null)
}

refresh_pickers() {
  "$HOME/.tmuxinator/refresh_devws_pickers.sh"
}

choose_agent() {
  local agents="${DEVWS_AGENTS:-claude codex}"
  local agent

  agent="$(
    printf '%s\n' $agents |
      fzf --height=~10 --layout=reverse --prompt='Agent > ' \
        --header='Choose the agent for the new workspace'
  )"
  [ -n "$agent" ] && printf '%s\n' "$agent"
}

agent_pane() {
  local session="$1"
  local pane role command

  while IFS='|' read -r pane role command; do
    if [ "$role" = agent ]; then
      printf '%s\n' "$pane"
      return 0
    fi
  done < <(tmux list-panes -t "$session" \
    -F '#{pane_id}|#{@devws_role}|#{pane_current_command}' 2>/dev/null)

  # Compatibility for sessions created before agent_runner.sh was introduced.
  while IFS='|' read -r pane role command; do
    case "$command" in
      claude|codex|node)
        printf '%s\n' "$pane"
        return 0
        ;;
    esac
  done < <(tmux list-panes -t "$session" \
    -F '#{pane_id}|#{@devws_role}|#{pane_current_command}' 2>/dev/null)
  return 1
}

switch_agent() {
  local session pane agent root
  session="$(tmux display-message -p '#S')"
  pane="$(agent_pane "$session")" || return
  agent="$(choose_agent)" || return
  [ -n "$agent" ] || return
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"

  tmux set-option -t "$session" @devws_agent "$agent"
  tmux respawn-pane -k -t "$pane" -c "${root:-$PWD}" \
    "$HOME/.tmuxinator/agent_runner.sh $agent"
  refresh_pickers
}

restart_agent() {
  local session pane agent root
  session="$(tmux display-message -p '#S')"
  pane="$(agent_pane "$session")" || return
  agent="$(tmux show-options -t "$session" -v @devws_agent 2>/dev/null)"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"
  [ -n "$agent" ] || agent="${DEVWS_DEFAULT_AGENT:-claude}"

  tmux respawn-pane -k -t "$pane" -c "${root:-$PWD}" \
    "$HOME/.tmuxinator/agent_runner.sh $agent"
}

new_terminal() {
  local session root terminal_pane
  session="$(tmux display-message -p '#S')"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"
  terminal_pane="$(
    tmux list-panes -t "$session" -F '#{pane_id}|#{pane_title}' |
      awk -F '|' '$2 == "terminal" { print $1; exit }'
  )"
  [ -n "$terminal_pane" ] || return
  tmux split-window -h -t "$terminal_pane" -c "${root:-$PWD}"
}

open_folder() {
  local session root choice
  session="$(tmux display-message -p '#S')"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"
  [ -d "$root" ] || return

  choice="$(
    printf '%s\n' 'Finder' 'Zed' 'VS Code' |
      fzf --height=~10 --layout=reverse --prompt='Open with > '
  )"
  case "$choice" in
    Finder) open "$root" >/dev/null 2>&1 & ;;
    Zed) command -v zed >/dev/null && zed "$root" >/dev/null 2>&1 & ;;
    'VS Code') command -v code >/dev/null && code "$root" >/dev/null 2>&1 & ;;
  esac
}

new_workspace() {
  local entered_path project_root agent

  printf '\033c'
  printf 'New workspace\n\n'
  read -e -r -p 'Folder path: ' entered_path
  [ -n "$entered_path" ] || return

  # Bash readline supplies Tab completion. Resolve the result before passing it
  # to tmuxinator so relative paths and a leading "~/" behave as expected.
  case "$entered_path" in
    "~") entered_path="$HOME" ;;
    "~/"*) entered_path="$HOME/${entered_path#\~/}" ;;
  esac
  if ! project_root="$(cd "$entered_path" 2>/dev/null && pwd -P)"; then
    printf 'Folder does not exist: %s\nPress Enter to return…' "$entered_path"
    read -r
    return
  fi

  agent="$(choose_agent)" || return
  [ -n "$agent" ] || return

  tmuxinator start dev project_root="$project_root" agent="$agent"
  refresh_pickers
}

close_workspace() {
  local current selected target quoted_target
  current="$(tmux display-message -p '#S' 2>/dev/null)"
  selected="$(
    dev_sessions |
      awk -v cur="$current" '{ print ($0 == cur ? "* " : "  ") $0 }' |
      fzf --height=100% --layout=reverse \
        --header='Close workspace (Esc: cancel)' --prompt='Close > ' \
        --info=inline --no-separator --border=none --margin=0 --padding=0
  )"
  [ -n "$selected" ] || return

  target="${selected#??}"
  printf -v quoted_target '%q' "$target"
  # Run through the tmux server so closing the workspace containing this
  # sidebar does not terminate the refresh command along with the pane.
  tmux run-shell -b \
    "tmux kill-session -t $quoted_target; '$HOME/.tmuxinator/refresh_devws_pickers.sh'"
}

while tmux list-sessions >/dev/null 2>&1; do
  current="$(tmux display-message -p '#S' 2>/dev/null)"
  selected="$(
    {
      printf 'new\t\033[32m+ New workspace\033[0m\n'
      printf 'switch-agent\t\033[36m⇄ Switch agent\033[0m\n'
      printf 'restart-agent\t\033[36m↻ Restart agent\033[0m\n'
      printf 'new-terminal\t\033[36m▣ New terminal\033[0m\n'
      printf 'open-folder\t\033[36m⌂ Open folder\033[0m\n'
      printf 'close\t\033[31m× Close workspace\033[0m\n'
      printf 'spacer\t\n'
      printf 'divider\t\033[2m────────────────────────\033[0m\n'
      workspace_rows "$current"
    } |
      fzf --height=100% --layout=reverse --ansi \
        --delimiter=$'\t' --with-nth=2.. \
        --header='WORKSPACE COMMANDS / OPEN WORKSPACES' --prompt='> ' \
        --info=inline --no-separator --border=none --margin=0 --padding=0
  )"

  case "${selected%%$'\t'*}" in
    new) new_workspace ;;
    switch-agent) switch_agent ;;
    restart-agent) restart_agent ;;
    new-terminal) new_terminal ;;
    open-folder) open_folder ;;
    close) close_workspace ;;
    spacer|divider) ;;
    '') ;;
    workspace:*)
      workspace="${selected%%$'\t'*}"
      tmux switch-client -t "${workspace#workspace:}" 2>/dev/null
      ;;
  esac
done
