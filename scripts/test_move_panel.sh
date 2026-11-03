#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-move-panel.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-move-panel-test-$$"
# The same layout dev.yml opens a workspace with: picker, editor, agent, terminal.
DEFAULT_LAYOUT="12f9,200x50,0,0{25x50,0,0,58,174x50,26,0[174x24,26,0,57,174x25,26,25{86x25,26,25,59,87x25,113,25,60}]}"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT" 2>/dev/null || { sleep 0.5; rm -rf "$TEST_ROOT"; }
}
trap cleanup EXIT

fail() {
  printf 'Move panel test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call goes to an isolated server; ~/.tmuxinator holds the real scripts.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator" "$TEST_ROOT/project"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
chmod +x "$TEST_ROOT/bin/tmux"
for script in "$SCRIPTS"/*.sh "$SCRIPTS"/*.json "$SCRIPTS"/*.md; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT"
unset TMUX
SCRIPTS="$HOME/.tmuxinator"
MOVE_PANEL="$SCRIPTS/move_panel.sh"

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

picker_of() {
  tmux list-panes -t "$1:" -F '#{pane_id} #{pane_title}' | awk '$2 == "devws-picker" { print $1 }'
}

# Where a pane sits and how big it is.
geometry() {
  tmux display-message -p -t "$1" '#{pane_left} #{pane_top} #{pane_width} #{pane_height}'
}

# Every pane's place, in pane order: changes when anything moves.
layout_of() {
  tmux list-panes -t "$1:" -F '#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height}'
}

focused() {
  tmux display-message -p -t "$1:" '#{pane_id}'
}

# <session> <pane> <direction>: the pane trades places with its neighbour
# <other>, each taking the other's place and size, and keeps the focus.
#   moves_to <session> <pane> <direction> <other>
moves_to() {
  local session="$1" pane="$2" direction="$3" other="$4" from to
  from="$(geometry "$pane")"
  to="$(geometry "$other")"
  tmux select-pane -t "$pane"
  "$MOVE_PANEL" "$session" "$direction" || fail "moving $direction failed"
  [ "$(geometry "$pane")" = "$to" ] || fail "the pane did not take its neighbour's place moving $direction"
  [ "$(geometry "$other")" = "$from" ] || fail "the neighbour did not take the pane's place moving $direction"
  [ "$(focused "$session")" = "$pane" ] || fail "the focus did not stay on the moved pane"
}

# <session> <pane> <direction>: nothing moves and the reason is given.
#   stays <session> <pane> <direction> <message>
stays() {
  local session="$1" pane="$2" direction="$3" message="$4" before error
  tmux select-pane -t "$pane"
  before="$(layout_of "$session")"
  if error="$("$MOVE_PANEL" "$session" "$direction" 2>&1)"; then
    fail "moving $direction succeeded where it should not"
  fi
  [[ "$error" == *"$message"* ]] || fail "moving $direction said: $error"
  [ "$(layout_of "$session")" = "$before" ] || fail "something moved going $direction"
}

SESSION=dev-moves
create_workspace "$SESSION"

# Panels trade places: the layout's sizes stay, the roles go with the panes.
test_left_and_right() {
  local agent terminal
  agent="$(pane_id "$SESSION" agent)"
  terminal="$(pane_id "$SESSION" terminal)"
  moves_to "$SESSION" "$terminal" left "$agent"
  [ "$(pane_id "$SESSION" terminal)" = "$terminal" ] || fail "the terminal lost its role"
  moves_to "$SESSION" "$terminal" right "$agent"
}

test_below_and_above() {
  local editor below
  editor="$(pane_id "$SESSION" editor)"
  tmux select-pane -t "$editor"
  below="$(tmux display-message -p -t "$SESSION:.{down-of}" '#{pane_id}')"
  moves_to "$SESSION" "$editor" below "$below"
  moves_to "$SESSION" "$editor" above "$below"
}

# tmux's own neighbours wrap around the window; a move does not.
test_no_panel_on_that_side() {
  stays "$SESSION" "$(pane_id "$SESSION" editor)" above "no panel above"
  stays "$SESSION" "$(pane_id "$SESSION" terminal)" right "no panel to the right"
  stays "$SESSION" "$(pane_id "$SESSION" terminal)" below "no panel below"
  ! "$MOVE_PANEL" "$SESSION" diagonal 2>/dev/null || fail "an unknown direction was accepted"
}

test_the_sidebar_stays() {
  stays "$SESSION" "$(pane_id "$SESSION" agent)" left "sidebar"
  stays "$SESSION" "$(picker_of "$SESSION")" right "sidebar"
}

# Minimizing moves the focus to the editor; a move expands the group first.
test_a_minimized_group_is_expanded_first() {
  local editor
  "$SCRIPTS/minimize_panes.sh" "$SESSION" toggle agents
  [ -n "$(pane_id "$SESSION" minimized-terminal)" ] || fail "the group did not minimize"
  editor="$(pane_id "$SESSION" editor)"
  tmux select-pane -t "$editor"
  "$MOVE_PANEL" "$SESSION" below || fail "moving in a minimized workspace failed"
  [ -z "$(pane_id "$SESSION" minimized-terminal)" ] || fail "the minimized group was not expanded"
  [ "$(tmux display-message -p -t "$editor" '#{pane_top}')" -gt 0 ] || fail "the editor did not move below"
  "$MOVE_PANEL" "$SESSION" above || fail "moving the editor back failed"
}

# The saved workspace has the panes in their new order.
test_moves_are_saved() {
  local saved="$HOME/.devws-state/sessions.tsv" live stored
  tmux select-pane -t "$(pane_id "$SESSION" terminal)"
  "$MOVE_PANEL" "$SESSION" left || fail "moving the terminal failed"
  [ -s "$saved" ] || fail "the workspace was not saved"
  live="$(tmux list-panes -t "$SESSION:" -F '#{pane_index} #{@devws_role}' | awk '$2 != "" { print }')"
  stored="$(awk -F '\t' -v s="$SESSION" '$1 == "P" && $2 == s && $4 != "picker" { print $3, $4 }' "$saved")"
  [ "$live" = "$stored" ] || fail "the saved order is not the new one: $stored"
}

test_the_menu_and_keys() {
  local items
  items="$(TMUX_PANE="$(pane_id "$SESSION" editor)" TMUX="fake,1,0" "$SCRIPTS/devws_menu.sh" --items layout)"
  grep -q $'^move-panel\t.*Move panel.*C-b S-' <<<"$items" || fail "the menu has no Move panel"
  for binding in "S-Right.*move_panel.sh.* right" "S-Left.*move_panel.sh.* left" \
    "S-Up.*move_panel.sh.* above" "S-Down.*move_panel.sh.* below"; do
    grep -q "^bind -r $binding" "$ROOT/config/tmux/tmux.conf" || fail "tmux.conf does not bind $binding"
  done
}

test_left_and_right
test_below_and_above
test_no_panel_on_that_side
test_the_sidebar_stays
test_a_minimized_group_is_expanded_first
test_moves_are_saved
test_the_menu_and_keys

printf 'Move panel tests passed.\n'
