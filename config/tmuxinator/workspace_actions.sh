#!/usr/bin/env bash
# The devws workspace actions, shared by the sidebar (session_picker.sh) and
# the tabbed menu (devws_menu.sh). Source this file; each action works on the
# client's current workspace and asks what it needs with fzf or a prompt.

# A tmux server started before installation may still have its old PATH.
PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

# Agents and editors to offer, and the defaults.
config="$HOME/.tmuxinator/.env"
[ -r "$config" ] && . "$config"
# shellcheck source=agents.sh
. "$HOME/.tmuxinator/agents.sh"
# shellcheck source=workspace_panes.sh
. "$HOME/.tmuxinator/workspace_panes.sh"

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

# Every question from a list, in one look (picker.sh):
#   pick <prompt> <header> [fzf options...]   the list on stdin
pick() {
  "$HOME/.tmuxinator/picker.sh" "$@"
}

# [header]: what the agent is for.
choose_agent() {
  local agents="${DEVWS_AGENTS:-claude codex}"
  local header="${1:-The agent for the new workspace}"
  local agent

  agent="$(printf '%s\n' $agents | pick Agent "$header")"
  [ -n "$agent" ] && printf '%s\n' "$agent"
}

choose_editor() {
  local editors="${DEVWS_EDITORS:-nvim vim code zed}"
  local editor

  editor="$(printf '%s\n' $editors | pick Editor 'The editor for the workspace')"
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
  done < <(tmux list-panes -t "$(main_window "$session")" \
    -F '#{pane_id}|#{@devws_role}|#{pane_current_command}' 2>/dev/null)

  # Compatibility for sessions created before agent_runner.sh was introduced.
  while IFS='|' read -r pane role command; do
    [ -z "$role" ] || continue
    case "$command" in
      claude|codex|node)
        printf '%s\n' "$pane"
        return 0
        ;;
    esac
  done < <(tmux list-panes -t "$(main_window "$session")" \
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
  done < <(tmux list-panes -t "$(main_window "$session")" \
    -F '#{pane_id}|#{@devws_role}|#{pane_current_command}' 2>/dev/null)

  # Compatibility for workspaces created before editor_runner.sh existed.
  while IFS='|' read -r pane role command; do
    case "$command" in
      nvim|vim|code|zed)
        printf '%s\n' "$pane"
        return 0
        ;;
    esac
  done < <(tmux list-panes -t "$(main_window "$session")" \
    -F '#{pane_id}|#{@devws_role}|#{pane_current_command}' 2>/dev/null)
  return 1
}

# Agent and terminal actions need the real panes, which live in a hidden
# window while minimized, so they are expanded first.
expand_minimized_panes() {
  "$HOME/.tmuxinator/minimize_panes.sh" "$1" restore
}

start_workspace_agent() {
  local session="$1" agent="$2" pane root command target
  pane="$(agent_pane "$session")" || pane=""
  root="$(tmux show-options -qv -t "=$session:" @devws_root)"
  command="$(printf '%q %q' "$HOME/.tmuxinator/agent_runner.sh" "$agent")"
  if [ -n "$pane" ]; then
    tmux respawn-pane -k -t "$pane" -c "${root:-$PWD}" "$command" || return
  else
    load_workspace_panes "$session"
    target="$(pane_with_role terminal)"
    [ -n "$target" ] || target="$(pane_with_role editor)"
    [ -n "$target" ] || target="$(printf '%s\n' "$workspace_panes" |
      awk -F '\t' '$3 != "devws-picker" { print $1; exit }')"
    [ -n "$target" ] || target="$(main_window "$session")"
    pane="$(tmux split-window -v -b -t "$target" -c "${root:-$PWD}" -P -F '#{pane_id}' "$command")" || return
    tmux set-option -p -t "$pane" @devws_role agent
  fi
  "$HOME/.tmuxinator/agent_status.sh" "$session" idle
}

switch_agent() {
  local session agent root
  session="$(tmux display-message -p '#S')"
  expand_minimized_panes "$session" || return
  agent="$(choose_agent)" || return
  [ -n "$agent" ] || return
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"

  start_workspace_agent "$session" "$agent" || return
  tmux set-option -t "$session" @devws_agent "$agent"
  [ -z "$root" ] || remember_opening "$root" "$agent" \
    "$(tmux show-options -t "$session" -v @devws_editor 2>/dev/null)"
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
  [ -z "$root" ] || remember_opening "$root" \
    "$(tmux show-options -t "$session" -v @devws_agent 2>/dev/null)" "$editor"
  tmux respawn-pane -k -t "$pane" -c "${root:-$PWD}" \
    "$HOME/.tmuxinator/editor_runner.sh $editor"
  save_state
  refresh_pickers
}

restart_agent() {
  local session agent
  session="$(tmux display-message -p '#S')"
  expand_minimized_panes "$session" || return
  agent="$(tmux show-options -t "$session" -v @devws_agent 2>/dev/null)"
  [ -n "$agent" ] || agent="${DEVWS_DEFAULT_AGENT:-claude}"

  start_workspace_agent "$session" "$agent" || return
  save_state
  refresh_pickers
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
  kind="$(printf '%s\n' claude codex | pick Kind 'Register an agent: which program it runs')" || return
  [ -n "$kind" ] || return

  folder="$(unregistered_folders "$kind" |
    pick Folder 'Its config folder (pick one, or type a path)' --print-query | tail -n 1)"
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

# Asks for a registered agent, then one of its kind's folders (or a typed
# path), and writes it to .env: the agent runs with that folder set from its
# next start (register_agent.sh folder).
change_agent_folder() {
  local agent kind folder candidate
  agent="$(printf '%s\n' ${DEVWS_AGENTS:-claude codex} |
    pick Agent 'Change the config folder of an agent')" || return
  [ -n "$agent" ] || return
  kind="$(agent_kind "$agent")" || return

  folder="$(for candidate in "$HOME/.$kind" "$HOME/.$kind"-*; do
      [ -d "$candidate" ] && printf '~/%s\n' "${candidate#"$HOME"/}"
    done | pick Folder "$agent runs in $(agent_home "$agent") (pick one, or type a path)" --print-query |
    tail -n 1)"
  [ -n "$folder" ] || return

  "$HOME/.tmuxinator/register_agent.sh" folder "$agent" "$folder"
  read -r -p 'Press Enter to return…'
}

remove_agent() {
  local agent
  agent="$(printf '%s\n' ${DEVWS_AGENTS:-claude codex} |
    pick Remove 'Remove an agent from devws (its folder is kept)')" || return
  [ -n "$agent" ] || return
  "$HOME/.tmuxinator/register_agent.sh" remove "$agent"
  read -r -p 'Press Enter to return…'
}

# ai-memory (memory.sh): the agents' long-term memory. Search asks the
# workspace's agent, which searches through ai-memory's MCP tool.
memory() {
  "$HOME/.tmuxinator/memory.sh" "$@"
}

search_memory() {
  local question
  read -e -r -p 'Search memory for: ' question || return
  [ -n "$question" ] || return
  memory ask "$(tmux display-message -p '#S')" "$question" || pause
}

recent_memory() {
  memory recent "$(tmux show-options -t "$(tmux display-message -p '#S')" -v @devws_root 2>/dev/null)"
  pause
}

open_memory_web() {
  memory web || pause
}

memory_doctor() {
  memory doctor
  pause
}

# Another agent, terminal or Neovim, right of, left of, below or above the
# focused pane (new_panel.sh).
new_panel() {
  local session kind program agent place position
  session="$(tmux display-message -p '#S')"
  # Esc on a later question goes back to the one before.
  while :; do
    kind="$(printf '%s\n' Agent Terminal Neovim | pick 'New panel' 'What to open in the new panel')" || return 0
    agent=""
    case "$kind" in
      Agent)
        program=agent
        agent="$(choose_agent 'The agent for the new panel')" || continue
        [ -n "$agent" ] || continue
        ;;
      Terminal) program=terminal ;;
      Neovim) program=nvim ;;
      *) return 0 ;;
    esac
    place="$(printf '%s\n' Right Left Below Above |
      pick Where 'Where to open it, next to the pane you are in')" || continue
    case "$place" in
      Right) position=right ;;
      Left) position=left ;;
      Below) position=below ;;
      Above) position=above ;;
      *) continue ;;
    esac
    break
  done
  "$HOME/.tmuxinator/new_panel.sh" "$session" "$position" "$program" ${agent:+"$agent"} >/dev/null
}

# The focused panel trades places with its neighbour on a side (move_panel.sh).
move_panel() {
  local place direction
  place="$(printf '%s\n' Right Left Below Above | pick Move 'Move the panel you are in')" || return 0
  case "$place" in
    Right) direction=right ;;
    Left) direction=left ;;
    Below) direction=below ;;
    Above) direction=above ;;
    *) return 0 ;;
  esac
  "$HOME/.tmuxinator/move_panel.sh" "$(tmux display-message -p '#S')" "$direction"
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

# Ghostty's look from devws (config/ghostty): linked, so it is applied once
# per machine and follows the repo after that. What it did stays on screen.
apply_ghostty_theme() {
  printf '\033c'
  "$HOME/.tmuxinator/ghostty_theme.sh" apply
  pause
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
      pick 'Database folder' ".env folders in $(basename "$root")" \
        --height=100% --delimiter=$'\t' --tabstop=24
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

  choice="$(printf '%s\n' 'Finder' 'Zed' 'VS Code' | pick 'Open with' "Open $(basename "$root") in")"
  case "$choice" in
    Finder) open "$root" >/dev/null 2>&1 & ;;
    Zed) command -v zed >/dev/null && zed "$root" >/dev/null 2>&1 & ;;
    'VS Code') command -v code >/dev/null && code "$root" >/dev/null 2>&1 & ;;
  esac
}

# Waits for Enter. With nothing to read (no terminal, or it closed) there is no
# one to wait for: that is not the action failing.
pause() {
  read -r -p 'Press Enter to return…' || :
}

# The folder opened, with its agent and editor: `devws` with no folder lists
# it to reopen with them (recent.sh).
remember_opening() {
  "$HOME/.tmuxinator/recent.sh" add "$1" "$2" "$3" 2>/dev/null
}

# Opens a workspace on <folder>, asking for the agent and editor unless given.
#   start_workspace_at <folder> [<agent> <editor>]
start_workspace_at() {
  local folder="$1" agent="${2:-}" editor="${3:-}"
  if [ -z "$agent" ] || [ -z "$editor" ]; then
    agent="$(choose_agent)" || return
    [ -n "$agent" ] || return
    editor="$(choose_editor)" || return
    [ -n "$editor" ] || return
  fi

  remember_opening "$folder" "$agent" "$editor"
  tmuxinator start dev project_root="$folder" agent="$agent" editor="$editor"
  save_state
  refresh_pickers
}

new_workspace() {
  local entered_path project_root

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

  start_workspace_at "$project_root"
}

# The start screen (github_workspace.sh): reopens a folder with the agent and
# editor it had, or opens a new branch of a GitHub repo, asking for them.
new_github_workspace() {
  local folder status
  printf '\033c'
  folder="$("$HOME/.tmuxinator/github_workspace.sh")"
  status=$?
  # 1 is a cancelled question; anything else failed and says why.
  if [ "$status" -ne 0 ]; then
    [ "$status" -eq 1 ] || pause
    return
  fi
  # shellcheck disable=SC2046
  start_workspace_at "$folder" $("$HOME/.tmuxinator/recent.sh" get "$folder")
}

# An agent team (devws agentic): the start screen asks for the repo and the
# branch, devAgentics makes the task, and the workspace opens on it.
open_agentic_team() {
  printf '\033c'
  "$HOME/.tmuxinator/agentic.sh" || pause
}

# Resolve group workspaces through their team before looking up panes or
# the task. Never start a board here: it would start another dispatcher.
team_session() {
  local current parent
  current="$(tmux display-message -p ${TMUX_PANE:+-t "$TMUX_PANE"} '#S')"
  parent="$(tmux show-options -qv -t "=$current:" @devws_team_of 2>/dev/null)"
  printf '%s\n' "${parent:-$current}"
}

focus_team_role() {
  local role="$1" view="${2:-}" owner pane client
  owner="$(team_session)"
  pane="$(tmux list-panes -s -t "=$owner" -F '#{pane_id}|#{@devws_role}' 2>/dev/null |
    awk -F '|' -v role="$role" '$2 == role { print $1; exit }')"
  if [ -z "$pane" ]; then
    printf 'The team %s is not open.\n' "$([ "$role" = team-log ] && printf board || printf leader)" >&2
    return 1
  fi
  if [ -n "$view" ]; then
    tmux set-option -p -t "$pane" @devagentics_board_view "$view" || return
  fi
  tmux select-window -t "$pane" || return
  tmux select-pane -t "$pane" || return
  client="$(tmux display-message -p '#{client_name}')"
  [ -z "$client" ] || tmux switch-client -c "$client" -t "$pane" || return
}

open_team_view() {
  case "${1:-}" in
    overview|agents|steps|activity|usage) focus_team_role team-log "$1" ;;
    *) printf 'Unknown team view: %s\n' "${1:-}" >&2; return 2 ;;
  esac
}

open_team_leader() {
  focus_team_role agent
}

# Moves the agent team's leader to its other program (Claude or Codex):
# devAgentics makes the move once the leader's turn is over.
switch_leader_model() {
  local task
  task="$(tmux show-options -qv -t "=$(team_session):" @devws_task 2>/dev/null)"
  printf '\033c'
  if [ -z "$task" ]; then
    printf 'This workspace is not an agent team.\n'
  else
    "${DEVWS_AGENTIC_COMMAND:-devagentics}" leader switch "$task"
  fi
  pause
}

# Changes an agent team role's model, effort or approvals (devagentics role
# set): the role, the setting, then a value its program takes, Default for
# the program's own, or, for a model, one typed. The workspace's repo roles
# count. The leader is left out: Switch leader model moves it.
team_roles() {
  local agentic="${DEVWS_AGENTIC_COMMAND:-devagentics}" root listing role setting value
  local -a repo=()
  root="$(tmux show-options -t "$(tmux display-message -p '#S')" -v @devws_root 2>/dev/null)"
  [ -z "$root" ] || repo=(--repo "$root")
  printf '\033c'
  if ! listing="$("$agentic" role list "${repo[@]}" 2>&1)"; then
    printf '%s\n' "$listing"
    pause
    return
  fi
  role="$(awk '$1 != "leader"' <<<"$listing" |
    pick Role 'The role to change: name, group, program, model, effort, approvals, source')"
  role="${role%% *}"
  [ -n "$role" ] || return
  setting="$(printf '%s\n' model effort approvals | pick Setting "What to change in $role")"
  [ -n "$setting" ] || return
  value="$(
    {
      "$agentic" role choices "$role" "$setting" "${repo[@]}"
      printf '%s\n' Default
      [ "$setting" != model ] || printf '%s\n' 'Other…'
    } | pick Value "The $setting for $role (Default: its program's own)"
  )"
  case "$value" in
    '') return ;;
    Default) value="" ;;
    'Other…')
      read -e -r -p 'Model: ' value
      [ -n "$value" ] || return
      ;;
  esac
  "$agentic" role set "$role" "--$setting" "$value" "${repo[@]}"
  pause
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
    } | awk '!seen[$0]++' | pick Branch 'Branch for the new worktree'
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
  remember_opening "$folder" "$agent" "$editor"
  tmuxinator start dev project_root="$folder" agent="$agent" editor="$editor"
  save_state
  refresh_pickers
}

# Long work runs as a background job (job.sh): the menu closes at once and the
# header shows it, where the network and weather are, until it is done.
#   start_job <title> [--lock <name>]... -- <command> [args]
start_job() {
  "$HOME/.tmuxinator/job.sh" start "$@" >/dev/null || pause
}

# The workspace's changes, committed by its agent or by hand (git_actions.sh):
# the popup only stays to show a failure. Push asks about the PR, then runs
# as a job.
commit_with_agent() {
  "$HOME/.tmuxinator/git_actions.sh" commit || pause
}

commit_by_hand() {
  "$HOME/.tmuxinator/git_actions.sh" commit --manual || pause
}

push_branch() {
  local root branch pr
  root="$(workspace_root)"
  pr="$("$HOME/.tmuxinator/git_actions.sh" ask-pr)" || return
  branch="$(git -C "$root" branch --show-current 2>/dev/null)"
  start_job "Push ${branch:-$(basename "$root")}" -- \
    env DEVWS_ROOT="$root" "$HOME/.tmuxinator/git_actions.sh" push --pr "$pr"
}

login_github() {
  local status=0
  if ! command -v gh >/dev/null 2>&1; then
    printf 'GitHub CLI is missing; run ./install.sh in the devws checkout.\n' >&2
    pause || true
    return 1
  fi
  # Run in the popup's terminal so device/browser login also works over SSH.
  gh auth login || status=$?
  pause || true
  return "$status"
}

# The repo's apps in jungleforge, run on one of its branches (jungleforge.sh):
# the same containers, ports and databases on another folder.
jungleforge() {
  "$HOME/.tmuxinator/jungleforge.sh" "$@"
}

workspace_root() {
  tmux show-options -t "$(tmux display-message -p '#S')" -v @devws_root 2>/dev/null
}

setup_jungleforge() {
  printf '\033c'
  jungleforge setup
  pause
}

# The repo's apps. A repo jungleforge does not know is offered registering
# first; anything else wrong is explained.
known_apps() {
  local root="$1" rows error answer
  rows="$(jungleforge apps "$root" 2>/dev/null)" && { printf '%s\n' "$rows"; return 0; }
  error="$(jungleforge apps "$root" 2>&1 >/dev/null)"
  printf '%s\n' "$error" >&2
  case "$error" in
    *'Not a jungleforge project'*)
      printf 'Register it in jungleforge now? [Y/n] ' >&2
      read -r answer || return 1
      case "$answer" in n | N | no) return 1 ;; esac
      register_repo "$root" >&2 || { pause >&2; return 1; }
      rows="$(jungleforge apps "$root")" || { pause >&2; return 1; }
      printf '%s\n' "$rows"
      ;;
    *) pause >&2; return 1 ;;
  esac
}

register_in_jungleforge() {
  printf '\033c'
  register_repo "$(workspace_root)"
  pause
}

# The checkout jungleforge's registry points at, its "main": for a devws
# GitHub repo (a bare clone, github_repo.sh) the default branch's own folder,
# made when missing, since branch folders come and go; else the folder.
registry_checkout() {
  local root="$1" common
  common="$(git -C "$root" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" ||
    { printf 'Not a git repo: %s\n' "$root"; return 1; }
  if [ "$(basename "$common")" = .bare ]; then
    "$HOME/.tmuxinator/worktree.sh" ensure "$root" "$("$HOME/.tmuxinator/github_repo.sh" default-branch "$root")"
  else
    git -C "$root" rev-parse --show-toplevel
  fi
}

# "(repo root)" and the first-level folders an app may live in, those with a
# deploy/ marked: "<label><TAB><subfolder>".
app_folder_choices() {
  local root="$1" folder name
  printf '(repo root)%s\t.\n' "$([ -d "$root/deploy" ] && printf '  · deploy/')"
  for folder in "$root"/*/; do
    name="$(basename "$folder")"
    case "$name" in node_modules | vendor | deploy) continue ;; esac
    printf '%s%s\t%s\n' "$name" "$([ -d "$folder/deploy" ] && printf '  · deploy/')" "$name"
  done
}

# PhiPlanner → phi-planner, jungleforge's naming.
app_name_for() {
  printf '%s' "$1" | sed 's/\([a-z0-9]\)\([A-Z]\)/\1-\2/g' | tr 'A-Z_. ' 'a-z---' | tr -cd 'a-z0-9-'
}

# Registers each app of the workspace's repo it is asked for, scaffolding a
# deploy/ from a jungleforge template where the folder has none.
register_repo() {
  local root="$1" registry repo_name choice subfolder app_folder suggested app template answer port
  local count=0
  jungleforge available || { printf 'jungleforge is not set up: choose Set up jungleforge first.\n'; return 1; }
  registry="$(registry_checkout "$root")" || return 1
  repo_name="$(basename "$(dirname "$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)")")"
  [ "$(basename "$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)")" = .bare ] ||
    repo_name="$(basename "$registry")"
  while :; do
    choice="$(app_folder_choices "$root" | pick Folder "Where the app is, in $(basename "$root")" \
      --delimiter=$'\t' --with-nth=1)" || return 1
    [ -n "$choice" ] || return 1
    subfolder="${choice##*$'\t'}"
    [ "$subfolder" = . ] && subfolder=""
    app_folder="$root${subfolder:+/$subfolder}"
    suggested="$(app_name_for "$repo_name")"
    [ "$count" -gt 0 ] && [ -n "$subfolder" ] && suggested="$suggested-$(app_name_for "$subfolder")"
    read -r -p "App name [$suggested]: " app || return 1
    app="${app:-$suggested}"
    if [ ! -f "$app_folder/deploy/.docker-env" ]; then
      template="$(jungleforge templates | awk -F '\t' '{ printf "%s (%s)\t%s\n", $2, $1, $1 }' |
        pick Stack "No deploy/ in ${subfolder:-the repo root}: the template to start one from" \
          --delimiter=$'\t' --with-nth=1)" || return 1
      [ -n "$template" ] || return 1
      port="$(jungleforge scaffold "$app_folder" "${template##*$'\t'}" "$app")" || return 1
      share_docker_env "$root" "$registry" "${subfolder:+$subfolder/}deploy/.docker-env"
      printf 'Made %s/deploy from jungleforge'"'"'s %s template%s.\n' \
        "${subfolder:-.}" "${template##*$'\t'}" "${port:+, on port $port}"
    fi
    jungleforge register "$app" "$registry${subfolder:+/$subfolder}" || return 1
    printf 'Registered %s: %s\n' "$app" "$registry${subfolder:+/$subfolder}"
    count=$((count + 1))
    read -r -p 'Register another app of this repo? [y/N] ' answer || break
    case "$answer" in y | Y | yes) ;; *) break ;; esac
  done
  printf '\nLeft to you: commit the new deploy/ folders (main runs them once merged; Run this branch\n'
  printf 'works now), and point the app'"'"'s own .env at the shared services: DB_HOST=postgres,\n'
  printf 'REDIS_HOST=redis, RABBITMQ_HOST=rabbitmq.\n'
}

# A .docker-env git ignores goes to the repo's .shared, linked into this
# branch and the registry's checkout, like the other ignored files.
share_docker_env() {
  local root="$1" registry="$2" file="$3" repo_dir common
  git -C "$root" check-ignore -q "$file" 2>/dev/null || return 0
  common="$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)"
  [ "$(basename "$common")" = .bare ] || return 0
  repo_dir="$(dirname "$common")"
  "$HOME/.tmuxinator/shared_files.sh" seed "$repo_dir" "$root" "$file" &&
    rm "$root/$file" &&
    "$HOME/.tmuxinator/shared_files.sh" link "$repo_dir" "$root" &&
    "$HOME/.tmuxinator/shared_files.sh" link "$repo_dir" "$registry"
}

# The apps to move, one per line: all ticked, Tab unticks one; a repo with a
# single app is not asked. A repo jungleforge does not know is offered
# registering (choose_app too), and a cancelled question just fails.
choose_apps() {
  local rows apps
  rows="$(known_apps "$1")" || return 1
  apps="$(cut -f1 <<<"$rows")"
  if [ "$(grep -c . <<<"$apps")" -eq 1 ]; then
    printf '%s\n' "$apps"
    return 0
  fi
  pick Apps 'Apps to move to this branch' --multi --bind 'load:select-all' <<<"$apps"
}

# One app, for Migrate, Shell and Logs.
choose_app() {
  local rows apps
  rows="$(known_apps "$1")" || return 1
  apps="$(cut -f1 <<<"$rows")"
  if [ "$(grep -c . <<<"$apps")" -eq 1 ]; then
    printf '%s\n' "$apps"
    return 0
  fi
  pick App 'Which app' <<<"$apps"
}

# "--lock <app>" for each app: jobs on one app run one after another.
app_locks() {
  local app
  for app in "$@"; do
    printf -- '--lock\n%s\n' "$app"
  done
}

# A job running jungleforge.sh on the apps, titled "<apps> → <what>".
#   jungleforge_job <what> <jungleforge.sh arguments…> -- <app>...
jungleforge_job() {
  local what="$1" arguments=() apps=() locks=() line
  shift
  while [ "$#" -gt 0 ] && [ "$1" != -- ]; do
    arguments+=("$1")
    shift
  done
  shift
  apps=("$@")
  while IFS= read -r line; do
    locks+=("$line")
  done < <(app_locks "${apps[@]}")
  start_job "${apps[*]} → $what" "${locks[@]}" -- \
    "$HOME/.tmuxinator/jungleforge.sh" "${arguments[@]}" "${apps[@]}"
}

# The branch checked out in <folder>, or "main" for the registry's checkout.
branch_label() {
  git -C "$1" branch --show-current 2>/dev/null || basename "$1"
}

# Runs <folder>'s branch: the ticked apps start on it, as a job. Esc on the
# apps returns 130, for Switch to ask the branch again.
run_branch_at() {
  local folder="$1" apps
  printf '\033c'
  apps="$(choose_apps "$folder")" || return
  [ -n "$apps" ] || return 130
  # shellcheck disable=SC2086
  jungleforge_job "$(branch_label "$folder")" run "$folder" -- $apps
}

run_this_branch() {
  run_branch_at "$(workspace_root)"
}

switch_running_branch() {
  local root targets running label folder mark choice status
  root="$(workspace_root)"
  printf '\033c'
  known_apps "$root" >/dev/null || return
  targets="$(jungleforge targets "$root")"
  running="$(jungleforge running "$root" | cut -f2)"
  # Esc on the apps comes back here.
  while :; do
    choice="$(
      while IFS=$'\t' read -r label folder; do
        mark="  "
        grep -qxF "$folder" <<<"$running" && mark="▶ "
        [ "$label" = main ] && label="main ($folder)"
        printf '%s%s\t%s\n' "$mark" "$label" "$folder"
      done <<<"$targets" |
        pick Run 'Branch to run (▶ running now)' --delimiter=$'\t' --with-nth=1
    )" || return
    [ -n "$choice" ] || return
    run_branch_at "${choice##*$'\t'}"
    status=$?
    [ "$status" -eq 130 ] || return "$status"
  done
}

stop_running_apps() {
  local root rows
  root="$(workspace_root)"
  rows="$(known_apps "$root")" || return
  # shellcheck disable=SC2046
  jungleforge_job stop stop "$root" -- $(cut -f1 <<<"$rows")
}

# The background jobs' newest log that matters (job.sh): a failure first, in
# the workspace's log panel.
show_job_log() {
  "$HOME/.tmuxinator/job.sh" open || pause
}

list_network_devices() {
  local devws="${DEVWS_BIN:-$HOME/.local/bin/devws}" output status
  output="$(mktemp "${TMPDIR:-/tmp}/devws-network.XXXXXX")" || return
  # Let sudo own the terminal while discovery runs, then page the results.
  "$devws" network devices | tee "$output"
  status="${PIPESTATUS[0]}"
  if [ "$status" -eq 0 ] && [ -t 1 ] && command -v less >/dev/null 2>&1; then
    less -+F "$output"
  else
    pause || true
  fi
  rm -f "$output"
  return "$status"
}

# open_guide <page>: the guide's page in a popup large enough to read it.
# Opened from the tmux server once the menu's popup has closed: a client
# shows one popup at a time.
open_guide() {
  local client target=""
  client="$(tmux display-message -p '#{client_name}' 2>/dev/null)"
  [ -z "$client" ] || target="-c $(printf '%q' "$client")"
  tmux run-shell -b "sleep 0.2; tmux display-popup $target -E -w 90% -h 90% \
-T ' devws guide · / searches · q closes ' $(printf '%q' "$HOME/.tmuxinator/guide.sh $1")"
}

start_services() {
  start_job "Services" --lock services -- "$HOME/.tmuxinator/jungleforge.sh" services
}

migrate_running_app() {
  local root app
  root="$(workspace_root)"
  app="$(choose_app "$root")" || return
  [ -n "$app" ] || return
  start_job "$app → migrate" --lock "$app" -- "$HOME/.tmuxinator/jungleforge.sh" migrate "$root" "$app"
}

# Shell and Logs open in a panel below, which stays a terminal after Ctrl-C.
open_app_panel() {
  local command="$1" root app session pane
  root="$(workspace_root)"
  app="$(choose_app "$root")" || return
  [ -n "$app" ] || return
  session="$(tmux display-message -p '#S')"
  pane="$("$HOME/.tmuxinator/new_panel.sh" "$session" below terminal)" || return
  tmux send-keys -t "$pane" "$(printf '%q ' "$HOME/.tmuxinator/jungleforge.sh" "$command" "$root" "$app")" Enter
}

open_app_shell() {
  open_app_panel shell
}

open_app_logs() {
  open_app_panel logs
}

# jungleforge's own table of its containers, read-only, in the menu's popup.
show_app_info() {
  printf '\033c'
  "$HOME/.tmuxinator/jungleforge.sh" info
  pause
}

# A jungleforge setting (a port…) of one of the repo's apps, in this branch's
# deploy/.docker-env, or of the shared services: picked, then typed. A new
# port needs the container made again, so a restart is offered.
change_app_setting() {
  local root rows app subfolder choice whose target setting key current value error answer running
  local -a which=()
  root="$(workspace_root)"
  printf '\033c'
  rows="$(known_apps "$root")" || return
  # Esc on the settings comes back here.
  while :; do
    choice="$(
      {
        while IFS=$'\t' read -r app subfolder; do
          printf '%s · %sdeploy/.docker-env\t%s\n' "$app" "${subfolder:+$subfolder/}" "$app"
        done <<<"$rows"
        printf 'shared services · jungleforge .env\tshared\n'
      } | pick Settings 'Whose settings' --delimiter $'\t' --with-nth 1
    )" || return
    whose="${choice%%$'\t'*}"
    target="${choice##*$'\t'}"
    if [ "$target" = shared ]; then which=(shared); else which=("$root" "$target"); fi
    setting="$(jungleforge settings "${which[@]}" |
      awk -F '\t' '{ printf "%s = %s%s\t%s\n", $1, $2, ($3 == "" ? "" : "   # " $3), $1 }' |
      pick Setting "$whose" --delimiter $'\t' --with-nth 1)" && break
  done
  key="${setting##*$'\t'}"
  current="$(jungleforge settings "${which[@]}" | awk -F '\t' -v key="$key" '$1 == key { print $2 }')"
  while :; do
    printf '\n%s is %s. New value (Enter keeps it): ' "$key" "$current"
    read -r value || return
    [ -n "$value" ] || return 0
    error="$(jungleforge set "${which[@]}" "$key" "$value" 2>&1)" && break
    printf '%s\n' "${error#devws jungleforge: }"
  done
  printf '%s is now %s.\n' "$key" "$value"
  if [ "$target" = shared ]; then
    printf 'Restart the shared services now? [Y/n] '
    read -r answer
    case "$answer" in n | N | no) ;; *) start_services ;; esac
    return
  fi
  running="$(jungleforge running "$root" 2>/dev/null | awk -F '\t' -v app="$target" '$1 == app { print $2 }')"
  if [ "$running" = "$root" ]; then
    printf 'Restart %s now? [Y/n] ' "$target"
    read -r answer
    case "$answer" in n | N | no) ;; *) jungleforge_job restart run "$root" -- "$target" ;; esac
  elif [ -n "$running" ]; then
    printf '%s runs from %s: the change applies when this branch runs.\n' "$target" "$running"
    pause
  fi
}

# The repo's apps running from <root>, which is closing: asks to move them back
# to main (the default), stop them, or leave them running — not offered when
# the folder is going away. Prints the command for close_workspace to run: a
# background job, which also removes the folder (when <removing>) once the
# apps no longer run from it.
ask_about_running_apps() {
  local root="$1" removing="$2" apps main answer choices='[M]ain/[s]top/[l]eave' line
  local -a command=() job=()
  apps="$(jungleforge running "$root" 2>/dev/null | awk -F '\t' -v root="$root" '$2 == root { print $1 }' | paste -sd ' ' -)"
  [ -n "$apps" ] || return 0
  main="$(jungleforge targets "$root" 2>/dev/null | awk -F '\t' '$1 == "main" { print $2; exit }')"
  [ -n "$removing" ] && choices='[M]ain/[s]top'
  printf '\n%s run from this branch. Switch them back to main, stop them%s? %s ' \
    "$apps" "$([ -z "$removing" ] && printf ', or leave them running')" "$choices" >&2
  read -r answer
  case "$answer" in
    s | S | stop) answer=stop ;;
    l | L | leave) [ -n "$removing" ] && answer=main || answer=leave ;;
    *) answer=main ;;
  esac
  # shellcheck disable=SC2086
  case "$answer" in
    stop) command=("$HOME/.tmuxinator/jungleforge.sh" stop "$root" $apps) ;;
    main) command=("$HOME/.tmuxinator/jungleforge.sh" run "$main" $apps) ;;
    *) return 0 ;;
  esac
  # The folder goes once the apps run elsewhere ($0 is the folder, "$@" the
  # command).
  [ -z "$removing" ] ||
    command=(sh -c '"$@" && exec "$HOME/.tmuxinator/worktree.sh" remove "$0" '"${remove_untracked:-}" "$root" "${command[@]}")
  job=("$HOME/.tmuxinator/job.sh" start "$apps → $answer")
  # shellcheck disable=SC2086
  while IFS= read -r line; do
    job+=("$line")
  done < <(app_locks $apps)
  job+=(-- "${command[@]}")
  printf '%q ' "${job[@]}"
}

# Closing a workspace on a worktree of its own asks whether to remove the
# worktree's folder too, "keep" being the answer unless "y" is typed. One
# whose only changes are untracked files is offered for deletion with them
# (then remove_untracked, of close_workspace, is --untracked); one with a
# changed tracked file is always kept. The branch is never deleted.
ask_to_remove_worktree() {
  local root="$1" branch changes answer
  "$HOME/.tmuxinator/worktree.sh" linked "$root" || return 1
  branch="$("$HOME/.tmuxinator/worktree.sh" branch "$root" 2>/dev/null)"
  changes="$("$HOME/.tmuxinator/worktree.sh" status "$root" 2>/dev/null)" || return 1
  printf '\033c'
  if [ "$changes" != clean ]; then
    # The reason comes last, by the prompt: a long list would scroll it out
    # of a small popup.
    printf '%s\n' "$changes" | head -n 10
    [ "$(grep -c . <<<"$changes")" -le 10 ] || printf '… and %s more\n' "$(($(grep -c . <<<"$changes") - 10))"
    if grep -qv '^?? ' <<<"$changes"; then
      printf '\nKeeping the worktree %s and its branch: it has uncommitted changes (above).\n' "$root"
      printf 'Press Enter to close the workspace…'
      read -r
      return 1
    fi
    printf '\nThe worktree %s has only untracked files (above).\n' "$root"
    printf 'Delete the worktree %s with these untracked files? The branch %s is kept. [y/N] ' \
      "$root" "${branch:-(detached)}"
    read -r answer
    case "$answer" in
      y|Y|yes|Yes) remove_untracked=--untracked; return 0 ;;
      *) return 1 ;;
    esac
  fi
  printf 'The workspace is on the worktree %s.\n' "$root"
  printf 'Remove that folder too? The branch %s is kept. [y/N] ' "${branch:-(detached)}"
  read -r answer
  case "$answer" in
    y|Y|yes|Yes) return 0 ;;
    *) return 1 ;;
  esac
}

# The workspace to go to instead of <session>: the one used last, else any.
next_workspace() {
  # A workspace never attached has no time: it counts as 0.
  tmux list-sessions -F '#{session_last_attached}|#{session_name}' 2>/dev/null |
    awk -F '|' -v closing="$1" '$2 ~ /^dev-/ && $2 != closing { print ($1 == "" ? 0 : $1) "|" $2 }' |
    sort -t '|' -k1,1rn | awk -F '|' 'NR == 1 { print $2 }'
}

# The clients on <session> go to the next workspace first, so closing the one
# you are in moves you to another rather than out of devws; with no other
# workspace left they leave. Prints the commands, for close_workspace.
move_clients_off() {
  local session="$1" next client
  next="$(next_workspace "$session")"
  [ -n "$next" ] || return 0
  tmux list-clients -t "=$session:" -F '#{client_name}' 2>/dev/null | while IFS= read -r client; do
    printf 'tmux switch-client -c %q -t %q;' "$client" "=$next:"
  done
}

# Branch folders done with (PR merged or closed, no changes) go, as a job
# (tidy.sh); the popup stays only to say there is nothing to tidy.
tidy_up() {
  printf '\033c'
  "$HOME/.tmuxinator/tidy.sh" || pause
}

detach_client() {
  tmux detach-client
}

close_workspace() {
  local current selected target quoted_target root quoted_root remove_worktree="" move_apps="" move_clients
  # Set by ask_to_remove_worktree when the untracked files go too.
  local remove_untracked=""
  current="$(tmux display-message -p '#S' 2>/dev/null)"
  selected="$(
    dev_sessions |
      awk -v cur="$current" '{ print ($0 == cur ? "* " : "  ") $0 }' |
      pick Close 'Close a workspace (* the one you are in)' --height=100%
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
    remove_worktree="'$HOME/.tmuxinator/worktree.sh' remove $quoted_root $remove_untracked 2>/dev/null || tmux display-message 'Kept the worktree '$quoted_root': git would not remove it';"
  fi
  # Apps running from the folder move (or stop) first, as a job that then
  # removes the folder itself: it cannot go while they run from it.
  if [ -d "$root" ]; then
    move_apps="$(ask_about_running_apps "$root" "$remove_worktree")"
    if [ -n "$move_apps" ]; then
      move_apps="$move_apps >/dev/null;"
      remove_worktree=""
    fi
  fi
  move_clients="$(move_clients_off "$target")"
  # Run through the tmux server so closing the workspace containing this
  # sidebar does not terminate the refresh command along with the pane.
  tmux run-shell -b \
    "$move_clients '$HOME/.tmuxinator/save_state.sh' --closing $quoted_target; tmux kill-session -t $quoted_target; $move_apps $remove_worktree '$HOME/.tmuxinator/save_state.sh'; '$HOME/.tmuxinator/refresh_devws_pickers.sh'"
}
