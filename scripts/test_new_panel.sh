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

test_an_extra_agent
test_a_terminal_and_a_neovim
test_left_and_above
test_not_beside_the_sidebar
test_a_minimized_group_is_expanded_first
test_new_panels_are_saved

printf 'New panel tests passed.\n'
