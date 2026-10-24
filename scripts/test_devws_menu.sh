#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-menu.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-menu-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'devws menu test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call goes to an isolated server. ~/.tmuxinator holds the real
# menu and actions, and logging stand-ins for the scripts the actions call.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator" "$TEST_ROOT/calls"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
# fzf answers from a queue, one line per call (after the query line that
# --print-query adds); the agent folders it offers are recorded.
cat > "$TEST_ROOT/bin/fzf" <<EOF
#!/usr/bin/env bash
# Whether the menu marked a popup open while it asks (devws_menu.sh).
[ -e "$TEST_ROOT/devws-popup-$(id -u)" ] && echo open > "$TEST_ROOT/calls/popup" || echo closed > "$TEST_ROOT/calls/popup"
cat > "$TEST_ROOT/calls/fzf.input"
answer="\$(head -n 1 "$TEST_ROOT/fzf.answers")"
sed -i '' 1d "$TEST_ROOT/fzf.answers"
case " \$* " in *" --print-query "*) printf '\\n' ;; esac
[ -n "\$answer" ] && printf '%s\\n' "\$answer"
EOF
# tmuxinator records the workspace it was asked to start.
cat > "$TEST_ROOT/bin/tmuxinator" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" > "$TEST_ROOT/calls/tmuxinator"
EOF
chmod +x "$TEST_ROOT/bin/tmux" "$TEST_ROOT/bin/fzf" "$TEST_ROOT/bin/tmuxinator"
mkdir -p "$TEST_ROOT/home/.claude" "$TEST_ROOT/home/.claude-work"
for script in devws_menu.sh workspace_actions.sh workspace_panes.sh session_picker.sh agents.sh worktree.sh .env; do
  ln -s "$SCRIPTS/$script" "$TEST_ROOT/home/.tmuxinator/$script"
done
for script in toggle_layout.sh minimize_panes.sh header_toggle.sh open_database.sh follow_mode.sh register_agent.sh \
  new_panel.sh save_state.sh refresh_devws_pickers.sh; do
  cat > "$TEST_ROOT/home/.tmuxinator/$script" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" > "$TEST_ROOT/calls/$script"
EOF
  chmod +x "$TEST_ROOT/home/.tmuxinator/$script"
done
# TMPDIR keeps the popup flag apart from a real menu's.
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT"
unset TMUX

MENU="$HOME/.tmuxinator/devws_menu.sh"
tmux new-session -d -s dev-menu -x 120 -y 30 'sleep 600'
PANE="$(tmux display-message -p -t dev-menu: '#{pane_id}')"

# Runs the menu the way the popup does: inside the workspace's tmux session.
menu() {
  TMUX_PANE="$PANE" TMUX="fake,1,0" "$MENU" "$@"
}

actions_of() {
  menu --items "$1" | cut -f1 | paste -sd ' ' -
}

test_tabs_list_their_actions() {
  [ "$(actions_of workspace)" = "new worktree open-folder toggle-layout minimize header new-panel close" ] ||
    fail "Workspace tab: $(actions_of workspace)"
  [ "$(actions_of agent)" = "jump switch-agent restart-agent initialize-agent register-agent remove-agent follow-mode" ] ||
    fail "Agent tab: $(actions_of agent)"
  [ "$(actions_of editor)" = "switch-editor follow-mode" ] || fail "Editor tab: $(actions_of editor)"
  [ "$(actions_of database)" = "open-database database-folder" ] ||
    fail "Database tab: $(actions_of database)"
}

test_items_show_labels_and_shortcuts() {
  # Captured first: with pipefail, grep -q stopping early would fail the pipe.
  local workspace agent database
  workspace="$(menu --items workspace)"
  agent="$(menu --items agent)"
  database="$(menu --items database)"
  grep -q $'^toggle-layout\t.*Toggle layout.*C-b \\\\' <<<"$workspace" || fail "Toggle layout has no shortcut"
  grep -q $'^minimize\t.*Minimize agent and terminal.*C-b _' <<<"$workspace" || fail "Minimize has no shortcut"
  grep -q $'^header\t.*Hide or show the header.*C-b C-s' <<<"$workspace" || fail "Hide header has no shortcut"
  grep -q $'^new-panel\t.*New panel.*C-b N' <<<"$workspace" || fail "New panel has no shortcut"
  grep -q $'^open-database\t.*Database.*C-b B' <<<"$database" || fail "Database has no shortcut"
  grep -q $'^follow-mode\t.*Agent follow.*C-b F' <<<"$agent" || fail "Agent follow has no shortcut"
  grep -q $'^switch-agent\t.*Switch agent' <<<"$agent" || fail "Switch agent has no label"
}

test_switching_tabs() {
  tmux set-option -t dev-menu -u @devws_menu_tab
  [ "$(menu --tab)" = "workspace" ] || fail "the menu does not start on Workspace"
  [[ "$(menu --switch next)" == *"reload("*"--items agent)"* ]] || fail "next did not reload the Agent tab"
  [ "$(menu --tab)" = "agent" ] || fail "next did not move to Agent"
  menu --switch prev >/dev/null
  menu --switch prev >/dev/null
  [ "$(menu --tab)" = "database" ] || fail "prev did not wrap around to Database"
  menu --switch next >/dev/null
  [ "$(menu --tab)" = "workspace" ] || fail "next did not wrap around to Workspace"
  # A click on the tab bar " Workspace   Agent   Editor   Database " gives its column.
  [[ "$(menu --switch-at 23)" == *transform-header* ]] || fail "a tab click did not change the header"
  [ "$(menu --tab)" = "editor" ] || fail "clicking Editor (column 23) did not open it"
  menu --switch-at 1 >/dev/null
  [ "$(menu --tab)" = "workspace" ] || fail "clicking Workspace (column 1) did not open it"
  menu --switch-at 39 >/dev/null
  [ "$(menu --tab)" = "database" ] || fail "clicking Database (column 39) did not open it"
  menu --switch-at 12 >/dev/null
  [ "$(menu --tab)" = "database" ] || fail "a click between tabs changed the tab"
  menu --switch-at 55 >/dev/null
  [ "$(menu --tab)" = "database" ] || fail "a click beside the tabs changed the tab"
  menu --switch Editor >/dev/null
}

test_header_marks_the_current_tab() {
  menu --switch Database >/dev/null
  local header
  header="$(menu --header)"
  grep -q $'\033\\[1;30;46m Database ' <<<"$header" || fail "the header does not mark Database"
  grep -q $'\033\\[2m Workspace ' <<<"$header" || fail "the header does not list every tab"
}

# New panel asks what to open (for an agent, which one) and where, beside or
# below the focused pane, then opens it there (new_panel.sh).
test_new_panel_asks_what_to_open() {
  local answers kind agent place expected
  for answers in "Agent|codex|Beside|dev-menu side agent codex" "Terminal||Below|dev-menu below terminal" \
    "Neovim||Beside|dev-menu side nvim"; do
    IFS='|' read -r kind agent place expected <<<"$answers"
    printf '%s\n' "$kind" ${agent:+"$agent"} "$place" > "$TEST_ROOT/fzf.answers"
    rm -f "$TEST_ROOT/calls/new_panel.sh"
    menu --run new-panel
    [ "$(cat "$TEST_ROOT/calls/new_panel.sh" 2>/dev/null)" = "$expected" ] ||
      fail "New panel → $kind ran new_panel.sh with '$(cat "$TEST_ROOT/calls/new_panel.sh" 2>/dev/null)'"
  done
  : > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/new_panel.sh"
  menu --run new-panel
  [ ! -e "$TEST_ROOT/calls/new_panel.sh" ] || fail "a cancelled New panel opened one"
}

# While the menu is open the header and spinners hold still (they would show
# the panes behind it): the menu marks itself open, and always unmarks.
test_the_menu_marks_itself_open() {
  : > "$TEST_ROOT/fzf.answers"
  TMUX_PANE="$PANE" TMUX="fake,1,0" "$MENU" || fail "the menu failed"
  [ "$(cat "$TEST_ROOT/calls/popup" 2>/dev/null)" = open ] || fail "the menu did not mark itself open"
  [ ! -e "$TEST_ROOT/devws-popup-$(id -u)" ] || fail "the closed menu left the popup flag"
  printf 'Terminal\nBelow\n' > "$TEST_ROOT/fzf.answers"
  menu --run new-panel
  [ "$(cat "$TEST_ROOT/calls/popup")" = open ] || fail "New panel's popup was not marked open"
  [ ! -e "$TEST_ROOT/devws-popup-$(id -u)" ] || fail "New panel left the popup flag"
}

test_script_actions_run_for_the_session() {
  local action script expected
  for action in toggle-layout:toggle_layout.sh:dev-menu minimize:minimize_panes.sh:"dev-menu toggle agents" \
    header:header_toggle.sh:toggle \
    open-database:open_database.sh:dev-menu follow-mode:follow_mode.sh:dev-menu; do
    IFS=: read -r action script expected <<<"$action"
    rm -f "$TEST_ROOT/calls/$script"
    menu --run "$action"
    [ "$(cat "$TEST_ROOT/calls/$script" 2>/dev/null)" = "$expected" ] ||
      fail "$action ran $script with '$(cat "$TEST_ROOT/calls/$script" 2>/dev/null)'"
  done
}

# Register asks for the kind, then a folder of that kind not registered yet,
# then a name, which defaults to the folder's suffix.
test_register_and_remove_agents() {
  printf 'claude\n~/.claude-work\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/register_agent.sh"
  printf '\n\n' | DEVWS_AGENTS="claude codex" menu --run register-agent >/dev/null
  [ "$(cat "$TEST_ROOT/calls/register_agent.sh" 2>/dev/null)" = "add work claude ~/.claude-work" ] ||
    fail "register ran '$(cat "$TEST_ROOT/calls/register_agent.sh" 2>/dev/null)'"
  ! grep -qx '~/.claude' "$TEST_ROOT/calls/fzf.input" || fail "register offered a registered folder"

  # A name that can't be registered is asked again, saying why.
  local output
  printf 'claude\n~/.claude-work\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/register_agent.sh"
  output="$(printf 'My Claude\ncodex\nmy_claude\n\n' | DEVWS_AGENTS="claude codex" menu --run register-agent 2>&1)"
  [ "$(cat "$TEST_ROOT/calls/register_agent.sh" 2>/dev/null)" = "add my_claude claude ~/.claude-work" ] ||
    fail "register ran '$(cat "$TEST_ROOT/calls/register_agent.sh" 2>/dev/null)' after bad names"
  grep -q "letters, digits and _" <<< "$output" || fail "a bad name was not explained: $output"
  grep -q "codex is already registered" <<< "$output" || fail "a taken name was not explained: $output"

  printf 'codex\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/register_agent.sh"
  printf '\n' | DEVWS_AGENTS="claude codex" menu --run remove-agent >/dev/null
  [ "$(cat "$TEST_ROOT/calls/register_agent.sh" 2>/dev/null)" = "remove codex" ] ||
    fail "remove ran '$(cat "$TEST_ROOT/calls/register_agent.sh" 2>/dev/null)'"
}

# A repo in projects/app, with a worktree for <branch> in projects/app-<branch>.
make_repo_with_worktree() {
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
    GIT_AUTHOR_NAME=devws GIT_AUTHOR_EMAIL=devws@example.com \
    GIT_COMMITTER_NAME=devws GIT_COMMITTER_EMAIL=devws@example.com
  REPO="$TEST_ROOT/projects/app"
  if [ ! -d "$REPO" ]; then
    git init -q -b main "$REPO"
    printf 'one\n' > "$REPO/file.txt"
    git -C "$REPO" add file.txt
    git -C "$REPO" commit -q -m first
  fi
  git -C "$REPO" worktree add -q -b "$1" "$TEST_ROOT/projects/app-$1" HEAD
}

# <session> <root>: a workspace to close, so dev-menu stays for the others.
workspace_on() {
  tmux new-session -d -s "$1" -x 120 -y 30 'sleep 600'
  tmux set-option -t "$1" @devws_root "$2"
}

closed() {
  ! tmux has-session -t "=$1" 2>/dev/null
}

wait_until() {
  local attempt
  for attempt in $(seq 1 30); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

test_closing_a_worktree_workspace_asks_before_removing_it() {
  local output folder
  make_repo_with_worktree keep
  folder="$TEST_ROOT/projects/app-keep"
  workspace_on dev-keep "$folder"
  printf '  dev-keep\n' > "$TEST_ROOT/fzf.answers"
  output="$(printf '\n' | menu --run close 2>&1)"
  grep -q "Remove that folder too? The branch keep is kept. \[y/N\]" <<< "$output" ||
    fail "closing a worktree workspace did not ask: $output"
  wait_until closed dev-keep || fail "the workspace was not closed"
  sleep 0.3
  [ -d "$folder" ] || fail "Enter removed the worktree"

  make_repo_with_worktree gone
  folder="$TEST_ROOT/projects/app-gone"
  workspace_on dev-gone "$folder"
  printf '  dev-gone\n' > "$TEST_ROOT/fzf.answers"
  printf 'y\n' | menu --run close >/dev/null 2>&1
  wait_until closed dev-gone || fail "the workspace was not closed"
  removed() { [ ! -e "$folder" ]; }
  wait_until removed || fail "y did not remove the worktree"
  git -C "$REPO" show-ref --verify --quiet refs/heads/gone || fail "removing the worktree deleted its branch"
}

test_a_worktree_with_changes_is_kept_without_asking() {
  local output folder
  make_repo_with_worktree dirty
  folder="$TEST_ROOT/projects/app-dirty"
  printf 'draft\n' > "$folder/notes.txt"
  workspace_on dev-dirty "$folder"
  printf '  dev-dirty\n' > "$TEST_ROOT/fzf.answers"
  # A "y" typed anyway must not remove it.
  output="$(printf 'y\n' | menu --run close 2>&1)"
  grep -q "Keeping the worktree .*uncommitted changes" <<< "$output" || fail "the kept worktree was not explained: $output"
  grep -q '?? notes.txt' <<< "$output" || fail "the changes were not listed: $output"
  ! grep -q "Remove that folder" <<< "$output" || fail "a worktree with changes was offered for removal"
  wait_until closed dev-dirty || fail "the workspace was not closed"
  sleep 0.3
  [ -f "$folder/notes.txt" ] || fail "the worktree with changes was removed"
}

test_closing_the_main_folder_never_asks() {
  local output
  workspace_on dev-main "$REPO"
  printf '  dev-main\n' > "$TEST_ROOT/fzf.answers"
  output="$(printf 'y\n' | menu --run close 2>&1)"
  ! grep -q "Remove that folder" <<< "$output" || fail "the repo's main folder was offered for removal"
  wait_until closed dev-main || fail "the workspace was not closed"
  [ -f "$REPO/file.txt" ] || fail "the repo lost its files"
}

test_a_worktree_workspace_on_a_new_branch() {
  tmux set-option -t dev-menu @devws_root "$REPO"
  printf '+ New branch…\nclaude\nnvim\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/tmuxinator"
  printf 'bad..name\nfeat/login\n' | menu --run worktree >/dev/null 2>&1
  [ "$(cat "$TEST_ROOT/calls/tmuxinator" 2>/dev/null)" = \
    "start dev project_root=$(cd "$TEST_ROOT" && pwd -P)/projects/app-feat-login agent=claude editor=nvim" ] ||
    fail "the worktree workspace started with '$(cat "$TEST_ROOT/calls/tmuxinator" 2>/dev/null)'"
  [ "$(git -C "$TEST_ROOT/projects/app-feat-login" symbolic-ref --short HEAD)" = feat/login ] ||
    fail "the worktree is not on the new branch"
  tmux set-option -u -t dev-menu @devws_root
}

test_every_action_has_a_function() {
  local tab action function
  for tab in workspace agent editor database; do
    for action in $(actions_of "$tab"); do
      function="$(menu --function "$action")" || fail "no function for $action"
      bash -c ". '$SCRIPTS/workspace_actions.sh'; declare -F '$function' >/dev/null" ||
        fail "$action's function $function is not defined in workspace_actions.sh"
    done
  done
  ! menu --run no-such-action 2>/dev/null || fail "an unknown action was accepted"
}

test_sidebar_opens_the_menu() {
  grep -q '☰ Menu' "$SCRIPTS/session_picker.sh" || fail "the sidebar has no Menu entry"
  ! grep -q 'Switch agent' "$SCRIPTS/session_picker.sh" || fail "the sidebar still lists the commands"
  grep -q 'devws_menu.sh' "$SCRIPTS/session_picker.sh" || fail "the sidebar entry does not open the menu"
  bash -n "$SCRIPTS/session_picker.sh" || fail "the sidebar script has a syntax error"
}

test_space_opens_the_menu() {
  grep -q "^bind Space run-shell -b \"~/.tmuxinator/devws_menu.sh --popup '#{client_name}'\"" \
    "$ROOT/config/tmux/tmux.conf" || fail "C-b Space does not open the menu"
}

test_tabs_list_their_actions
test_items_show_labels_and_shortcuts
test_switching_tabs
test_header_marks_the_current_tab
test_script_actions_run_for_the_session
test_new_panel_asks_what_to_open
test_the_menu_marks_itself_open
test_register_and_remove_agents
test_closing_a_worktree_workspace_asks_before_removing_it
test_a_worktree_with_changes_is_kept_without_asking
test_closing_the_main_folder_never_asks
test_a_worktree_workspace_on_a_new_branch
test_every_action_has_a_function
test_sidebar_opens_the_menu
test_space_opens_the_menu

printf 'devws menu tests passed.\n'
