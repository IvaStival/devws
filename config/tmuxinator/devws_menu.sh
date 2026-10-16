#!/usr/bin/env bash
# The devws menu: every workspace action, grouped in tabs, in a tmux popup
# (C-b Space, or "☰ Menu" in the sidebar). The actions themselves live in
# workspace_actions.sh, shared with the sidebar.
#
#   devws_menu.sh                   the menu (run inside the popup)
#   devws_menu.sh --popup [client]  opens the popup on a tmux client
#
# fzf calls these back while the menu is open; the tests use them too:
#   --items <tab>   the tab's actions: "<action><TAB><label and shortcut>"
#   --header        the tab bar, the current tab marked
#   --tab           the current tab
#   --switch <next|prev|tab title>   changes tab; prints fzf actions to show it
#   --switch-at <column>             changes to the tab clicked at that column
#   --run <action>  runs an action; --function <action> names its function

set -u

script="$HOME/.tmuxinator/devws_menu.sh"
tabs=(workspace agent editor database)

# tab|action|label|shortcut|function in workspace_actions.sh
actions='workspace|new|+ New workspace||new_workspace
workspace|open-folder|⌂ Open folder||open_folder
workspace|toggle-layout|▥ Toggle layout|C-b \|toggle_layout
workspace|minimize|▁ Minimize agent and terminal|C-b _|minimize_agents
workspace|header|▔ Hide or show the header|C-b C-s|toggle_header
workspace|new-terminal|▣ New terminal||new_terminal
workspace|close|× Close workspace||close_workspace
agent|switch-agent|⇄ Switch agent||switch_agent
agent|restart-agent|↻ Restart agent||restart_agent
agent|initialize-agent|◇ Initialize agent||initialize_agent
agent|register-agent|＋ Register agent||register_agent
agent|remove-agent|− Remove agent||remove_agent
agent|follow-mode|◎ Agent follow|C-b F|choose_follow_mode
editor|switch-editor|⇄ Switch editor||switch_editor
editor|follow-mode|◎ Agent follow|C-b F|choose_follow_mode
database|open-database|⛁ Database|C-b B|open_database
database|database-folder|⛁ Database folder||choose_database_folder'

session() {
  if [ -n "${TMUX_PANE:-}" ]; then
    tmux display-message -p -t "$TMUX_PANE" '#{session_name}'
  else
    tmux display-message -p '#{session_name}'
  fi
}

title() {
  printf '%s%s' "$(printf '%s' "${1:0:1}" | tr '[:lower:]' '[:upper:]')" "${1:1}"
}

current_tab() {
  local tab
  tab="$(tmux show-option -qv -t "$(session)" @devws_menu_tab)"
  case " ${tabs[*]} " in
    *" $tab "*) printf '%s' "$tab" ;;
    *) printf '%s' "${tabs[0]}" ;;
  esac
}

items() {
  local tab action label shortcut function
  while IFS='|' read -r tab action label shortcut function; do
    [ "$tab" = "$1" ] || continue
    printf '%s\t%-32s \033[2m%s\033[0m\n' "$action" "$label" "$shortcut"
  done <<<"$actions"
}

header() {
  local current tab
  current="$(current_tab)"
  for tab in "${tabs[@]}"; do
    if [ "$tab" = "$current" ]; then
      printf '\033[1;30;46m %s \033[0m ' "$(title "$tab")"
    else
      printf '\033[2m %s \033[0m ' "$(title "$tab")"
    fi
  done
  printf '\n'
}

# Moves to the next or previous tab (wrapping around) or to the tab clicked
# in the header; a click beside the tabs changes nothing.
switch_tab() {
  local current index count=${#tabs[@]} target="" tab
  current="$(current_tab)"
  for index in "${!tabs[@]}"; do
    [ "${tabs[$index]}" = "$current" ] && break
  done
  case "$1" in
    next) target="${tabs[$(((index + 1) % count))]}" ;;
    prev) target="${tabs[$(((index + count - 1) % count))]}" ;;
    *)
      for tab in "${tabs[@]}"; do
        [ "$(title "$tab")" = "$1" ] && target="$tab"
      done
      ;;
  esac
  [ -n "$target" ] || return 0
  tmux set-option -t "$(session)" @devws_menu_tab "$target"
  printf "reload('%s' --items %s)+transform-header('%s' --header)+first\n" "$script" "$target" "$script"
}

# The tab bar is " Title " per tab, one space apart; fzf reports a header
# click's column (its "word" would be the whole bar, split on tabs).
switch_at() {
  local column="$1" start=1 width tab
  case "$column" in ''|*[!0-9]*) return 0 ;; esac
  for tab in "${tabs[@]}"; do
    width=$((${#tab} + 2))
    if [ "$column" -ge "$start" ] && [ "$column" -lt $((start + width)) ]; then
      switch_tab "$(title "$tab")"
      return 0
    fi
    start=$((start + width + 1))
  done
}

function_for() {
  local tab action label shortcut function
  while IFS='|' read -r tab action label shortcut function; do
    if [ "$action" = "$1" ]; then
      printf '%s' "$function"
      return 0
    fi
  done <<<"$actions"
  return 1
}

run_action() {
  local function
  if ! function="$(function_for "$1")"; then
    printf 'devws menu: unknown action: %s\n' "$1" >&2
    return 2
  fi
  . "$HOME/.tmuxinator/workspace_actions.sh"
  "$function"
}

open_popup() {
  local client="${1:-}"
  tmux display-popup ${client:+-c "$client"} -E -w 60 -h 16 -T ' devws ' "$script"
}

show_menu() {
  local selected
  selected="$(
    items "$(current_tab)" |
      fzf --ansi --layout=reverse --height=100% --no-sort \
        --delimiter=$'\t' --with-nth=2 \
        --header="$(header)" --footer=$'\033[2m←/→ tab · ↑/↓ move · Enter run · Esc close\033[0m' \
        --prompt='› ' --gutter=' ' --info=hidden --no-separator --border=none --margin=0 --padding=0 \
        --bind="right:transform:'$script' --switch next" \
        --bind="tab:transform:'$script' --switch next" \
        --bind="left:transform:'$script' --switch prev" \
        --bind="btab:transform:'$script' --switch prev" \
        --bind="click-header:transform:'$script' --switch-at \"\$FZF_CLICK_HEADER_COLUMN\"" \
        --bind='left-click:accept'
  )" || return 0
  [ -n "$selected" ] || return 0
  run_action "${selected%%$'\t'*}"
}

case "${1:-}" in
  --items) items "${2:-}" ;;
  --header) header ;;
  --tab) current_tab ;;
  --switch) switch_tab "${2:-}" ;;
  --switch-at) switch_at "${2:-}" ;;
  --run) run_action "${2:-}" ;;
  --function) function_for "${2:-}" ;;
  --popup) open_popup "${2:-}" ;;
  '') show_menu ;;
  *)
    printf 'Usage: devws_menu.sh [--popup [client]]\n' >&2
    exit 2
    ;;
esac
