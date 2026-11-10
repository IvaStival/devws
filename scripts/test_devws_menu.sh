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
# A popup needs a client, which the tests have none of: it is recorded.
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
[ "\$1" = display-popup ] && { printf '%s\n' "\$*" > "$TEST_ROOT/calls/popup-command"; exit 0; }
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
# fzf answers from a queue, one line per call (after the query line that
# --print-query adds); an empty answer is Esc (130, as fzf). The agent
# folders it offers are recorded.
cat > "$TEST_ROOT/bin/fzf" <<EOF
#!/usr/bin/env bash
# Whether the menu marked a popup open while it asks (devws_menu.sh).
[ -e "$TEST_ROOT/devws-popup-$(id -u)" ] && echo open > "$TEST_ROOT/calls/popup" || echo closed > "$TEST_ROOT/calls/popup"
cat > "$TEST_ROOT/calls/fzf.input"
answer="\$(head -n 1 "$TEST_ROOT/fzf.answers")"
sed -i '' 1d "$TEST_ROOT/fzf.answers"
case " \$* " in *" --print-query "*) printf '\\n' ;; esac
[ -n "\$answer" ] || exit 130
printf '%s\\n' "\$answer"
EOF
# tmuxinator records the workspace it was asked to start.
cat > "$TEST_ROOT/bin/tmuxinator" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" > "$TEST_ROOT/calls/tmuxinator"
EOF
chmod +x "$TEST_ROOT/bin/tmux" "$TEST_ROOT/bin/fzf" "$TEST_ROOT/bin/tmuxinator"
mkdir -p "$TEST_ROOT/home/.claude" "$TEST_ROOT/home/.claude-work"
for script in devws_menu.sh workspace_actions.sh workspace_panes.sh session_picker.sh agents.sh worktree.sh recent.sh picker.sh .env; do
  ln -s "$SCRIPTS/$script" "$TEST_ROOT/home/.tmuxinator/$script"
done
for script in toggle_layout.sh minimize_panes.sh header_toggle.sh open_database.sh follow_mode.sh register_agent.sh \
  new_panel.sh move_panel.sh save_state.sh refresh_devws_pickers.sh git_actions.sh jungleforge.sh job.sh agentic.sh; do
  cat > "$TEST_ROOT/home/.tmuxinator/$script" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" > "$TEST_ROOT/calls/$script"
EOF
  chmod +x "$TEST_ROOT/home/.tmuxinator/$script"
done
# jungleforge.sh answers its questions from jf.<question> files and logs the
# rest (run, stop…), one call per line; it is set up unless jf.missing exists.
cat > "$TEST_ROOT/home/.tmuxinator/jungleforge.sh" <<EOF
#!/usr/bin/env bash
case "\$1" in
  available) [ ! -e "$TEST_ROOT/jf.missing" ] ;;
  apps) cat "$TEST_ROOT/jf.apps" 2>/dev/null ||
    { echo "devws jungleforge: Not a jungleforge project: register it in /jf/.env" >&2; exit 2; } ;;
  targets | running | settings) cat "$TEST_ROOT/jf.\$1" 2>/dev/null ;;
  # A value "taken" is refused, as a port another app has.
  set) printf '%s\n' "\$*" >> "$TEST_ROOT/calls/jungleforge.sh"
    [ "\${@: -1}" != taken ] || { echo "devws jungleforge: 8092 is already phi-planner's APP_HTTP_PORT" >&2; exit 2; } ;;
  *) printf '%s\n' "\$*" >> "$TEST_ROOT/calls/jungleforge.sh" ;;
esac
EOF
chmod +x "$TEST_ROOT/home/.tmuxinator/jungleforge.sh"
# git_actions.sh answers the PR question "no".
cat > "$TEST_ROOT/home/.tmuxinator/git_actions.sh" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" > "$TEST_ROOT/calls/git_actions.sh"
[ "\$1" = ask-pr ] && printf 'no'
exit 0
EOF
chmod +x "$TEST_ROOT/home/.tmuxinator/git_actions.sh"
# TMPDIR keeps the popup flag apart from a real menu's.
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT"
# As if ai-memory were not installed, whatever this machine has.
export DEVWS_AI_MEMORY=devws-no-ai-memory
unset TMUX

MENU="$HOME/.tmuxinator/devws_menu.sh"
tmux new-session -d -s dev-menu -x 120 -y 30 'sleep 600'
PANE="$(tmux display-message -p -t dev-menu: '#{pane_id}')"
# A normal workspace has an editor panel; the items that arrange around it show.
tmux set-option -p -t "$PANE" @devws_role editor

# Runs the menu the way the popup does: inside the workspace's tmux session.
menu() {
  TMUX_PANE="$PANE" TMUX="fake,1,0" "$MENU" "$@"
}

actions_of() {
  menu --items "$1" | cut -f1 | paste -sd ' ' -
}

test_tabs_list_their_actions() {
  [ "$(actions_of workspace)" = "github agentic new worktree open-folder close tidy detach" ] ||
    fail "Workspace tab: $(actions_of workspace)"
  [ "$(actions_of layout)" = "toggle-layout minimize header new-panel move-panel switch-editor" ] ||
    fail "Layout tab: $(actions_of layout)"
  [ "$(actions_of agent)" = "jump switch-agent restart-agent follow-mode initialize-agent register-agent remove-agent team-roles" ] ||
    fail "Agent tab: $(actions_of agent)"
  [ "$(actions_of git)" = "commit commit-manual push" ] || fail "Git tab: $(actions_of git)"
  [ "$(actions_of app)" = "run-here switch-run stop-run services migrate shell logs info settings register-jungleforge open-database database-folder job-log" ] ||
    fail "App tab: $(actions_of app)"
  [ "$(actions_of help)" = "guide guide-shortcuts guide-workspaces guide-layout guide-agents guide-agentic guide-git guide-app guide-commands guide-configuration" ] ||
    fail "Help tab: $(actions_of help)"
}

# An agent team (devws agentic) has no editor: the items that arrange the
# screen around one, and the database that sits beside it, are not offered.
test_an_agent_team_has_no_editor_items() {
  tmux set-option -p -t "$PANE" @devws_role agent
  [ "$(actions_of layout)" = "header new-panel move-panel" ] || fail "Layout tab of an agent team: $(actions_of layout)"
  ! grep -qE 'open-database|database-folder' <<<"$(actions_of app)" || fail "an agent team is offered the database: $(actions_of app)"
  grep -q 'job-log' <<<"$(actions_of app)" || fail "an agent team lost the job log"
  tmux set-option -p -t "$PANE" @devws_role editor
  [ "$(actions_of layout)" = "toggle-layout minimize header new-panel move-panel switch-editor" ] ||
    fail "Layout tab of a normal workspace: $(actions_of layout)"
}

test_open_an_agent_team_runs_devws_agentic() {
  rm -f "$TEST_ROOT/calls/agentic.sh"
  menu --run agentic </dev/null >/dev/null 2>&1 || true
  [ -e "$TEST_ROOT/calls/agentic.sh" ] || fail "Open an agent team did not run agentic.sh"
}

# An agent team's workspace (@devws_task) offers moving its leader to the
# leader's other program; devAgentics makes the move once its turn is over.
test_an_agent_team_switches_its_leaders_model() {
  ! grep -q switch-leader <<<"$(actions_of team)" || fail "a workspace without a task offers Switch leader model"
  cat > "$TEST_ROOT/bin/devagentics-stub" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" > "$TEST_ROOT/calls/devagentics"
echo "The leader moves to claude once its turn is over."
EOF
  chmod +x "$TEST_ROOT/bin/devagentics-stub"
  tmux set-option -t dev-menu @devws_task feat-x
  [ "$(actions_of team)" = "team-overview team-agents team-steps team-activity team-usage team-leader switch-leader" ] ||
    fail "Team tab: $(actions_of team)"
  grep -q 'Team' <<<"$(menu --header)" || fail "team header missing"
  menu --switch Team >/dev/null
  [ "$(menu --tab)" = team ] || fail "Team tab cannot be selected"
  menu --switch next >/dev/null
  [ "$(menu --tab)" = git ] || fail "Team tab does not cycle to Git"
  menu --switch prev >/dev/null
  [ "$(menu --tab)" = team ] || fail "Git tab does not cycle back to Team"
  local output
  output="$(DEVWS_AGENTIC_COMMAND=devagentics-stub menu --run switch-leader <<<'' 2>&1)"
  tmux set-option -u -t dev-menu @devws_task
  [ "$(cat "$TEST_ROOT/calls/devagentics" 2>/dev/null)" = "leader switch feat-x" ] ||
    fail "Switch leader model ran '$(cat "$TEST_ROOT/calls/devagentics" 2>/dev/null)'"
  grep -q "moves to claude" <<<"$output" || fail "Switch leader model did not say where: $output"
  ! grep -q 'Team' <<<"$(menu --header)" || fail "normal workspace has a Team tab"
  [ "$(menu --tab)" = workspace ] || fail "stale Team selection was not reset"
}

test_team_navigation_reuses_the_board_from_group_workspaces() {
  local original_pane="$PANE" board leader view count output
  tmux new-session -d -s dev-team -x 120 -y 30 'sleep 600'
  leader="$(tmux display-message -p -t dev-team: '#{pane_id}')"
  tmux set-option -p -t "$leader" @devws_role agent
  tmux set-option -t dev-team @devws_task feat-team
  board="$(tmux split-window -t "$leader" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$board" @devws_role team-log
  tmux set-option -t dev-menu @devws_team_of dev-team
  count="$(tmux list-panes -a | wc -l | tr -d ' ')"
  [ "$(actions_of team)" = "team-overview team-agents team-steps team-activity team-usage team-leader switch-leader" ] ||
    fail "group workspace has no team navigation"
  grep -q $'^team-steps\tTeam · ' <<<"$(menu --items-for steps)" || fail "global search lost Team actions"
  for view in overview agents steps activity usage; do
    menu --run "team-$view" || fail "could not open $view"
    [ "$(tmux show-options -pqv -t "$board" @devagentics_board_view)" = "$view" ] || fail "$view did not target the existing board"
    [ "$(tmux display-message -p -t "$board" '#{pane_active}')" = 1 ] || fail "board not focused"
  done
  menu --run team-leader || fail "could not return to leader"
  [ "$(tmux display-message -p -t "$leader" '#{pane_active}')" = 1 ] || fail "leader not focused"
  [ "$(tmux list-panes -a | wc -l | tr -d ' ')" = "$count" ] || fail "navigation created another pane"
  output="$(DEVWS_AGENTIC_COMMAND=devagentics-stub menu --run switch-leader <<<'' 2>&1)"
  [ "$(cat "$TEST_ROOT/calls/devagentics")" = "leader switch feat-team" ] || fail "group leader switch used the wrong task"
  tmux kill-pane -t "$board"
  if output="$(menu --run team-overview 2>&1)"; then
    fail "missing board was accepted"
  fi
  grep -q 'not open' <<<"$output" || fail "missing board was not explained"
  tmux set-option -u -t dev-menu @devws_team_of
  tmux kill-session -t dev-team
  PANE="$original_pane"
}

# Team roles: a role, then a setting, then a value of its program's (from
# devAgentics), Default or a typed model; devAgentics changes the role's file.
test_team_roles_change_a_roles_setting() {
  local output role='hammer-claude  floor   claude  claude-haiku-5-5  low  auto  writes  ~/.devagentics/roles'
  cat > "$TEST_ROOT/bin/devagentics-stub" <<EOF
#!/usr/bin/env bash
case "\$1 \$2" in
  'role list') printf '%s\n' '$role' 'leader         cto     claude  -                 high -     reads   built in' ;;
  'role choices') printf '%s\n' claude-opus-5-5 claude-sonnet-5-5 ;;
  'role set') printf '%s\n' "\$*" >> "$TEST_ROOT/calls/role-set"; echo 'hammer-claude changed.' ;;
esac
EOF
  chmod +x "$TEST_ROOT/bin/devagentics-stub"
  tmux set-option -t dev-menu @devws_root "$TEST_ROOT/projects/app"
  grep -qw team-roles <<<"$(actions_of agent)" || fail "the Agent tab has no Team roles"
  change_role() {
    rm -f "$TEST_ROOT/calls/role-set"
    printf '%s\n' "$@" > "$TEST_ROOT/fzf.answers"
    # Esc ends an action unsuccessfully, as the menu's others.
    output="$(DEVWS_AGENTIC_COMMAND=devagentics-stub menu --run team-roles <<<"${typed:-}" 2>&1)" || true
  }
  role_set() { cat "$TEST_ROOT/calls/role-set" 2>/dev/null; }

  change_role "$role" model claude-sonnet-5-5
  [ "$(role_set)" = "role set hammer-claude --model claude-sonnet-5-5 --repo $TEST_ROOT/projects/app" ] ||
    fail "Team roles ran '$(role_set)'"
  grep -q 'hammer-claude changed' <<<"$output" || fail "Team roles did not show what changed: $output"
  change_role "$role" effort Default
  [ "$(role_set)" = "role set hammer-claude --effort  --repo $TEST_ROOT/projects/app" ] ||
    fail "Default did not clear the setting: '$(role_set)'"
  typed=a-model-from-next-week change_role "$role" model 'Other…'
  [ "$(role_set)" = "role set hammer-claude --model a-model-from-next-week --repo $TEST_ROOT/projects/app" ] ||
    fail "Other… did not take the typed model: '$(role_set)'"
  change_role "$role" ''
  [ -z "$(role_set)" ] || fail "Esc still changed a role: $(role_set)"
  # The leader's program changes with Switch leader model: it is not listed.
  change_role ''
  ! grep -q '^leader ' "$TEST_ROOT/calls/fzf.input" || fail "Team roles offers the leader"
  unset -f change_role role_set
  tmux set-option -u -t dev-menu @devws_root
}

test_items_show_labels_and_shortcuts() {
  # Captured first: with pipefail, grep -q stopping early would fail the pipe.
  local layout agent app
  layout="$(menu --items layout)"
  agent="$(menu --items agent)"
  app="$(menu --items app)"
  grep -q $'^toggle-layout\t.*Toggle layout.*C-b \\\\' <<<"$layout" || fail "Toggle layout has no shortcut"
  grep -q $'^minimize\t.*Minimize agent and terminal.*C-b _' <<<"$layout" || fail "Minimize has no shortcut"
  grep -q $'^header\t.*Hide or show the header.*C-b C-s' <<<"$layout" || fail "Hide header has no shortcut"
  grep -q $'^new-panel\t.*New panel.*C-b N' <<<"$layout" || fail "New panel has no shortcut"
  grep -q $'^move-panel\t.*Move panel.*C-b S-' <<<"$layout" || fail "Move panel has no shortcut"
  grep -q $'^open-database\t.*Database.*C-b B' <<<"$app" || fail "Database has no shortcut"
  grep -q $'^follow-mode\t.*Agent follow.*C-b F' <<<"$agent" || fail "Agent follow has no shortcut"
  grep -q $'^switch-agent\t.*Switch agent' <<<"$agent" || fail "Switch agent has no label"
}

# The memory items show only with ai-memory installed (DEVWS_AI_MEMORY: none
# for these tests, whatever the machine has, unless pointed at a stub).
test_memory_items_need_ai_memory() {
  ! grep -q memory- <<<"$(actions_of agent)" || fail "memory items without ai-memory"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$TEST_ROOT/bin/ai-memory-stub"
  chmod +x "$TEST_ROOT/bin/ai-memory-stub"
  [ "$(DEVWS_AI_MEMORY="$TEST_ROOT/bin/ai-memory-stub" actions_of agent | grep -o 'memory-[a-z]*' | tr '\n' ' ')" = \
    "memory-search memory-recent memory-web memory-doctor " ] ||
    fail "memory items with ai-memory: $(DEVWS_AI_MEMORY="$TEST_ROOT/bin/ai-memory-stub" actions_of agent)"
}

# Register in jungleforge: the folder the app is in, its name (the repo's by
# default) and, with no deploy/ there, a template to scaffold one from.
test_register_scaffolds_and_registers_an_app() {
  local root="$TEST_ROOT/projects/app"
  tmux set-option -t dev-menu @devws_root "$root"
  mkdir -p "$root/web"
  printf 'web\tweb\nphp (deploy)\tdeploy\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/jungleforge.sh"
  printf '\nn\n\n' | menu --run register-jungleforge >/dev/null 2>&1
  grep -qx "scaffold $root/web deploy app" "$TEST_ROOT/calls/jungleforge.sh" ||
    fail "no deploy/ was scaffolded: $(cat "$TEST_ROOT/calls/jungleforge.sh" 2>/dev/null)"
  # The registry gets git's own (symlink-free) path of the checkout.
  grep -qx "register app $(cd "$root" && pwd -P)/web" "$TEST_ROOT/calls/jungleforge.sh" ||
    fail "the app was not registered: $(cat "$TEST_ROOT/calls/jungleforge.sh" 2>/dev/null)"
  tmux set-option -u -t dev-menu @devws_root
}

# Running a repo jungleforge does not know offers to register it; a no runs
# nothing.
test_running_an_unregistered_repo_offers_registering() {
  local output
  tmux set-option -t dev-menu @devws_root "$TEST_ROOT/projects/app"
  rm -f "$TEST_ROOT/jf.apps" "$TEST_ROOT/calls/job.sh"
  output="$(printf 'n\n' | menu --run run-here 2>&1)" || true
  grep -q 'Register it in jungleforge now' <<<"$output" || fail "registering was not offered: $output"
  [ -z "$(job_call)" ] || fail "a no still ran: $(job_call)"
  tmux set-option -u -t dev-menu @devws_root
}

# Without jungleforge the App tab offers setting it up instead of running
# branches; the database and the job log stay.
test_the_app_tab_follows_jungleforge() {
  : > "$TEST_ROOT/jf.missing"
  [ "$(actions_of app)" = "setup-jungleforge open-database database-folder job-log" ] ||
    fail "App tab without jungleforge: $(actions_of app)"
  rm "$TEST_ROOT/jf.missing"
}

# With an empty query the menu shows its tab; typing searches every tab, each
# match tagged with its tab.
test_typing_searches_every_tab() {
  local all
  tmux set-option -t dev-menu @devws_menu_tab workspace
  [ "$(menu --items-for '')" = "$(menu --items workspace)" ] || fail "an empty query does not show the tab"
  all="$(menu --items-for data)"
  grep -q $'^open-database\tApp · ⛁ Database' <<<"$all" || fail "typing does not reach the App tab: $all"
  grep -q $'^jump\tAgent · ! Go to waiting agent' <<<"$all" || fail "not every tab is searched: $all"
  # Every action shown in a tab, once.
  [ "$(grep -c . <<<"$all")" -eq "$(for tab in workspace layout agent team git app help; do menu --items "$tab"; done | grep -c .)" ] ||
    fail "a search does not list every action once"
}

# The frequent actions have keys of their own, which open them in the menu's
# popup.
test_the_frequent_actions_have_keys() {
  local key action
  for key in e:run-here E:switch-run g:commit G:push j:job-log; do
    action="${key#*:}"
    key="${key%%:*}"
    grep -q "^bind $key run-shell -b \"~/.tmuxinator/devws_menu.sh --popup-run $action '#{client_name}'\"" \
      "$ROOT/config/tmux/tmux.conf" || fail "C-b $key does not open $action"
    grep -q $'^'"$action"$'\t.*C-b '"$key"'$' <<<"$(menu --items-for "$action" | sed $'s/\033\\[[0-9;]*m//g; s/ *$//')" ||
      fail "the menu does not show C-b $key beside $action"
  done
}

test_switching_tabs() {
  tmux set-option -t dev-menu -u @devws_menu_tab
  [ "$(menu --tab)" = "workspace" ] || fail "the menu does not start on Workspace"
  [[ "$(menu --switch next)" == *"reload("*"--items-for {q})"* ]] || fail "next did not reload the items"
  [ "$(menu --tab)" = "layout" ] || fail "next did not move to Layout"
  menu --switch prev >/dev/null
  menu --switch prev >/dev/null
  [ "$(menu --tab)" = "help" ] || fail "prev did not wrap around to Help"
  menu --switch next >/dev/null
  [ "$(menu --tab)" = "workspace" ] || fail "next did not wrap around to Workspace"
  # A click on the tab bar " Workspace   Layout   Agent   Git   App   Help " gives its column.
  [[ "$(menu --switch-at 23)" == *transform-header* ]] || fail "a tab click did not change the header"
  [ "$(menu --tab)" = "agent" ] || fail "clicking Agent (column 23) did not open it"
  menu --switch-at 1 >/dev/null
  [ "$(menu --tab)" = "workspace" ] || fail "clicking Workspace (column 1) did not open it"
  menu --switch-at 39 >/dev/null
  [ "$(menu --tab)" = "app" ] || fail "clicking App (column 39) did not open it"
  menu --switch-at 12 >/dev/null
  [ "$(menu --tab)" = "app" ] || fail "a click between tabs changed the tab"
  menu --switch-at 60 >/dev/null
  [ "$(menu --tab)" = "app" ] || fail "a click beside the tabs changed the tab"
  menu --switch-at 44 >/dev/null
  [ "$(menu --tab)" = "help" ] || fail "clicking Help (column 44) did not open it"
  menu --switch Layout >/dev/null
}

test_header_marks_the_current_tab() {
  menu --switch App >/dev/null
  local header
  header="$(menu --header)"
  grep -q $'\033\\[1;30;46m App ' <<<"$header" || fail "the header does not mark App"
  grep -q $'\033\\[2m Workspace ' <<<"$header" || fail "the header does not list every tab"
}

# New panel asks what to open (for an agent, which one) and where — right,
# left, below or above the focused pane — then opens it there (new_panel.sh).
test_new_panel_asks_what_to_open() {
  local answers kind agent place expected
  for answers in "Agent|codex|Right|dev-menu right agent codex" "Terminal||Below|dev-menu below terminal" \
    "Neovim||Left|dev-menu left nvim" "Terminal||Above|dev-menu above terminal"; do
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

# Move panel asks which way, then moves the focused panel (move_panel.sh).
test_move_panel_asks_which_way() {
  local answers place expected
  for answers in "Right|dev-menu right" "Left|dev-menu left" "Below|dev-menu below" "Above|dev-menu above"; do
    IFS='|' read -r place expected <<<"$answers"
    printf '%s\n' "$place" > "$TEST_ROOT/fzf.answers"
    rm -f "$TEST_ROOT/calls/move_panel.sh"
    menu --run move-panel
    [ "$(cat "$TEST_ROOT/calls/move_panel.sh" 2>/dev/null)" = "$expected" ] ||
      fail "Move panel → $place ran move_panel.sh with '$(cat "$TEST_ROOT/calls/move_panel.sh" 2>/dev/null)'"
  done
  : > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/move_panel.sh"
  menu --run move-panel
  [ ! -e "$TEST_ROOT/calls/move_panel.sh" ] || fail "a cancelled Move panel moved one"
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

# With devws-ui installed (rust_tools.sh), it draws the menu: every shown
# action of every tab goes in at once, the tab it returns is remembered and
# its action runs; Esc runs nothing but keeps the tab.
test_devws_ui_draws_the_menu() {
  local expected
  mkdir -p "$HOME/.cargo/bin"
  cat > "$HOME/.cargo/bin/devws-ui" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" > "$TEST_ROOT/calls/ui.args"
cat > "$TEST_ROOT/calls/ui.input"
[ -e "$TEST_ROOT/devws-popup-$(id -u)" ] && echo open > "$TEST_ROOT/calls/popup"
answer="\$(cat "$TEST_ROOT/ui.answer")"
case "\$answer" in esc:*) printf '%s\n' "\${answer#esc:}"; exit 130 ;; esac
printf '%s\n' "\$answer"
EOF
  chmod +x "$HOME/.cargo/bin/devws-ui"
  tmux set-option -t dev-menu @devws_menu_tab layout
  printf 'git\tcommit-manual\n' > "$TEST_ROOT/ui.answer"
  rm -f "$TEST_ROOT/calls/git_actions.sh" "$TEST_ROOT/calls/popup"
  TMUX_PANE="$PANE" TMUX="fake,1,0" "$MENU" || fail "the menu failed with devws-ui"

  [ "$(cat "$TEST_ROOT/calls/ui.args")" = "$(printf '%s\n' menu --tab layout)" ] ||
    fail "devws-ui was given: $(cat "$TEST_ROOT/calls/ui.args")"
  expected="$(for tab in workspace layout agent git app help; do
    menu --items "$tab" | cut -f1 | sed "s/^/$tab\t/"
  done)"
  [ "$(cut -f1,2 "$TEST_ROOT/calls/ui.input")" = "$expected" ] ||
    fail "the rows are not every shown action, tab by tab: $(cut -f1,2 "$TEST_ROOT/calls/ui.input")"
  grep -qx $'git\tpush\t↑ Push / PR\tC-b G' "$TEST_ROOT/calls/ui.input" ||
    fail "a row does not carry its label and shortcut: $(grep push "$TEST_ROOT/calls/ui.input")"
  [ "$(cat "$TEST_ROOT/calls/popup" 2>/dev/null)" = open ] || fail "the menu did not mark itself open"
  [ "$(cat "$TEST_ROOT/calls/git_actions.sh" 2>/dev/null)" = "commit --manual" ] || fail "the chosen action did not run"
  [ "$(menu --tab)" = git ] || fail "the tab devws-ui ended on was not kept: $(menu --tab)"

  printf 'esc:help\n' > "$TEST_ROOT/ui.answer"
  rm -f "$TEST_ROOT/calls/git_actions.sh"
  TMUX_PANE="$PANE" TMUX="fake,1,0" "$MENU" || fail "Esc in devws-ui failed the menu"
  [ "$(menu --tab)" = help ] || fail "Esc did not keep the tab"
  [ ! -e "$TEST_ROOT/calls/git_actions.sh" ] || fail "Esc ran an action"
  rm -rf "$HOME/.cargo"
}

test_script_actions_run_for_the_session() {
  local action script expected
  for action in toggle-layout:toggle_layout.sh:dev-menu minimize:minimize_panes.sh:"dev-menu toggle agents" \
    header:header_toggle.sh:toggle \
    open-database:open_database.sh:dev-menu follow-mode:follow_mode.sh:dev-menu \
    commit:git_actions.sh:commit commit-manual:git_actions.sh:"commit --manual"; do
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

# github_workspace.sh asks its own questions and prints the folder; a
# cancelled one (exit 1) starts nothing.
test_a_github_workspace_opens_the_chosen_folder() {
  printf '#!/usr/bin/env bash\nprintf "%%s\\n" "%s"\n' "$TEST_ROOT/devws/acme/app/app-feat-x" \
    > "$HOME/.tmuxinator/github_workspace.sh"
  chmod +x "$HOME/.tmuxinator/github_workspace.sh"
  printf 'claude\nnvim\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/tmuxinator"
  menu --run github >/dev/null 2>&1
  [ "$(cat "$TEST_ROOT/calls/tmuxinator" 2>/dev/null)" = \
    "start dev project_root=$TEST_ROOT/devws/acme/app/app-feat-x agent=claude editor=nvim" ] ||
    fail "the GitHub workspace started with '$(cat "$TEST_ROOT/calls/tmuxinator" 2>/dev/null)'"

  # Reopened, it has the agent and editor it had, and asks nothing.
  : > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/tmuxinator"
  menu --run github </dev/null >/dev/null 2>&1
  [ "$(cat "$TEST_ROOT/calls/tmuxinator" 2>/dev/null)" = \
    "start dev project_root=$TEST_ROOT/devws/acme/app/app-feat-x agent=claude editor=nvim" ] ||
    fail "a reopened folder did not start with its agent and editor: '$(cat "$TEST_ROOT/calls/tmuxinator" 2>/dev/null)'"

  printf '#!/usr/bin/env bash\nexit 1\n' > "$HOME/.tmuxinator/github_workspace.sh"
  rm -f "$TEST_ROOT/calls/tmuxinator"
  menu --run github </dev/null >/dev/null 2>&1
  [ ! -e "$TEST_ROOT/calls/tmuxinator" ] || fail "a cancelled GitHub workspace started one"
}

# Closing the workspace you are in moves you to another one, not out of
# devws: a real client (on its own tmux server) is attached to it.
test_closing_your_workspace_moves_you_to_another() {
  local client_socket="devws-menu-client-$$"
  workspace_on dev-yours "$TEST_ROOT"
  "$REAL_TMUX" -L "$client_socket" -f /dev/null new-session -d -x 120 -y 30 \
    "TMUX= $REAL_TMUX -L $SOCKET -f /dev/null attach -t dev-yours"
  attached() { [ -n "$(tmux list-clients -t dev-yours -F '#{client_name}' 2>/dev/null)" ]; }
  wait_until attached || fail "the test client did not attach"
  printf '  dev-yours\n' > "$TEST_ROOT/fzf.answers"
  menu --run close </dev/null >/dev/null 2>&1
  wait_until closed dev-yours || fail "the workspace was not closed"
  moved() { tmux list-clients -F '#{session_name}' 2>/dev/null | grep -q '^dev-'; }
  wait_until moved || fail "the client left devws instead of moving to another workspace"
  "$REAL_TMUX" -L "$client_socket" kill-server
}

job_call() {
  cat "$TEST_ROOT/calls/job.sh" 2>/dev/null
}

# Push asks about the PR first, then pushes as a background job, with nothing
# left to wait for in the menu.
test_push_asks_then_runs_as_a_job() {
  tmux set-option -t dev-menu @devws_root "$TEST_ROOT/projects/app"
  rm -f "$TEST_ROOT/calls/job.sh"
  menu --run push </dev/null >/dev/null 2>&1
  [ "$(job_call)" = "start Push main -- env DEVWS_ROOT=$TEST_ROOT/projects/app $HOME/.tmuxinator/git_actions.sh push --pr no" ] ||
    fail "push started '$(job_call)'"
  tmux set-option -u -t dev-menu @devws_root
}

# Run this branch moves the ticked apps to the workspace's folder, as a job
# holding each app's lock.
test_run_this_branch_moves_the_ticked_apps() {
  tmux set-option -t dev-menu @devws_root "$TEST_ROOT/devws/app-feat-x"
  printf 'phi-planner\tbackend\nphi-planner-web\tweb\n' > "$TEST_ROOT/jf.apps"
  printf 'phi-planner\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/job.sh"
  menu --run run-here </dev/null >/dev/null 2>&1
  [ "$(job_call)" = "start phi-planner → app-feat-x --lock phi-planner -- $HOME/.tmuxinator/jungleforge.sh run $TEST_ROOT/devws/app-feat-x phi-planner" ] ||
    fail "run-here started '$(job_call)'"
}

# Settings: whose (an app of the repo, or the shared services), which one,
# the new value, asked again while refused; an app running from this folder
# is restarted on yes.
test_settings_change_a_value_and_offer_a_restart() {
  local root="$TEST_ROOT/devws/app-feat-x"
  tmux set-option -t dev-menu @devws_root "$root"
  printf 'phi-planner\tbackend\nphi-planner-web\tweb\n' > "$TEST_ROOT/jf.apps"
  printf 'APP_HTTP_PORT\t8092\tnginx\nPHP_CONTAINER\tphi-php\t\n' > "$TEST_ROOT/jf.settings"
  printf 'phi-planner\t%s\n' "$root" > "$TEST_ROOT/jf.running"
  printf 'phi-planner · backend/deploy/.docker-env\tphi-planner\nAPP_HTTP_PORT = 8092   # nginx\tAPP_HTTP_PORT\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/job.sh" "$TEST_ROOT/calls/jungleforge.sh"
  printf 'taken\n8093\n\n' | menu --run settings >/dev/null 2>&1
  grep -qF 'PHP_CONTAINER = phi-php' "$TEST_ROOT/calls/fzf.input" || fail "the settings list: $(cat "$TEST_ROOT/calls/fzf.input")"
  [ "$(cat "$TEST_ROOT/calls/jungleforge.sh")" = "set $root phi-planner APP_HTTP_PORT taken"$'\n'"set $root phi-planner APP_HTTP_PORT 8093" ] ||
    fail "settings set: $(cat "$TEST_ROOT/calls/jungleforge.sh")"
  [ "$(job_call)" = "start phi-planner → restart --lock phi-planner -- $HOME/.tmuxinator/jungleforge.sh run $root phi-planner" ] ||
    fail "the app was not restarted: '$(job_call)'"

  # The shared services: their own restart.
  printf 'shared services · jungleforge .env\tshared\nAPP_HTTP_PORT = 8092   # nginx\tAPP_HTTP_PORT\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/job.sh" "$TEST_ROOT/calls/jungleforge.sh"
  printf '5433\n\n' | menu --run settings >/dev/null 2>&1
  grep -qx "set shared APP_HTTP_PORT 5433" "$TEST_ROOT/calls/jungleforge.sh" || fail "shared set: $(cat "$TEST_ROOT/calls/jungleforge.sh")"
  [ "$(job_call)" = "start Services --lock services -- $HOME/.tmuxinator/jungleforge.sh services" ] ||
    fail "the services were not restarted: '$(job_call)'"
  rm -f "$TEST_ROOT/jf.running" "$TEST_ROOT/jf.settings" "$TEST_ROOT/jf.apps"
}

# Esc on the apps goes back to the branch list (the stub's empty answer is
# Esc: nothing chosen).
test_esc_on_the_apps_goes_back_to_the_branches() {
  local main="$TEST_ROOT/projects/PhiPlanner"
  tmux set-option -t dev-menu @devws_root "$TEST_ROOT/devws/app-feat-x"
  printf 'phi-planner\tbackend\nphi-planner-web\tweb\n' > "$TEST_ROOT/jf.apps"
  printf 'main\t%s\n' "$main" > "$TEST_ROOT/jf.targets"
  printf '  main (%s)\t%s\n\n  main (%s)\t%s\nphi-planner\n' "$main" "$main" "$main" "$main" > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/job.sh"
  menu --run switch-run </dev/null >/dev/null 2>&1
  [ "$(job_call)" = "start phi-planner → PhiPlanner --lock phi-planner -- $HOME/.tmuxinator/jungleforge.sh run $main phi-planner" ] ||
    fail "after Esc on the apps, switch started '$(job_call)'"
}

# Switch lists main and the worktrees, the running one marked, and runs the
# chosen one; a repo with a single app is not asked which.
test_switch_marks_the_running_branch_and_runs_the_chosen_one() {
  local main="$TEST_ROOT/projects/PhiPlanner" branch="$TEST_ROOT/devws/app-feat-x"
  tmux set-option -t dev-menu @devws_root "$branch"
  printf 'phi-planner\tbackend\n' > "$TEST_ROOT/jf.apps"
  printf 'main\t%s\nfeat/x\t%s\n' "$main" "$branch" > "$TEST_ROOT/jf.targets"
  printf 'phi-planner\t%s\n' "$branch" > "$TEST_ROOT/jf.running"
  printf '  main (%s)\t%s\n' "$main" "$main" > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/job.sh"
  menu --run switch-run </dev/null >/dev/null 2>&1
  grep -qF "▶ feat/x" "$TEST_ROOT/calls/fzf.input" || fail "the running branch is not marked: $(cat "$TEST_ROOT/calls/fzf.input")"
  [ "$(job_call)" = "start phi-planner → PhiPlanner --lock phi-planner -- $HOME/.tmuxinator/jungleforge.sh run $main phi-planner" ] ||
    fail "switch started '$(job_call)'"
  rm -f "$TEST_ROOT/jf.running"
}

# Closing the running branch's workspace switches its apps back to main by
# default.
test_closing_the_running_branch_switches_back_to_main() {
  local folder main="$TEST_ROOT/projects/app"
  make_repo_with_worktree running
  folder="$TEST_ROOT/projects/app-running"
  workspace_on dev-running "$folder"
  printf 'phi-planner\tbackend\n' > "$TEST_ROOT/jf.apps"
  printf 'main\t%s\nrunning\t%s\n' "$main" "$folder" > "$TEST_ROOT/jf.targets"
  printf 'phi-planner\t%s\n' "$folder" > "$TEST_ROOT/jf.running"
  printf '  dev-running\n' > "$TEST_ROOT/fzf.answers"
  rm -f "$TEST_ROOT/calls/job.sh"
  # Keep the folder (Enter), then the default: back to main (Enter).
  printf '\n\n' | menu --run close >/dev/null 2>&1
  wait_until closed dev-running || fail "the workspace was not closed"
  switched() {
    [ "$(job_call)" = "start phi-planner → main --lock phi-planner -- $HOME/.tmuxinator/jungleforge.sh run $main phi-planner" ]
  }
  wait_until switched || fail "the apps were not switched back to main: $(job_call)"
  rm -f "$TEST_ROOT/jf.running"
}

test_every_action_has_a_function() {
  local tab action function
  for tab in workspace layout agent team git app help; do
    for action in $(actions_of "$tab"); do
      function="$(menu --function "$action")" || fail "no function for $action"
      function="${function%% *}"
      bash -c ". '$SCRIPTS/workspace_actions.sh'; declare -F '$function' >/dev/null" ||
        fail "$action's function $function is not defined in workspace_actions.sh"
    done
  done
  ! menu --run no-such-action 2>/dev/null || fail "an unknown action was accepted"
}

# A Help item opens its guide page in a large popup on the same client, once
# the menu's popup has closed (from the tmux server: run-shell -b).
test_help_opens_the_guide_page() {
  local opened
  menu --run guide-shortcuts
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    [ -s "$TEST_ROOT/calls/popup-command" ] && break
    sleep 0.1
  done
  opened="$(cat "$TEST_ROOT/calls/popup-command" 2>/dev/null)"
  [[ "$opened" == *"-w 90% -h 90%"* ]] || fail "the guide popup is not large: $opened"
  [[ "$opened" == *"guide.sh shortcuts"* ]] || fail "the guide popup does not show the page: $opened"
}

test_sidebar_opens_the_menu() {
  grep -q '☰ Menu' "$SCRIPTS/session_picker.sh" || fail "the sidebar has no Menu entry"
  ! grep -q 'Switch agent' "$SCRIPTS/session_picker.sh" || fail "the sidebar still lists the commands"
  grep -q 'devws_menu.sh' "$SCRIPTS/session_picker.sh" || fail "the sidebar entry does not open the menu"
  bash -n "$SCRIPTS/session_picker.sh" || fail "the sidebar script has a syntax error"
}

# With scroll margins (which tmux turns on for iTerm2), a pane scrolling
# under the menu drags the popup's cells: the pane showed through the menu.
test_scroll_margins_are_off() {
  grep -q "^set -as terminal-overrides ',\*:Cmg@:Clmg@:Dsmg@:Enmg@'" "$ROOT/config/tmux/tmux.conf" ||
    fail "tmux.conf leaves the scroll margins on"
}

test_space_opens_the_menu() {
  grep -q "^bind Space run-shell -b \"~/.tmuxinator/devws_menu.sh --popup '#{client_name}'\"" \
    "$ROOT/config/tmux/tmux.conf" || fail "C-b Space does not open the menu"
}

# Restart and Switch must recreate a closed main agent without replacing
# the editor or terminal that survived it.
test_agent_actions_reopen_a_closed_panel() {
  local action pane terminal
  cat > "$HOME/.tmuxinator/agent_runner.sh" <<'EOF'
#!/usr/bin/env bash
tmux set-option -p -t "$TMUX_PANE" @devws_role agent
tmux set-option -p -t "$TMUX_PANE" @devws_agent "$1"
exec sleep 600
EOF
  printf '#!/usr/bin/env bash\nexit 0\n' > "$HOME/.tmuxinator/agent_status.sh"
  chmod +x "$HOME/.tmuxinator/agent_runner.sh" "$HOME/.tmuxinator/agent_status.sh"
  tmux set-option -t dev-menu @devws_root "$TEST_ROOT"
  tmux set-option -t dev-menu @devws_agent claude
  terminal="$(tmux split-window -t "$PANE" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$terminal" @devws_role terminal
  pane="$(tmux split-window -t "$terminal" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$pane" @devws_role agent

  for action in restart-agent switch-agent; do
    tmux kill-pane -t "$pane"
    printf 'codex\n' > "$TEST_ROOT/fzf.answers"
    menu --run "$action" || fail "$action could not reopen the closed agent panel"
    reopened_agent() {
      pane="$(tmux list-panes -t dev-menu: -F '#{pane_id} #{@devws_role}' | awk '$2 == "agent" { print $1; exit }')"
      [ -n "$pane" ]
    }
    wait_until reopened_agent || fail "$action did not create a main agent panel"
    [ "$(tmux show-options -pqv -t "$PANE" @devws_role)" = editor ] || fail "$action replaced the editor"
    [ "$(tmux show-options -pqv -t "$terminal" @devws_role)" = terminal ] || fail "$action replaced the terminal"
    [ "$(tmux display-message -p -t "$pane" '#{pane_current_path}')" = "$(cd "$TEST_ROOT" && pwd -P)" ] || fail "$action opened outside the project"
  done
  [ "$(tmux show-options -pqv -t "$pane" @devws_agent)" = codex ] || fail "Switch did not start the chosen agent"
  tmux kill-pane -t "$pane"
  tmux kill-pane -t "$terminal"
  tmux set-option -u -t dev-menu @devws_root
}

test_agent_actions_reopen_a_closed_panel
test_tabs_list_their_actions
test_an_agent_team_has_no_editor_items
test_open_an_agent_team_runs_devws_agentic
test_an_agent_team_switches_its_leaders_model
test_team_navigation_reuses_the_board_from_group_workspaces
test_team_roles_change_a_roles_setting
test_items_show_labels_and_shortcuts
test_typing_searches_every_tab
test_memory_items_need_ai_memory
test_the_app_tab_follows_jungleforge
test_the_frequent_actions_have_keys
test_switching_tabs
test_header_marks_the_current_tab
test_devws_ui_draws_the_menu
test_script_actions_run_for_the_session
test_new_panel_asks_what_to_open
test_move_panel_asks_which_way
test_the_menu_marks_itself_open
test_register_and_remove_agents
test_closing_a_worktree_workspace_asks_before_removing_it
test_a_worktree_with_changes_is_kept_without_asking
test_closing_the_main_folder_never_asks
test_closing_your_workspace_moves_you_to_another
test_a_worktree_workspace_on_a_new_branch
test_a_github_workspace_opens_the_chosen_folder
test_push_asks_then_runs_as_a_job
test_run_this_branch_moves_the_ticked_apps
test_settings_change_a_value_and_offer_a_restart
test_register_scaffolds_and_registers_an_app
test_running_an_unregistered_repo_offers_registering
test_esc_on_the_apps_goes_back_to_the_branches
test_switch_marks_the_running_branch_and_runs_the_chosen_one
test_closing_the_running_branch_switches_back_to_main
test_every_action_has_a_function
test_help_opens_the_guide_page
test_sidebar_opens_the_menu
test_space_opens_the_menu
test_scroll_margins_are_off

printf 'devws menu tests passed.\n'
