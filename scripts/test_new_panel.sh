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
  rm -rf "$TEST_ROOT"
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

# <pane>: beside the terminal, in its row (the default layout's agent row).
beside_terminal() {
  [ "$(field "$1" pane_top)" = "$(field "$(pane_id "$SESSION" terminal)" pane_top)" ] &&
    [ "$(field "$1" pane_left)" -gt "$(field "$(pane_id "$SESSION" terminal)" pane_left)" ]
}

started() {
  ls "$TEST_ROOT/started/$1".* >/dev/null 2>&1
}

SESSION=dev-panels
create_workspace "$SESSION"

test_an_extra_agent() {
  local pane
  pane="$("$NEW_PANEL" "$SESSION" agent work)" || fail "adding an agent failed"
  wait_until started claude || fail "the extra agent did not start"
  [ "$(tmux show-options -pqv -t "$pane" @devws_role)" = extra-agent ] &&
    [ "$(tmux show-options -pqv -t "$pane" @devws_agent)" = work ] || fail "the extra agent is not marked as one"
  beside_terminal "$pane" || fail "the extra agent is not beside the terminal"
  [ "$(sed -n 1p "$TEST_ROOT"/started/claude.*)" = "$TEST_ROOT/project" ] || fail "the extra agent did not start in the project"
  grep -qx -- --settings "$TEST_ROOT"/started/claude.* || fail "the extra agent started without devws's hooks"
  [ "$(pane_id "$SESSION" agent)" != "$pane" ] || fail "the extra agent took the main agent's role"
  ! "$NEW_PANEL" "$SESSION" agent nobody 2>/dev/null || fail "an unregistered agent was added"
}

test_a_terminal_and_a_neovim() {
  local terminal editor
  terminal="$("$NEW_PANEL" "$SESSION" terminal)" || fail "adding a terminal failed"
  [ "$(tmux show-options -pqv -t "$terminal" @devws_role)" = extra ] || fail "the terminal is not an extra one"
  editor="$("$NEW_PANEL" "$SESSION" nvim)" || fail "adding a Neovim failed"
  wait_until started nvim || fail "Neovim did not start"
  [ "$(tmux show-options -pqv -t "$editor" @devws_role)" = extra-editor ] || fail "Neovim is not marked as one"
  [ "$(sed -n 1p "$TEST_ROOT"/started/nvim.*)" = "$TEST_ROOT/project" ] || fail "Neovim did not open the project"
  ! grep -qx -- --listen "$TEST_ROOT"/started/nvim.* || fail "the extra Neovim took the agent's link"
  beside_terminal "$terminal" && beside_terminal "$editor" || fail "the new panels are not beside the terminal"
  # Each new panel takes half of the widest pane beside the terminal, not of
  # the terminal every time: after three, the terminal keeps a fair share.
  [ "$(field "$(pane_id "$SESSION" terminal)" pane_width)" -ge 18 ] ||
    fail "the terminal was squeezed to $(field "$(pane_id "$SESSION" terminal)" pane_width) columns"
  ! "$NEW_PANEL" "$SESSION" sideways 2>/dev/null || fail "an unknown kind was accepted"
}

test_a_minimized_group_is_expanded_first() {
  local pane
  "$SCRIPTS/minimize_panes.sh" "$SESSION" toggle agents
  [ -n "$(pane_id "$SESSION" minimized-terminal)" ] || fail "the group did not minimize"
  pane="$("$NEW_PANEL" "$SESSION" terminal)" || fail "adding to a minimized group failed"
  [ -z "$(pane_id "$SESSION" minimized-terminal)" ] || fail "the minimized group was not expanded"
  beside_terminal "$pane" || fail "the panel added to a minimized group is not beside the terminal"
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
test_a_minimized_group_is_expanded_first
test_new_panels_are_saved

printf 'New panel tests passed.\n'
