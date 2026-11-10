#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-new-panel.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-new-panel-test-$$"
# The same layout dev.yml opens a workspace with: picker, editor, agent, terminal.
DEFAULT_LAYOUT="12f9,200x50,0,0{25x50,0,0,58,174x50,26,0[174x24,26,0,57,174x25,26,25{86x25,26,25,59,87x25,113,25,60}]}"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  # The panes' shells may still be writing as the server goes: try again.
  rm -rf "$TEST_ROOT" 2>/dev/null || { sleep 0.5; rm -rf "$TEST_ROOT"; }
}
trap cleanup EXIT

fail() {
  printf 'New panel test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call goes to an isolated server; claude and nvim are stubs that
# record how they were started. ~/.tmuxinator holds the real scripts and a
# .env with two agents.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator" "$TEST_ROOT/project" "$TEST_ROOT/started"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
for program in claude nvim; do
  cat > "$TEST_ROOT/bin/$program" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$PWD" "\$@" > "$TEST_ROOT/started/$program.\$\$"
exec sleep 600
EOF
done
chmod +x "$TEST_ROOT/bin/"*
for script in "$SCRIPTS"/*.sh "$SCRIPTS"/*.json "$SCRIPTS"/*.md; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
# The sidebar only titles itself, as the real one does, and waits.
rm "$TEST_ROOT/home/.tmuxinator/session_picker.sh"
printf '#!/usr/bin/env bash\ntmux select-pane -t "$TMUX_PANE" -T devws-picker\nexec sleep 600\n' \
  > "$TEST_ROOT/home/.tmuxinator/session_picker.sh"
chmod +x "$TEST_ROOT/home/.tmuxinator/session_picker.sh"
printf 'DEVWS_AGENTS="claude work"\nDEVWS_AGENT_work="claude %s"\n' "$TEST_ROOT/home/.claude-work" \
  > "$TEST_ROOT/home/.tmuxinator/.env"
mkdir -p "$TEST_ROOT/home/.claude-work"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX
SCRIPTS="$HOME/.tmuxinator"
NEW_PANEL="$SCRIPTS/new_panel.sh"

wait_until() {
  local attempt
  for attempt in $(seq 1 30); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

create_workspace() {
  local session="$1" role pane
  tmux new-session -d -s "$session" -x 200 -y 50 -c "$TEST_ROOT/project" 'sleep 600'
  tmux set-option -t "$session" @devws_agent claude
  tmux set-option -t "$session" @devws_root "$TEST_ROOT/project"
  tmux select-pane -t "$session:" -T devws-picker
  for role in editor agent terminal; do
    pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' 'sleep 600')"
    tmux set-option -p -t "$pane" @devws_role "$role"
    tmux select-layout -t "$session:" tiled
  done
  tmux select-layout -t "$session:" "$DEFAULT_LAYOUT"
}

pane_id() {
  tmux list-panes -s -t "$1" -F '#{pane_id} #{@devws_role}' |
    awk -v role="$2" '$2 == role { print $1; exit }'
}

field() {
  tmux display-message -p -t "$1" "#{$2}"
}

# <pane> <of>: right or left of <of>, at its top; below or above <of>, at its
# left edge.
beside() {
  [ "$(field "$1" pane_top)" = "$(field "$2" pane_top)" ] &&
    [ "$(field "$1" pane_left)" -gt "$(field "$2" pane_left)" ]
}

left_of() {
  [ "$(field "$1" pane_top)" = "$(field "$2" pane_top)" ] &&
    [ "$(field "$1" pane_left)" -lt "$(field "$2" pane_left)" ]
}

below() {
  [ "$(field "$1" pane_left)" = "$(field "$2" pane_left)" ] &&
    [ "$(field "$1" pane_top)" -gt "$(field "$2" pane_top)" ]
}

above() {
  [ "$(field "$1" pane_left)" = "$(field "$2" pane_left)" ] &&
    [ "$(field "$1" pane_top)" -lt "$(field "$2" pane_top)" ]
}

started() {
  ls "$TEST_ROOT/started/$1".* >/dev/null 2>&1
}

SESSION=dev-panels
create_workspace "$SESSION"

# A new panel splits the focused pane: right, left, below or above it.
test_an_extra_agent() {
  local pane terminal
  terminal="$(pane_id "$SESSION" terminal)"
  tmux select-pane -t "$terminal"
  pane="$("$NEW_PANEL" "$SESSION" right agent work)" || fail "adding an agent failed"
  wait_until started claude || fail "the extra agent did not start"
  [ "$(tmux show-options -pqv -t "$pane" @devws_role)" = extra-agent ] &&
    [ "$(tmux show-options -pqv -t "$pane" @devws_agent)" = work ] || fail "the extra agent is not marked as one"
  beside "$pane" "$terminal" || fail "the extra agent is not beside the focused terminal"
  [ "$(sed -n 1p "$TEST_ROOT"/started/claude.*)" = "$TEST_ROOT/project" ] || fail "the extra agent did not start in the project"
  grep -qx -- --settings "$TEST_ROOT"/started/claude.* || fail "the extra agent started without devws's hooks"
  [ "$(pane_id "$SESSION" agent)" != "$pane" ] || fail "the extra agent took the main agent's role"
  ! "$NEW_PANEL" "$SESSION" right agent nobody 2>/dev/null || fail "an unregistered agent was added"
}

test_a_terminal_and_a_neovim() {
  local terminal editor main_editor
  main_editor="$(pane_id "$SESSION" editor)"
  tmux select-pane -t "$main_editor"
  editor="$("$NEW_PANEL" "$SESSION" below nvim)" || fail "adding a Neovim failed"
  wait_until started nvim || fail "Neovim did not start"
  [ "$(tmux show-options -pqv -t "$editor" @devws_role)" = extra-editor ] || fail "Neovim is not marked as one"
  [ "$(sed -n 1p "$TEST_ROOT"/started/nvim.*)" = "$TEST_ROOT/project" ] || fail "Neovim did not open the project"
  ! grep -qx -- --listen "$TEST_ROOT"/started/nvim.* || fail "the extra Neovim took the agent's link"
  below "$editor" "$main_editor" || fail "Neovim is not below the focused editor"

  tmux select-pane -t "$main_editor"
  terminal="$("$NEW_PANEL" "$SESSION" right terminal)" || fail "adding a terminal failed"
  [ "$(tmux show-options -pqv -t "$terminal" @devws_role)" = extra ] || fail "the terminal is not an extra one"
  beside "$terminal" "$main_editor" || fail "the terminal is not beside the focused editor"
  ! "$NEW_PANEL" "$SESSION" right sideways 2>/dev/null || fail "an unknown kind was accepted"
  ! "$NEW_PANEL" "$SESSION" diagonal terminal 2>/dev/null || fail "an unknown position was accepted"
}

test_left_and_above() {
  local pane terminal main_editor
  terminal="$(pane_id "$SESSION" terminal)"
  tmux select-pane -t "$terminal"
  pane="$("$NEW_PANEL" "$SESSION" left terminal)" || fail "adding a terminal on the left failed"
  left_of "$pane" "$terminal" || fail "the terminal is not left of the focused terminal"
  [ "$(pane_id "$SESSION" terminal)" = "$terminal" ] || fail "the panel on the left took the terminal's role"

  main_editor="$(pane_id "$SESSION" editor)"
  tmux select-pane -t "$main_editor"
  pane="$("$NEW_PANEL" "$SESSION" above terminal)" || fail "adding a terminal above failed"
  above "$pane" "$main_editor" || fail "the terminal is not above the focused editor"
}

# Opened from the sidebar's menu, the focus is on the sidebar: the panel goes
# by the terminal instead of splitting the narrow sidebar.
test_not_beside_the_sidebar() {
  local pane picker
  picker="$(tmux list-panes -t "$SESSION:" -F '#{pane_id} #{pane_title}' | awk '$2 == "devws-picker" { print $1 }')"
  tmux select-pane -t "$picker"
  pane="$("$NEW_PANEL" "$SESSION" right terminal)" || fail "adding from the sidebar failed"
  beside "$pane" "$(pane_id "$SESSION" terminal)" || fail "a panel opened from the sidebar is not by the terminal"
}

# An agent team's workspaces have no terminal (the leader and the board, or a
# group's agents): from the sidebar the panel goes by another of its panes.
test_from_the_sidebar_without_a_terminal() {
  local session=dev-no-terminal pane agent
  tmux new-session -d -s "$session" -x 200 -y 50 -c "$TEST_ROOT/project" 'sleep 600'
  tmux set-option -t "$session" @devws_root "$TEST_ROOT/project"
  tmux select-pane -t "$session:" -T devws-picker
  agent="$(tmux split-window -d -h -t "$session:" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$agent" @devws_role agent
  pane="$("$NEW_PANEL" "$session" below terminal)" || fail "adding from the sidebar of a workspace with no terminal failed"
  below "$pane" "$agent" || fail "a panel opened from the sidebar with no terminal is not by the agent"
}

# Minimizing moves the focus to the editor; a new panel expands the group
# first, so nothing stays hidden, and splits the focused pane.
test_a_minimized_group_is_expanded_first() {
  local pane focused
  "$SCRIPTS/minimize_panes.sh" "$SESSION" toggle agents
  [ -n "$(pane_id "$SESSION" minimized-terminal)" ] || fail "the group did not minimize"
  focused="$(tmux display-message -p -t "$SESSION:" '#{pane_id}')"
  pane="$("$NEW_PANEL" "$SESSION" below terminal)" || fail "adding to a minimized group failed"
  [ -z "$(pane_id "$SESSION" minimized-terminal)" ] || fail "the minimized group was not expanded"
  below "$pane" "$focused" || fail "the panel is not below the focused pane"
}

test_new_panels_are_saved() {
  local saved="$HOME/.devws-state/sessions.tsv"
  [ -s "$saved" ] || fail "the workspace was not saved"
  awk -F '\t' -v s="$SESSION" '$1 == "P" && $2 == s && $4 == "extra-agent" && $6 == "work"' "$saved" | grep -q . ||
    fail "the extra agent was not saved with its agent: $(cat "$saved")"
  awk -F '\t' -v s="$SESSION" '$1 == "P" && $2 == s && $4 == "extra-editor"' "$saved" | grep -q . ||
    fail "the extra Neovim was not saved"
}

# An agent team's agents go to workspaces of their own, one per group, four
# agents to a workspace, tiled; the team's workspace keeps the leader and the
# board. A group's agent can be peeked under the board and sent back, and
# shown with go.
TEAM=dev-team
# A workspace's panels, its sidebar aside.
group_panes() {
  tmux list-panes -s -t "=$1" -F '#{pane_id} #{@devws_sidebar}' 2>/dev/null | awk '$2 != 1' | grep -c . || true
}

# The group workspace's sidebar: on the left, full height (but the border
# line on top).
has_sidebar() {
  tmux list-panes -t "=$1:" -F '#{@devws_sidebar} #{pane_left} #{pane_height} #{window_height}' |
    awk '$1 == 1 && $2 == 0 && $3 >= $4 - 1 { found = 1 } END { exit !found }'
}

test_agents_open_in_their_groups_workspaces() {
  local pane floor=() senior board main option
  tmux new-session -d -s "$TEAM" -n workspace -x 200 -y 50 -c "$TEST_ROOT/project" 'sleep 600'
  tmux set-option -t "$TEAM" @devws_root "$TEST_ROOT/project"
  tmux set-option -t "$TEAM" @devws_task team-9
  tmux set-option -t "$TEAM" @devws_template agentic
  # As agentic.yml marks it, and kept from renaming itself after its program.
  tmux set-option -w -t "=$TEAM:" @devws_main 1
  tmux set-option -w -t "=$TEAM:" automatic-rename off
  main="$(tmux display-message -p -t "=$TEAM:" '#{window_id}')"
  board="$(tmux split-window -h -t "$main" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$board" @devws_role team-log

  for _ in 1 2 3 4 5; do
    pane="$("$NEW_PANEL" "$TEAM" right agent work --group Floor --no-focus)" || fail "a floor agent did not open"
    floor+=("$pane")
  done
  senior="$("$NEW_PANEL" "$TEAM" right agent work --group Seniors --no-focus)" || fail "a senior did not open"
  [ "$(group_panes "$TEAM")" = 2 ] || fail "an agent opened in the team's workspace"
  [ "$(group_panes "$TEAM-floor")" = 4 ] || fail "the Floor workspace holds $(group_panes "$TEAM-floor"), not 4"
  [ "$(group_panes "$TEAM-floor-2")" = 1 ] || fail "the fifth floor agent is not in Floor 2's workspace"
  [ "$(group_panes "$TEAM-seniors")" = 1 ] || fail "the senior is not in its group's workspace"
  for option in "$TEAM-floor" "$TEAM-floor-2" "$TEAM-seniors"; do
    has_sidebar "$option" || fail "$option has no sidebar on the left: $(tmux list-panes -t "=$option:" -F "#{@devws_sidebar} #{pane_left} #{pane_top} #{pane_height} #{window_height}")"
  done
  for option in @devws_task @devws_template @devws_root; do
    [ "$(tmux show-option -qv -t "=$TEAM-floor:" "$option")" = "$(tmux show-option -qv -t "=$TEAM:" "$option")" ] ||
      fail "the Floor workspace does not share the team's $option"
  done
  [ "$(tmux show-options -pqv -t "$senior" @devws_role)" = extra-agent ] || fail "a group's agent is not an extra agent"
  ! "$NEW_PANEL" "$TEAM" right terminal --group Floor 2>/dev/null || fail "a terminal was put in a group"

  # A lane's agent starts in the lane's worktree, not the workspace's folder.
  mkdir -p "$TEST_ROOT/lane-back"
  rm -f "$TEST_ROOT"/started/claude.*
  pane="$("$NEW_PANEL" "$TEAM" right agent work --group Docs --folder "$TEST_ROOT/lane-back" --no-focus)" ||
    fail "an agent with a folder did not open"
  wait_until started claude || fail "the lane's agent did not start"
  [ "$(sed -n 1p "$TEST_ROOT"/started/claude.*)" = "$TEST_ROOT/lane-back" ] ||
    fail "the agent did not start in its folder: $(sed -n 1p "$TEST_ROOT"/started/claude.*)"
  ! "$NEW_PANEL" "$TEAM" right agent work --group Docs --folder "$TEST_ROOT/nowhere" 2>/dev/null ||
    fail "a missing folder was accepted"
  tmux kill-session -t "=$TEAM-docs"
  rm -f "$TEST_ROOT"/started/claude.*

  # Peek: under the board, then back to its group.
  "$SCRIPTS/agent_cli.sh" peek "$senior" || fail "peek failed"
  [ "$(tmux display-message -p -t "$senior" '#{window_id}')" = "$main" ] || fail "the peeked agent is not in the team's window"
  below "$senior" "$board" || fail "the peeked agent is not under the board"
  ! tmux has-session -t "=$TEAM-seniors" 2>/dev/null || fail "the emptied Seniors workspace stayed"
  "$SCRIPTS/agent_cli.sh" peek "${floor[0]}" || fail "peeking another failed"
  [ "$(tmux display-message -p -t "$senior" '#{session_name}')" = "$TEAM-seniors" ] ||
    fail "peeking another did not send the first back to a new Seniors workspace"
  [ "$(tmux display-message -p -t "${floor[0]}" '#{window_id}')" = "$main" ] || fail "the second peek"
  "$SCRIPTS/agent_cli.sh" peek "${floor[0]}" || fail "peeking it again failed"
  [ "$(tmux display-message -p -t "${floor[0]}" '#{session_name}')" = "$TEAM-floor" ] ||
    fail "peeking again did not send it back to Floor"
  [ "$(group_panes "$TEAM")" = 2 ] || fail "the team's workspace kept a peeked panel"

  # Go: its group's workspace, focused on it.
  "$SCRIPTS/agent_cli.sh" go "${floor[4]}" || fail "go failed"
  [ "$(tmux display-message -p -t "=$TEAM-floor-2:" '#{pane_id}')" = "${floor[4]}" ] || fail "go did not focus it"

  # A placed-back agent's workspace has its sidebar too.
  has_sidebar "$TEAM-seniors" || fail "the Seniors workspace made on peek has no sidebar"
  # Its last agent stopped, a group workspace closes; one with more retiles.
  "$SCRIPTS/agent_cli.sh" stop "${floor[4]}" >/dev/null || fail "stop failed"
  gone "$TEAM-floor-2" || fail "the emptied Floor 2 workspace stayed (with its sidebar)"
  "$SCRIPTS/agent_cli.sh" stop "${floor[1]}" >/dev/null || fail "stop failed"
  has_sidebar "$TEAM-floor" && [ "$(group_panes "$TEAM-floor")" = 3 ] || fail "Floor lost its sidebar when an agent stopped"
}

gone() {
  ! tmux has-session -t "=$1" 2>/dev/null
}

test_agents_open_in_their_groups_workspaces
test_an_extra_agent
test_a_terminal_and_a_neovim
test_left_and_above
test_not_beside_the_sidebar
test_from_the_sidebar_without_a_terminal
test_a_minimized_group_is_expanded_first
test_new_panels_are_saved

printf 'New panel tests passed.\n'
