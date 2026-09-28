#!/usr/bin/env bash
# Persistent devws sidebar: workspace switcher plus workspace commands.

set -u

config="$HOME/.tmuxinator/.env"
[ -r "$config" ] && . "$config"

if ! command -v fzf >/dev/null 2>&1; then
  printf 'devws picker: fzf is required; run the devws installer and restart tmux.\n' >&2
  exit 127
fi

# Always title this script's own pane. Without an explicit target tmux titles
# the client's active pane, which may be the editor or terminal.
tmux select-pane -t "$TMUX_PANE" -T devws-picker 2>/dev/null

dev_sessions() {
  tmux list-sessions -F '#{session_name}' 2>/dev/null |
    awk '/^dev-/'
}

print_padded_row() {
  local style="$1"
  local width="$2"
  local text="$3"
  local padding=$((width - ${#text}))

  (( padding < 0 )) && padding=0
  printf '%b%s%*s\033[0m' "$style" "$text" "$padding" ""
}

workspace_rows() {
  local current="$1"
  local session agent editor root branch state state_plain row_width

  row_width="$(
    tmux display-message -p -t "$TMUX_PANE" '#{pane_width}' 2>/dev/null
  )"
  case "$row_width" in
    ''|*[!0-9]*) row_width=80 ;;
  esac
  (( row_width > 2 )) && row_width=$((row_width - 2))

  while IFS='|' read -r session agent editor root; do
    case "$session" in dev-*) ;; *) continue ;; esac
    agent="${agent:-unknown}"
    editor="${editor:-unknown}"

    branch="—"
    state="  "
    state_plain="  "
    if [ -n "$root" ] && git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
      branch="$(git -C "$root" branch --show-current 2>/dev/null)"
      [ -n "$branch" ] || branch="detached"
      if [ -n "$(git -C "$root" status --porcelain 2>/dev/null)" ]; then
        state="\033[33m●\033[0m "
        state_plain="● "
      else
        state="\033[32m✓\033[0m "
        state_plain="✓ "
      fi
    fi

    if [ "$session" = "$current" ]; then
      printf 'workspace:%s\t' "$session"
      print_padded_row '\033[1;37;48;5;238m' \
        "$row_width" "$state_plain  $session"
      printf '\n'
      print_padded_row '\033[2;37;48;5;238m' "$row_width" "    $branch"
      printf '\n'
      print_padded_row '\033[2;37;48;5;238m' \
        "$row_width" "    $agent · $editor"
      printf '\0'
    else
      printf 'workspace:%s\t%b  %s\033[0m' "$session" "$state" "$session"
      printf '\n    \033[2m%s\033[0m' "$branch"
      printf '\n    \033[2m%s · %s\033[0m\0' "$agent" "$editor"
    fi
  done < <(tmux list-sessions \
    -F '#{session_name}|#{@devws_agent}|#{@devws_editor}|#{@devws_root}' 2>/dev/null)
}

refresh_pickers() {
  "$HOME/.tmuxinator/refresh_devws_pickers.sh"
}

save_state() {
  "$HOME/.tmuxinator/save_state.sh"
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

choose_editor() {
  local editors="${DEVWS_EDITORS:-lvim nvim vim code zed}"
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
      lvim|nvim|vim|code|zed)
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
  pane="$(agent_pane "$session")" || return
  agent="$(tmux show-options -t "$session" -v @devws_agent 2>/dev/null)"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"
  [ -n "$agent" ] || agent="${DEVWS_DEFAULT_AGENT:-claude}"

  tmux respawn-pane -k -t "$pane" -c "${root:-$PWD}" \
    "$HOME/.tmuxinator/agent_runner.sh $agent"
}

initialize_agent() {
  local session agent root message
  session="$(tmux display-message -p '#S')"
  agent="$(tmux show-options -t "$session" -v @devws_agent 2>/dev/null)"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"
  agent="${agent:-${DEVWS_DEFAULT_AGENT:-claude}}"

  case "$agent" in
    claude|codex) ;;
    *) agent="${DEVWS_DEFAULT_AGENT:-claude}" ;;
  esac
  if message="$("$HOME/.tmuxinator/init_agent.sh" "$root" "$agent" 2>&1)"; then
    tmux display-message "$message"
  else
    tmux display-message "Initialize agent failed: $message"
  fi
}

terminal_pane() {
  local session="$1"

  # @devws_role is authoritative; the title only survives in workspaces created
  # before the terminal pane started marking itself.
  tmux list-panes -t "$session" \
    -F '#{pane_id}|#{@devws_role}|#{pane_title}' 2>/dev/null |
    awk -F '|' '
      $2 == "terminal" { print $1; exit }
      $3 == "terminal" { fallback = $1 }
      END { if (fallback != "") print fallback }'
}

new_terminal() {
  local session root pane added
  session="$(tmux display-message -p '#S')"
  root="$(tmux show-options -t "$session" -v @devws_root 2>/dev/null)"
  pane="$(terminal_pane "$session")"
  [ -n "$pane" ] || return
  added="$(
    tmux split-window -h -t "$pane" -c "${root:-$PWD}" -P -F '#{pane_id}'
  )" || return
  tmux set-option -p -t "$added" @devws_role extra 2>/dev/null
  save_state
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

while tmux list-sessions >/dev/null 2>&1; do
  current="$(tmux display-message -p '#S' 2>/dev/null)"
  current_position="$(
    tmux list-sessions -F '#{session_name}' 2>/dev/null |
      awk -v current="$current" '
        /^dev-/ { position++ }
        $0 == current { print position; exit }
      '
  )"
  [ -n "$current_position" ] || current_position=1
  selected="$(
    workspace_rows "$current" |
      fzf --height=100% --layout=reverse --ansi --read0 --gap=1 \
        --highlight-line \
        --color='bg+:244,fg+:255' \
        --delimiter=$'\t' --with-nth=2.. \
        --bind="load:pos($current_position)" \
        --bind='click-header:transform:
          case "$FZF_CLICK_HEADER_LINE" in
            2) printf "print(new)+accept\n" ;;
            3) printf "print(switch-agent)+accept\n" ;;
            4) printf "print(switch-editor)+accept\n" ;;
            5) printf "print(initialize-agent)+accept\n" ;;
            6) printf "print(restart-agent)+accept\n" ;;
            7) printf "print(new-terminal)+accept\n" ;;
            8) printf "print(open-folder)+accept\n" ;;
            9) printf "print(close)+accept\n" ;;
          esac' \
        --header=$'WORKSPACE COMMANDS\n\033[32m+ New workspace\033[0m\n\033[36m⇄ Switch agent\033[0m\n\033[36m⇄ Switch editor\033[0m\n\033[35m◇ Initialize agent\033[0m\n\033[36m↻ Restart agent\033[0m\n\033[36m▣ New terminal\033[0m\n\033[36m⌂ Open folder\033[0m\n\033[31m× Close workspace\033[0m\n\n\033[2mOPEN WORKSPACES\033[0m' \
        --prompt='> ' \
        --info=inline --no-separator --border=none --margin=0 --padding=0
  )"
  # Header actions add their command before fzf's current workspace output.
  selected="${selected%%$'\n'*}"

  case "${selected%%$'\t'*}" in
    new) new_workspace ;;
    switch-agent) switch_agent ;;
    switch-editor) switch_editor ;;
    initialize-agent) initialize_agent ;;
    restart-agent) restart_agent ;;
    new-terminal) new_terminal ;;
    open-folder) open_folder ;;
    close) close_workspace ;;
    '') ;;
    workspace:*)
      workspace="${selected%%$'\t'*}"
      tmux switch-client -t "${workspace#workspace:}" 2>/dev/null
      ;;
  esac
done
