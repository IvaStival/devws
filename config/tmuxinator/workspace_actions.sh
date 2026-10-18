#!/usr/bin/env bash
# The devws workspace actions, shared by the sidebar (session_picker.sh) and
# the tabbed menu (devws_menu.sh). Source this file; each action works on the
# client's current workspace and asks what it needs with fzf or a prompt.

# Agents and editors to offer, and the defaults.
config="$HOME/.tmuxinator/.env"
[ -r "$config" ] && . "$config"
# shellcheck source=agents.sh
. "$HOME/.tmuxinator/agents.sh"

dev_sessions() {
  tmux list-sessions -F '#{session_name}' 2>/dev/null |
    awk '/^dev-/'
}

refresh_pickers() {
  "$HOME/.tmuxinator/refresh_devws_pickers.sh"
}

save_state() {
  "$HOME/.tmuxinator/save_state.sh"
}

# [header]: what the agent is for.
choose_agent() {
  local agents="${DEVWS_AGENTS:-claude codex}"
  local header="${1:-Choose the agent for the new workspace}"
  local agent

  agent="$(
    printf '%s\n' $agents |
      fzf --height=~10 --layout=reverse --prompt='Agent > ' \
        --header="$header"
  )"
  [ -n "$agent" ] && printf '%s\n' "$agent"
}

choose_editor() {
  local editors="${DEVWS_EDITORS:-nvim vim code zed}"
  local editor

  editor="$(
    printf '%s\n' $editors |
      fzf --height=~10 --layout=reverse --prompt='Editor > ' \
        --header='Choose the editor for the workspace'
  )"
  [ -n "$editor" ] && printf '%s\n' "$editor"
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

editor_pane() {
  local session="$1"
  local pane role command

  while IFS='|' read -r pane role command; do
    if [ "$role" = editor ]; then
      printf '%s\n' "$pane"
      return 0
    fi
  done < <(tmux list-panes -t "$session" \
    -F '#{pane_id}|#{@devws_role}|#{pane_current_command}' 2>/dev/null)

  # Compatibility for workspaces created before editor_runner.sh existed.
  while IFS='|' read -r pane role command; do
    case "$command" in
      nvim|vim|code|zed)
        printf '%s\n' "$pane"
        return 0
        ;;
    esac
  done < <(tmux list-panes -t "$session" \
    -F '#{pane_id}|#{@devws_role}|#{pane_current_command}' 2>/dev/null)
  return 1
}

# Agent and terminal actions need the real panes, which live in a hidden
# window while minimized, so they are expanded first.
expand_minimized_panes() {
  "$HOME/.tmuxinator/minimize_panes.sh" "$1" restore
}

switch_agent() {
  local session pane agent root
  session="$(tmux display-message -p '#S')"
  expand_minimized_panes "$session" || return
  pane="$(agent_pane "$session")" || return
  agent="$(choose_agent)" || return
  [ -n "$agent" ] || return
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"

  tmux set-option -t "$session" @devws_agent "$agent"
  tmux respawn-pane -k -t "$pane" -c "${root:-$PWD}" \
    "$HOME/.tmuxinator/agent_runner.sh $agent"
  "$HOME/.tmuxinator/agent_status.sh" "$session" idle
  save_state
  refresh_pickers
}

switch_editor() {
  local session pane editor root
  session="$(tmux display-message -p '#S')"
  pane="$(editor_pane "$session")" || return
  editor="$(choose_editor)" || return
  [ -n "$editor" ] || return
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"

  tmux set-option -t "$session" @devws_editor "$editor"
  tmux respawn-pane -k -t "$pane" -c "${root:-$PWD}" \
    "$HOME/.tmuxinator/editor_runner.sh $editor"
  save_state
  refresh_pickers
}

restart_agent() {
  local session pane agent root
  session="$(tmux display-message -p '#S')"
  expand_minimized_panes "$session" || return
  pane="$(agent_pane "$session")" || return
  agent="$(tmux show-options -t "$session" -v @devws_agent 2>/dev/null)"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"
  [ -n "$agent" ] || agent="${DEVWS_DEFAULT_AGENT:-claude}"

  tmux respawn-pane -k -t "$pane" -c "${root:-$PWD}" \
    "$HOME/.tmuxinator/agent_runner.sh $agent"
  "$HOME/.tmuxinator/agent_status.sh" "$session" idle
}

initialize_agent() {
  local session agent kind root message
  session="$(tmux display-message -p '#S')"
  agent="$(tmux show-options -t "$session" -v @devws_agent 2>/dev/null)"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"
  agent="${agent:-${DEVWS_DEFAULT_AGENT:-claude}}"

  # The project files are the kind's: CLAUDE.md or AGENTS.md.
  kind="$(agent_kind "$agent")" || kind="$(agent_kind "${DEVWS_DEFAULT_AGENT:-claude}")" || kind=claude
  if message="$("$HOME/.tmuxinator/init_agent.sh" "$root" "$kind" 2>&1)"; then
    tmux display-message "$message"
  else
    tmux display-message "Initialize agent failed: $message"
  fi
}

# The folders of a kind's accounts (~/.claude, ~/.claude-work, …) that no
# agent uses yet, written under ~.
unregistered_folders() {
  local kind="$1" folder agent used=" "
  for agent in ${DEVWS_AGENTS:-claude codex}; do
    used+="$(agent_home "$agent" 2>/dev/null) "
  done
  for folder in "$HOME/.$kind" "$HOME/.$kind"-*; do
    [ -d "$folder" ] || continue
    case "$used" in *" $folder "*) continue ;; esac
    printf '~/%s\n' "${folder#"$HOME"/}"
  done
}

# Asks for the kind, the account's folder (offered, or typed) and a name, then
# writes them to .env (register_agent.sh).
register_agent() {
  local kind folder name suggested
  kind="$(printf '%s\n' claude codex |
    fzf --height=~10 --layout=reverse --prompt='Kind > ' --header='Register an agent: which program it runs')" || return
  [ -n "$kind" ] || return

  folder="$(unregistered_folders "$kind" |
    fzf --height=~12 --layout=reverse --print-query --prompt='Folder > ' \
      --header="Its config folder (pick one, or type a path)" | tail -n 1)"
  [ -n "$folder" ] || return

  # ~/.claude-work suggests "work"; any other folder its own name.
  suggested="${folder##*/}"
  suggested="${suggested#.}"
  suggested="${suggested#"$kind"-}"
  suggested="$(printf '%s' "$suggested" | tr -c 'A-Za-z0-9_' _)"
  # Asked again until it can be registered: it becomes a variable name in .env.
  trap 'trap - INT; printf "\n"; return 130' INT
  while :; do
    read -e -r -p "Name [$suggested] (Ctrl-C to cancel): " name || { trap - INT; return; }
    name="${name:-$suggested}"
    if ! agent_valid_name "$name"; then
      printf 'A name uses only letters, digits and _ (such as %s).\n' "$suggested"
    elif [[ " ${DEVWS_AGENTS:-claude codex} " == *" $name "* ]]; then
      printf '%s is already registered.\n' "$name"
    else
      break
    fi
  done
  trap - INT

  "$HOME/.tmuxinator/register_agent.sh" add "$name" "$kind" "$folder"
  read -r -p 'Press Enter to return…'
}

remove_agent() {
  local agent
  agent="$(printf '%s\n' ${DEVWS_AGENTS:-claude codex} |
    fzf --height=~10 --layout=reverse --prompt='Remove > ' \
      --header='Remove an agent from devws (its folder is kept)')" || return
  [ -n "$agent" ] || return
  "$HOME/.tmuxinator/register_agent.sh" remove "$agent"
  read -r -p 'Press Enter to return…'
}

# Another agent, terminal or Neovim beside the terminal (new_panel.sh).
new_panel() {
  local session kind agent=""
  session="$(tmux display-message -p '#S')"
  kind="$(
    printf '%s\n' Agent Terminal Neovim |
      fzf --height=~10 --layout=reverse --prompt='New panel > ' \
        --header='Opens beside the terminal, with the agent group'
  )" || return 0
  case "$kind" in
    Agent)
      agent="$(choose_agent 'Choose the agent for the new panel')" || return 0
      [ -n "$agent" ] || return 0
      "$HOME/.tmuxinator/new_panel.sh" "$session" agent "$agent" >/dev/null
      ;;
    Terminal) "$HOME/.tmuxinator/new_panel.sh" "$session" terminal >/dev/null ;;
    Neovim) "$HOME/.tmuxinator/new_panel.sh" "$session" nvim >/dev/null ;;
    # Cancelled: nothing to open.
    *) return 0 ;;
  esac
}

toggle_layout() {
  "$HOME/.tmuxinator/toggle_layout.sh" "$(tmux display-message -p '#S')"
}

jump_agent() {
  "$HOME/.tmuxinator/jump_agent.sh" "$(tmux display-message -p '#{client_name}')"
}

minimize_agents() {
  "$HOME/.tmuxinator/minimize_panes.sh" "$(tmux display-message -p '#S')" toggle agents
}

toggle_header() {
  "$HOME/.tmuxinator/header_toggle.sh" toggle
}

choose_follow_mode() {
  "$HOME/.tmuxinator/follow_mode.sh" "$(tmux display-message -p '#S')"
}

open_database() {
  "$HOME/.tmuxinator/open_database.sh" "$(tmux display-message -p '#S')"
}

# Picks the folder whose .env the database pane connects to, for projects
# that keep their app in a subfolder or have several.
choose_database_folder() {
  local session root choices choice folder
  session="$(tmux display-message -p '#S')"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"
  [ -d "$root" ] || return
  . "$HOME/.tmuxinator/database_url.sh"

  choices="$(database_folder_choices "$root")"
  choice="$(
    { printf 'Automatic\tfind the database settings by itself\n'
      printf '%s\n' "$choices"; } |
      fzf --height=100% --layout=reverse --prompt='Database folder > ' \
        --delimiter=$'\t' --tabstop=24 \
        --header=".env folders in $(basename "$root")" \
        --info=inline --no-separator --border=none --margin=0 --padding=0
  )" || return
  folder="${choice%%$'\t'*}"
  [ "$folder" = Automatic ] && folder=""
  remember_database_folder "$root" "$folder" || return
  "$HOME/.tmuxinator/open_database.sh" "$session" --reconnect
  tmux display-message "Database folder: ${folder:-automatic}"
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
  local entered_path project_root agent editor

  printf '\033c'
  printf 'New workspace\n\n'
  # Keep Ctrl-C local to this prompt so cancelling workspace creation returns
  # to the picker instead of terminating the sidebar.
  trap 'trap - INT; printf "\n"; return 130' INT
  read -e -r -p 'Folder path (Ctrl-C to cancel): ' entered_path
  trap - INT
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

  editor="$(choose_editor)" || return
  [ -n "$editor" ] || return

  tmuxinator start dev project_root="$project_root" agent="$agent" editor="$editor"
  save_state
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
    "tmux kill-session -t $quoted_target; '$HOME/.tmuxinator/save_state.sh'; '$HOME/.tmuxinator/refresh_devws_pickers.sh'"
}
