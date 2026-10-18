#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-layout.XXXXXX")"
TOGGLE="$ROOT/config/tmuxinator/toggle_layout.sh"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-layout-test-$$"
# The same layout dev.yml opens a workspace with: picker, editor, agent, terminal.
DEFAULT_LAYOUT="12f9,200x50,0,0{25x50,0,0,58,174x50,26,0[174x24,26,0,57,174x25,26,25{86x25,26,25,59,87x25,113,25,60}]}"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Workspace layout test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the ones toggle_layout.sh and save_state.sh make,
# goes to an isolated server so the test never touches the user's sessions.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
chmod +x "$TEST_ROOT/bin/tmux"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX

create_workspace() {
  local session="$1"
  local role pane

  tmux new-session -d -s "$session" -x 200 -y 50 'sleep 600'
  tmux select-pane -t "$session:" -T devws-picker
  for role in editor agent terminal; do
    pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' 'sleep 600')"
    tmux set-option -p -t "$pane" @devws_role "$role"
    tmux select-layout -t "$session:" tiled
  done
  tmux select-layout -t "$session:" "$DEFAULT_LAYOUT"
}

pane_id() {
  tmux list-panes -t "$1:" -F '#{pane_id} #{@devws_role}' |
    awk -v role="$2" '$2 == role { print $1; exit }'
}

# Prints "left top width height" for a pane.
geometry() {
  tmux display-message -p -t "$1" '#{pane_left} #{pane_top} #{pane_width} #{pane_height}'
}

window_height() {
  tmux display-message -p -t "$1:" '#{window_height}'
}

assert_side_layout() {
  local session="$1"
  local editor_left editor_top editor_width editor_height
  local agent_left agent_top agent_width agent_height
  local terminal_left terminal_top terminal_width terminal_height

  read -r editor_left editor_top editor_width editor_height < <(geometry "$(pane_id "$session" editor)")
  read -r agent_left agent_top agent_width agent_height < <(geometry "$(pane_id "$session" agent)")
  read -r terminal_left terminal_top terminal_width terminal_height < <(geometry "$(pane_id "$session" terminal)")

  [[ "$editor_height" == "$(window_height "$session")" ]] ||
    fail "$session: editor is not full height in the side layout"
  (( agent_left > editor_left + editor_width - 1 )) ||
    fail "$session: agent is not right of the editor"
  [[ "$agent_top" == "$editor_top" ]] ||
    fail "$session: agent is not at the top of the right column"
  [[ "$terminal_left" == "$agent_left" ]] && (( terminal_top > agent_top + agent_height - 1 )) ||
    fail "$session: terminal is not below the agent"
}

assert_stacked_layout() {
  local session="$1"
  local editor_left editor_top editor_width editor_height
  local agent_left agent_top agent_width agent_height
  local terminal_left terminal_top terminal_width terminal_height

  read -r editor_left editor_top editor_width editor_height < <(geometry "$(pane_id "$session" editor)")
  read -r agent_left agent_top agent_width agent_height < <(geometry "$(pane_id "$session" agent)")
  read -r terminal_left terminal_top terminal_width terminal_height < <(geometry "$(pane_id "$session" terminal)")

  (( agent_top > editor_top + editor_height - 1 )) ||
    fail "$session: agent is not below the editor"
  [[ "$agent_left" == "$editor_left" ]] ||
    fail "$session: agent does not start at the editor's left edge"
  [[ "$terminal_top" == "$agent_top" ]] && (( terminal_left > agent_left + agent_width - 1 )) ||
    fail "$session: terminal is not right of the agent"
}

pane_commands() {
  tmux list-panes -t "$1:" -F '#{pane_id} #{pane_pid}' | sort
}

test_toggles_both_ways() {
  local session="dev-toggle"
  local before
  create_workspace "$session"
  before="$(pane_commands "$session")"

  "$TOGGLE" "$session" || fail "toggle to side layout exited non-zero"
  assert_side_layout "$session"

  "$TOGGLE" "$session" || fail "toggle back to stacked layout exited non-zero"
  assert_stacked_layout "$session"

  [[ "$(pane_commands "$session")" == "$before" ]] ||
    fail "toggling restarted or replaced panes"
}

test_keeps_focus() {
  local session="dev-focus"
  local editor
  create_workspace "$session"
  editor="$(pane_id "$session" editor)"
  tmux select-pane -t "$editor"

  "$TOGGLE" "$session"
  [[ "$(tmux display-message -p -t "$session:" '#{pane_id}')" == "$editor" ]] ||
    fail "toggling moved focus away from the active pane"
}

test_picker_closed_with_extra_terminal() {
  local session="dev-extra"
  local extra
  create_workspace "$session"
  tmux kill-pane -t "$(tmux list-panes -t "$session:" -F '#{pane_id} #{pane_title}' |
    awk '$2 == "devws-picker" { print $1 }')"
  extra="$(tmux split-window -h -t "$(pane_id "$session" terminal)" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$extra" @devws_role extra

  "$TOGGLE" "$session" || fail "toggle with extra terminal exited non-zero"
  assert_side_layout "$session"
  read -r _ extra_top _ _ < <(geometry "$extra")
  read -r _ terminal_top _ _ < <(geometry "$(pane_id "$session" terminal)")
  [[ "$extra_top" == "$terminal_top" ]] ||
    fail "extra terminal did not move beside the terminal"

  "$TOGGLE" "$session" || fail "toggle back with extra terminal exited non-zero"
  assert_stacked_layout "$session"
}

# New panel's agent and Neovim travel beside the terminal both ways, like
# extra terminals.
test_new_panels_move_with_the_terminal() {
  local session="dev-panels" agent_panel editor_panel terminal_top panel_top
  create_workspace "$session"
  agent_panel="$(tmux split-window -h -t "$(pane_id "$session" terminal)" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$agent_panel" @devws_role extra-agent
  editor_panel="$(tmux split-window -h -t "$agent_panel" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$editor_panel" @devws_role extra-editor

  for direction in side stacked; do
    "$TOGGLE" "$session" || fail "toggle to $direction with new panels exited non-zero"
    read -r _ terminal_top _ _ < <(geometry "$(pane_id "$session" terminal)")
    for panel in "$agent_panel" "$editor_panel"; do
      read -r _ panel_top _ _ < <(geometry "$panel")
      [[ "$panel_top" == "$terminal_top" ]] || fail "$direction: a new panel did not move beside the terminal"
    done
  done
}

# An agent started by hand in the terminal, its agent pane now a plain shell
# with no devws role: the terminal leads, and the shell travels beside it.
test_toggles_without_an_agent_pane() {
  local session="dev-missing"
  local shell before
  local editor_left editor_top editor_width editor_height
  local terminal_left terminal_top terminal_width terminal_height
  local shell_left shell_top shell_width shell_height
  create_workspace "$session"
  shell="$(pane_id "$session" agent)"
  tmux set-option -p -u -t "$shell" @devws_role
  before="$(pane_commands "$session")"

  "$TOGGLE" "$session" || fail "toggle without an agent pane exited non-zero"
  read -r editor_left editor_top editor_width editor_height < <(geometry "$(pane_id "$session" editor)")
  read -r terminal_left terminal_top terminal_width terminal_height < <(geometry "$(pane_id "$session" terminal)")
  read -r shell_left shell_top shell_width shell_height < <(geometry "$shell")
  [[ "$editor_height" == "$(window_height "$session")" ]] ||
    fail "without an agent, the editor is not full height in the side layout"
  [[ "$terminal_top" == "$editor_top" ]] && (( terminal_left > editor_left + editor_width - 1 )) ||
    fail "without an agent, the terminal is not right of the editor"
  [[ "$shell_top" == "$terminal_top" ]] || fail "the shell did not travel beside the terminal"

  "$TOGGLE" "$session" || fail "toggle back without an agent pane exited non-zero"
  read -r editor_left editor_top editor_width editor_height < <(geometry "$(pane_id "$session" editor)")
  read -r terminal_left terminal_top terminal_width terminal_height < <(geometry "$(pane_id "$session" terminal)")
  (( terminal_top > editor_top + editor_height - 1 )) && [[ "$terminal_left" == "$editor_left" ]] ||
    fail "without an agent, the terminal is not below the editor"
  [[ "$(pane_commands "$session")" == "$before" ]] || fail "toggling restarted or replaced panes"
}

test_toggles_both_ways
test_keeps_focus
test_picker_closed_with_extra_terminal
test_toggles_without_an_agent_pane
test_new_panels_move_with_the_terminal

printf 'Workspace layout tests passed.\n'
