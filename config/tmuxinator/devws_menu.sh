#!/usr/bin/env bash
# The devws menu: every workspace action, grouped in tabs, in a tmux popup
# (C-b Space, or "☰ Menu" in the sidebar). The actions themselves live in
# workspace_actions.sh, shared with the sidebar.
#
#   devws_menu.sh                   the menu (run inside the popup)
#   devws_menu.sh --popup [client]  opens the popup on a tmux client
#   devws_menu.sh --popup-run <action> [client]
#                                   one action alone in the menu's popup (its
#                                   key: C-b e, C-b g…)
#
# fzf calls these back while the menu is open; the tests use them too:
#   --items <tab>   the tab's actions: "<action><TAB><label and shortcut>"
#   --items-for <query>   the current tab's while nothing is typed; else
#                   every tab's, each tagged "<Tab> · ", for fzf to search
#   --header        the tab bar, the current tab marked
#   --tab           the current tab
#   --switch <next|prev|tab title>   changes tab; prints fzf actions to show it
#   --switch-at <column>             changes to the tab clicked at that column
#   --run <action>  runs an action; --function <action> names its function

set -u

script="$HOME/.tmuxinator/devws_menu.sh"
# One concern per tab: which workspaces there are, how the screen is
# arranged, the agent and its team, git, the app the branch runs (jungleforge, its
# database, the background jobs), and the guide (docs/guide/).
tabs=(workspace layout agent git app help)

# tab|action|label|shortcut|function in workspace_actions.sh, with its
# arguments[|when: see shown]
actions='workspace|github|⎇ Open or start a workspace||new_github_workspace
workspace|agentic|◈ Open an agent team||open_agentic_team
workspace|new|+ New folder workspace||new_workspace
workspace|worktree|⑂ New worktree workspace||new_worktree_workspace
workspace|open-folder|⌂ Open folder in…||open_folder
workspace|close|× Close workspace||close_workspace
workspace|tidy|⌫ Tidy up merged branches||tidy_up
workspace|detach|⏏ Detach (workspaces keep running)|C-b d|detach_client
layout|toggle-layout|▥ Toggle layout|C-b \|toggle_layout|editor
layout|minimize|▁ Minimize agent and terminal|C-b _|minimize_agents|editor
layout|header|▔ Hide or show the header|C-b C-s|toggle_header
layout|new-panel|▣ New panel|C-b N|new_panel
layout|move-panel|⇆ Move panel|C-b S-←→↑↓|move_panel
layout|switch-editor|⇄ Switch editor||switch_editor|editor
agent|jump|! Go to waiting agent|C-b a|jump_agent
agent|switch-agent|⇄ Switch agent||switch_agent
agent|restart-agent|↻ Restart agent||restart_agent
agent|follow-mode|◎ Agent follow|C-b F|choose_follow_mode
agent|initialize-agent|◇ Initialize agent||initialize_agent
agent|register-agent|＋ Register agent||register_agent
agent|remove-agent|− Remove agent||remove_agent
agent|team-roles|⚙ Team roles||team_roles
agent|memory-search|⁘ Search memory||search_memory|memory
agent|memory-recent|◷ Recent memory||recent_memory|memory
agent|memory-web|↗ Open memory web UI||open_memory_web|memory
agent|memory-doctor|✚ Memory doctor||memory_doctor|memory
team|team-overview|◈ Team overview||open_team_view overview|agentic
team|team-agents|◇ Team agents||open_team_view agents|agentic
team|team-steps|✓ Team steps||open_team_view steps|agentic
team|team-activity|≡ Team activity||open_team_view activity|agentic
team|team-usage|◷ Team usage||open_team_view usage|agentic
team|team-leader|↩ Return to leader||open_team_leader|agentic
team|switch-leader|⇄ Switch leader model||switch_leader_model|agentic
git|commit|✓ Commit (agent)|C-b g|commit_with_agent
git|commit-manual|✎ Commit manually||commit_by_hand
git|push|↑ Push / PR|C-b G|push_branch
app|setup-jungleforge|⚒ Set up jungleforge||setup_jungleforge|no-jungleforge
app|run-here|▶ Run this branch|C-b e|run_this_branch|jungleforge
app|switch-run|⇄ Switch running branch|C-b E|switch_running_branch|jungleforge
app|stop-run|■ Stop||stop_running_apps|jungleforge
app|services|⛃ Services||start_services|jungleforge
app|migrate|⇡ Migrate||migrate_running_app|jungleforge
app|shell|⌘ Shell||open_app_shell|jungleforge
app|logs|≡ Logs||open_app_logs|jungleforge
app|info|ⓘ Info||show_app_info|jungleforge
app|settings|⚙ Settings||change_app_setting|jungleforge
app|register-jungleforge|＋ Register in jungleforge||register_in_jungleforge|jungleforge
app|open-database|⛁ Database|C-b B|open_database|editor
app|database-folder|⛁ Database folder||choose_database_folder|editor
app|job-log|◷ Job log|C-b j|show_job_log
help|guide|? Start here||open_guide README
help|guide-shortcuts|⌨ Shortcuts||open_guide shortcuts
help|guide-workspaces|⎇ Workspaces||open_guide workspaces
help|guide-layout|▥ Layout||open_guide layout
help|guide-agents|◇ Agents||open_guide agents
help|guide-agentic|◈ Agent teams||open_guide agentic
help|guide-git|✓ Git||open_guide git
help|guide-app|▶ App, jobs and database||open_guide app
help|guide-commands|$ Commands||open_guide commands
help|guide-configuration|⚙ Configuration||open_guide configuration'

session() {
  if [ -n "${TMUX_PANE:-}" ]; then
    tmux display-message -p -t "$TMUX_PANE" '#{session_name}'
  else
    tmux display-message -p '#{session_name}'
  fi
}

# Group workspaces belong to their team's task, even without a task option
# of their own. Cache the context for this menu invocation.
team_workspace="$(tmux show-options -qv -t "=$(session):" @devws_team_of 2>/dev/null)"
team_workspace="${team_workspace:-$(session)}"
team_task="$(tmux show-options -qv -t "=$team_workspace:" @devws_task 2>/dev/null)"
if [ -n "$team_task" ]; then
  tabs=(workspace layout agent team git app help)
fi

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

#   items <tab> [tag]   the tab's actions, their labels led by [tag]
items() {
  local tab action label shortcut function when
  while IFS='|' read -r tab action label shortcut function when; do
    [ "$tab" = "$1" ] || continue
    shown "$when" || continue
    printf '%s\t%-40s \033[2m%s\033[0m\n' "$action" "${2:-}$label" "$shortcut"
  done <<<"$actions"
}

# An action's "when": empty is always; memory is with ai-memory installed;
# jungleforge / no-jungleforge with or without jungleforge set up (asked once
# per listing); editor is in a workspace with an editor panel (an agent team,
# `devws agentic`, has none); agentic is in an agent team's workspace (it has
# a task), including its group workspaces.
shown() {
  case "$1" in
    '') return 0 ;;
    memory) command -v "${DEVWS_AI_MEMORY:-ai-memory}" >/dev/null 2>&1 ;;
    jungleforge) jungleforge_available ;;
    no-jungleforge) ! jungleforge_available ;;
    editor) workspace_has_editor ;;
    agentic) [ -n "$team_task" ] ;;
    *) return 1 ;;
  esac
}

editor_state=""
workspace_has_editor() {
  if [ -z "$editor_state" ]; then
    if tmux list-panes -s -t "=$(session)" -F '#{@devws_role}' 2>/dev/null | grep -qx editor; then
      editor_state=yes
    else
      editor_state=no
    fi
  fi
  [ "$editor_state" = yes ]
}

jungleforge_state=""
jungleforge_available() {
  if [ -z "$jungleforge_state" ]; then
    if "$HOME/.tmuxinator/jungleforge.sh" available 2>/dev/null; then
      jungleforge_state=yes
    else
      jungleforge_state=no
    fi
  fi
  [ "$jungleforge_state" = yes ]
}

# While nothing is typed, the tab; once something is, every tab's actions
# tagged with theirs, which fzf then filters by what was typed.
items_for() {
  local tab
  if [ -z "$1" ]; then
    items "$(current_tab)"
    return
  fi
  for tab in "${tabs[@]}"; do
    items "$tab" "$(title "$tab") · "
  done
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
  # With something typed, the search across every tab stays.
  printf "reload('%s' --items-for {q})+transform-header('%s' --header)+first\n" "$script" "$script"
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
  local tab action label shortcut function when
  while IFS='|' read -r tab action label shortcut function when; do
    if [ "$action" = "$1" ]; then
      printf '%s' "$function"
      return 0
    fi
  done <<<"$actions"
  return 1
}

run_action() {
  local function
  local -a command
  if ! function="$(function_for "$1")"; then
    printf 'devws menu: unknown action: %s\n' "$1" >&2
    return 2
  fi
  read -r -a command <<<"$function"
  . "$HOME/.tmuxinator/workspace_actions.sh"
  "${command[@]}"
}

open_popup() {
  local client="${1:-}"
  # Room for the longest tab (App) whole, and its questions (branches, apps).
  tmux display-popup ${client:+-c "$client"} -E -w 70 -h 18 -T ' devws ' "$script"
}

# One action alone in the menu's popup: what its key (C-b e, C-b g…) opens.
popup_run() {
  local action="$1" client="${2:-}"
  function_for "$action" >/dev/null || { printf 'devws menu: unknown action: %s\n' "$action" >&2; return 2; }
  tmux display-popup ${client:+-c "$client"} -E -w 70 -h 18 -T ' devws ' "$script --run $action"
}

# The menu in the shared picker look (picker.sh). Typing searches every tab.
show_menu() {
  local selected
  selected="$(
    items "$(current_tab)" |
      "$HOME/.tmuxinator/picker.sh" Search "$(header)" \
        --height=100% --no-sort --delimiter=$'\t' --with-nth=2 \
        --footer=$'\033[2m←/→ tab · type to search every tab · Enter run · Esc close\033[0m' \
        --gutter=' ' --info=hidden --border=none --margin=0 --padding=0 \
        --bind="change:reload-sync('$script' --items-for {q})" \
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

# While the menu (or an action it runs, as C-b N's popup does) is open, the
# header's ghost and the agents' spinners hold still: each of their redraws
# would show the panes behind the popup for an instant. They watch this file
# (header_ticker.sh, agent_status.sh).
mark_popup_open() {
  local flag="${TMPDIR:-/tmp}"
  flag="${flag%/}/devws-popup-$(id -u)"
  : > "$flag"
  trap 'rm -f "'"$flag"'"' EXIT
}

case "${1:-}" in
  --run | '') mark_popup_open ;;
esac

case "${1:-}" in
  --items) items "${2:-}" ;;
  --items-for) items_for "${2:-}" ;;
  --popup-run) popup_run "${2:-}" "${3:-}" ;;
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
