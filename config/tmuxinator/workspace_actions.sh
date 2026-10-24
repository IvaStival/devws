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

# Another agent, terminal or Neovim, beside or below the focused pane
# (new_panel.sh).
new_panel() {
  local session kind program agent="" place position
  session="$(tmux display-message -p '#S')"
  kind="$(
    printf '%s\n' Agent Terminal Neovim |
      fzf --height=~10 --layout=reverse --prompt='New panel > ' \
        --header='What to open in the new panel'
  )" || return 0
  case "$kind" in
    Agent)
      program=agent
      agent="$(choose_agent 'Choose the agent for the new panel')" || return 0
      [ -n "$agent" ] || return 0
      ;;
    Terminal) program=terminal ;;
    Neovim) program=nvim ;;
    # Cancelled: nothing to open.
    *) return 0 ;;
  esac
  place="$(
    printf '%s\n' Beside Below |
      fzf --height=~10 --layout=reverse --prompt='Where > ' \
        --header='Beside or below the pane you are in'
  )" || return 0
  case "$place" in
    Beside) position=side ;;
    Below) position=below ;;
    *) return 0 ;;
  esac
  "$HOME/.tmuxinator/new_panel.sh" "$session" "$position" "$program" ${agent:+"$agent"} >/dev/null
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

# Opens a workspace on a git worktree of the current workspace's repo: an
# existing branch, or a new one off its HEAD (worktree.sh).
new_worktree_workspace() {
  local session root branch folder agent editor new_branch='+ New branch…'
  session="$(tmux display-message -p '#S')"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"

  printf '\033c'
  printf 'New worktree workspace\n\n'
  if ! git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    printf 'This workspace is not in a git repo: %s\nPress Enter to return…' "$root"
    read -r
    return
  fi

  branch="$(
    {
      printf '%s\n' "$new_branch"
      git -C "$root" for-each-ref --format='%(refname:short)' refs/heads
      git -C "$root" for-each-ref --format='%(refname:lstrip=3)' refs/remotes | grep -vx HEAD
    } | awk '!seen[$0]++' |
      fzf --height=~20 --layout=reverse --prompt='Branch > ' \
        --header='Branch for the new worktree (Esc: cancel)'
  )"
  [ -n "$branch" ] || return
  if [ "$branch" = "$new_branch" ]; then
    trap 'trap - INT; printf "\n"; return 130' INT
    while :; do
      read -e -r -p 'New branch name (Ctrl-C to cancel): ' branch
      [ -n "$branch" ] || { trap - INT; return; }
      git check-ref-format --branch "$branch" >/dev/null 2>&1 && break
      printf 'Not a valid branch name: %s\n' "$branch"
    done
    trap - INT
  fi

  agent="$(choose_agent)" || return
  [ -n "$agent" ] || return
  editor="$(choose_editor)" || return
  [ -n "$editor" ] || return

  if ! folder="$("$HOME/.tmuxinator/worktree.sh" ensure "$root" "$branch")"; then
    printf 'Press Enter to return…'
    read -r
    return
  fi
  tmuxinator start dev project_root="$folder" agent="$agent" editor="$editor"
  save_state
  refresh_pickers
}

# Closing a workspace on a worktree of its own asks whether to remove the
# worktree's folder too, "keep" being the answer unless "y" is typed. One with
# uncommitted changes is always kept, and the branch is never deleted.
ask_to_remove_worktree() {
  local root="$1" branch changes answer
  "$HOME/.tmuxinator/worktree.sh" linked "$root" || return 1
  branch="$("$HOME/.tmuxinator/worktree.sh" branch "$root" 2>/dev/null)"
  changes="$("$HOME/.tmuxinator/worktree.sh" status "$root" 2>/dev/null)" || return 1
  printf '\033c'
  if [ "$changes" != clean ]; then
    printf 'Keeping the worktree %s: it has uncommitted changes.\n\n' "$root"
    printf '%s\n' "$changes" | head -n 20
    printf '\nPress Enter to close the workspace…'
    read -r
    return 1
  fi
  printf 'The workspace is on the worktree %s.\n' "$root"
  printf 'Remove that folder too? The branch %s is kept. [y/N] ' "${branch:-(detached)}"
  read -r answer
  case "$answer" in
    y|Y|yes|Yes) return 0 ;;
    *) return 1 ;;
  esac
}

close_workspace() {
  local current selected target quoted_target root quoted_root remove_worktree=""
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
  # Asked before closing, while this pane is sure to be there to ask; the
  # folder goes once the workspace no longer runs in it, and git still refuses
  # if changes appeared meanwhile.
  root="$(tmux show-options -t "=$target:" -v @devws_root 2>/dev/null)"
  if [ -d "$root" ] && ask_to_remove_worktree "$root"; then
    printf -v quoted_root '%q' "$root"
    remove_worktree="'$HOME/.tmuxinator/worktree.sh' remove $quoted_root 2>/dev/null || tmux display-message 'Kept the worktree '$quoted_root': git would not remove it';"
  fi
  # Run through the tmux server so closing the workspace containing this
  # sidebar does not terminate the refresh command along with the pane.
  tmux run-shell -b \
    "tmux kill-session -t $quoted_target; $remove_worktree '$HOME/.tmuxinator/save_state.sh'; '$HOME/.tmuxinator/refresh_devws_pickers.sh'"
}
