#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-minimize.XXXXXX")"
MINIMIZE="$ROOT/config/tmuxinator/minimize_panes.sh"
TOGGLE="$ROOT/config/tmuxinator/toggle_layout.sh"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-minimize-test-$$"
CLIENT_SOCKET="devws-minimize-client-$$"
# The same layout dev.yml opens a workspace with: picker, editor, agent, terminal.
DEFAULT_LAYOUT="12f9,200x50,0,0{25x50,0,0,58,174x50,26,0[174x24,26,0,57,174x25,26,25{86x25,26,25,59,87x25,113,25,60}]}"

cleanup() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server 2>/dev/null || true
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Workspace minimize test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
chmod +x "$TEST_ROOT/bin/tmux"
# The key bindings call ~/.tmuxinator/minimize_panes.sh, as installed.
ln -s "$ROOT/config/tmuxinator" "$TEST_ROOT/home/.tmuxinator"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX

# The devws part of tmux.conf: header format and bindings, loaded on their own
# because the rest of the file needs plugins this test does not install.
sed -n '/^# devws bindings go after the reset/,$p' "$ROOT/config/tmux/tmux.conf" \
  > "$TEST_ROOT/devws.conf"

tmux new-session -d -s setup -x 200 -y 50 'sleep 600'
tmux set -g mouse on
tmux source-file "$TEST_ROOT/devws.conf"

create_workspace() {
  local session="$1"
  local role pane

  tmux new-session -d -s "$session" -x 200 -y 50 'sleep 600'
  tmux set-option -t "$session" @devws_agent claude
  tmux set-option -w -t "$session:" pane-border-status top
  tmux select-pane -t "$session:" -T devws-picker
  for role in editor agent terminal; do
    pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' 'sleep 600')"
    tmux set-option -p -t "$pane" @devws_role "$role"
    tmux select-layout -t "$session:" tiled
  done
  tmux select-layout -t "$session:" "$DEFAULT_LAYOUT"
}

# Searches every window: while minimized, the real agent and terminal panes
# live in the session's hidden window.
pane_id() {
  tmux list-panes -s -t "$1" -F '#{pane_id} #{@devws_role}' |
    awk -v role="$2" '$2 == role { print $1; exit }'
}

pane_size() {
  tmux display-message -p -t "$1" "#{$2}"
}

pane_window() {
  tmux display-message -p -t "$1" '#{window_name}'
}

pane_pid() {
  tmux display-message -p -t "$1" '#{pane_pid}'
}

assert_minimized() {
  local session="$1"
  [[ "$(pane_window "$(pane_id "$session" agent)")" == devws-minimized ]] &&
    [[ "$(pane_window "$(pane_id "$session" terminal)")" == devws-minimized ]] ||
    fail "$session: the real agent and terminal were not moved to the hidden window"
  [[ "$(pane_window "$(pane_id "$session" minimized-agent)")" != devws-minimized ]] &&
    [[ "$(pane_window "$(pane_id "$session" minimized-terminal)")" != devws-minimized ]] ||
    fail "$session: placeholders are not in the workspace window"
}

assert_restored() {
  local session="$1"
  [[ -z "$(pane_id "$session" minimized-agent)" ]] &&
    ! tmux list-windows -t "$session" -F '#{window_name}' | grep -qx devws-minimized ||
    fail "$session: placeholders or the hidden window were left behind"
}

window_layout() {
  tmux display-message -p -t "$1:" '#{window_layout}'
}

test_stacked_minimize_and_restore() {
  local session="dev-stacked"
  local before editor_height agent_pid
  create_workspace "$session"
  before="$(window_layout "$session")"
  editor_height="$(pane_size "$(pane_id "$session" editor)" pane_height)"
  agent_pid="$(pane_pid "$(pane_id "$session" agent)")"

  "$MINIMIZE" "$session" || fail "minimize exited non-zero"
  assert_minimized "$session"
  [[ "$(pane_size "$(pane_id "$session" minimized-agent)" pane_height)" == 1 ]] &&
    [[ "$(pane_size "$(pane_id "$session" minimized-terminal)" pane_height)" == 1 ]] ||
    fail "the bars are not one line high"
  (( $(pane_size "$(pane_id "$session" editor)" pane_height) > editor_height )) ||
    fail "the editor did not take the freed space"

  "$MINIMIZE" "$session" || fail "restore exited non-zero"
  assert_restored "$session"
  [[ "$(window_layout "$session")" == "$before" ]] ||
    fail "restore did not bring back the previous sizes"
  [[ "$(pane_pid "$(pane_id "$session" agent)")" == "$agent_pid" ]] ||
    fail "the agent process did not survive minimizing"
}

# An agent run by hand in the terminal, the agent pane now a plain shell with
# no devws role: the terminal leads the group, the shell goes along as an
# extra terminal, and both come back.
test_minimize_without_an_agent_pane() {
  local session="dev-no-agent"
  local before shell terminal_pid shell_pid
  create_workspace "$session"
  shell="$(pane_id "$session" agent)"
  tmux set-option -p -u -t "$shell" @devws_role
  before="$(window_layout "$session")"
  terminal_pid="$(pane_pid "$(pane_id "$session" terminal)")"
  shell_pid="$(pane_pid "$shell")"

  "$MINIMIZE" "$session" toggle agents || fail "minimize without an agent pane exited non-zero"
  [[ "$(pane_window "$(pane_id "$session" terminal)")" == devws-minimized ]] &&
    [[ "$(pane_window "$shell")" == devws-minimized ]] ||
    fail "the terminal and the shell were not moved to the hidden window"
  [[ -n "$(pane_id "$session" minimized-extra)" ]] || fail "the shell's placeholder is not an extra terminal's"
  [[ "$(pane_size "$(pane_id "$session" minimized-terminal)" pane_height)" == 1 ]] ||
    fail "the terminal bar is not one line high"

  "$MINIMIZE" "$session" toggle agents || fail "restore without an agent pane exited non-zero"
  [[ -z "$(tmux list-panes -s -t "$session" -F '#{@devws_role}' | grep '^minimized-')" ]] &&
    ! tmux list-windows -t "$session" -F '#{window_name}' | grep -qx devws-minimized ||
    fail "placeholders or the hidden window were left behind"
  [[ "$(window_layout "$session")" == "$before" ]] || fail "restore did not bring back the sizes"
  [[ "$(pane_pid "$(pane_id "$session" terminal)")" == "$terminal_pid" ]] &&
    [[ "$(pane_pid "$shell")" == "$shell_pid" ]] ||
    fail "the terminal's or the shell's process did not survive minimizing"
}

# Panels New panel adds beside the terminal — an extra agent and a Neovim —
# minimize with the group, each with its own label, and come back as they were.
test_new_panels_minimize_with_the_group() {
  local session="dev-panels" agent_panel editor_panel agent_pid editor_pid
  create_workspace "$session"
  agent_panel="$(tmux split-window -h -t "$(pane_id "$session" terminal)" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$agent_panel" @devws_role extra-agent
  tmux set-option -p -t "$agent_panel" @devws_agent work
  editor_panel="$(tmux split-window -h -t "$agent_panel" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$editor_panel" @devws_role extra-editor
  agent_pid="$(pane_pid "$agent_panel")"
  editor_pid="$(pane_pid "$editor_panel")"

  "$MINIMIZE" "$session" toggle agents || fail "minimize with new panels exited non-zero"
  [[ "$(pane_window "$agent_panel")" == devws-minimized ]] &&
    [[ "$(pane_window "$editor_panel")" == devws-minimized ]] ||
    fail "the new panels did not minimize with the group"
  tmux display-message -p -t "$(pane_id "$session" minimized-extra-agent)" '#{pane_start_command}' |
    grep -q 'Agent.*·.*work' || fail "the extra agent's placeholder is not labelled with its agent"
  tmux display-message -p -t "$(pane_id "$session" minimized-extra-editor)" '#{pane_start_command}' |
    grep -q 'Neovim' || fail "the Neovim placeholder is not labelled Neovim"

  "$MINIMIZE" "$session" toggle agents || fail "restore with new panels exited non-zero"
  [[ -z "$(tmux list-panes -s -t "$session" -F '#{@devws_role}' | grep '^minimized-')" ]] ||
    fail "placeholders were left behind"
  [[ "$(pane_pid "$agent_panel")" == "$agent_pid" ]] && [[ "$(pane_pid "$editor_panel")" == "$editor_pid" ]] ||
    fail "a new panel was restarted"
}

# A pane split off the editor by hand (C-b %, e.g. a Markdown viewer) has no
# devws role but is not the agent group's: minimizing leaves it in place, in
# both layouts.
test_minimize_leaves_a_pane_beside_the_editor() {
  local session viewer viewer_pid
  for session in dev-viewer-stacked dev-viewer-side; do
    create_workspace "$session"
    [ "$session" = dev-viewer-side ] && "$TOGGLE" "$session"
    viewer="$(tmux split-window -h -t "$(pane_id "$session" editor)" -P -F '#{pane_id}' 'sleep 600')"
    viewer_pid="$(pane_pid "$viewer")"

    "$MINIMIZE" "$session" toggle agents || fail "$session: minimize exited non-zero"
    [[ "$(pane_window "$viewer")" != devws-minimized ]] ||
      fail "$session: the pane beside the editor was minimized with the agent group"
    [[ -z "$(pane_id "$session" minimized-extra)" ]] ||
      fail "$session: the pane beside the editor got a placeholder"
    "$MINIMIZE" "$session" toggle agents || fail "$session: restore exited non-zero"
    [[ "$(pane_pid "$viewer")" == "$viewer_pid" ]] || fail "$session: the pane beside the editor was restarted"
  done
}

# New panel opened beside or below the editor (not in the agent group's area)
# stays where it is when the group minimizes, whatever it runs.
test_new_panels_beside_the_editor_stay() {
  local session panel panel_pid
  for session in dev-near-stacked dev-near-side; do
    create_workspace "$session"
    [ "$session" = dev-near-side ] && "$TOGGLE" "$session"
    panel="$(tmux split-window -v -t "$(pane_id "$session" editor)" -P -F '#{pane_id}' 'sleep 600')"
    tmux set-option -p -t "$panel" @devws_role extra-editor
    panel_pid="$(pane_pid "$panel")"
    "$MINIMIZE" "$session" toggle agents || fail "$session: minimize exited non-zero"
    [[ "$(pane_window "$panel")" != devws-minimized ]] ||
      fail "$session: a Neovim below the editor was minimized with the agent group"
    "$MINIMIZE" "$session" toggle agents || fail "$session: restore exited non-zero"
    [[ "$(pane_pid "$panel")" == "$panel_pid" ]] || fail "$session: the Neovim below the editor was restarted"
  done
}

# Codex names its pane after the task, which can hold a "|" ("Improve the
# guide | Parla"): it must not shift the pane's fields, or the layout reads as
# stacked and a panel beside the editor is hidden with the group.
test_a_pane_title_with_a_bar() {
  local session="dev-bar" viewer viewer_pid
  create_workspace "$session"
  "$TOGGLE" "$session"
  tmux select-pane -t "$(pane_id "$session" agent)" -T 'Improve the guide | Parla'
  viewer="$(tmux split-window -h -t "$(pane_id "$session" editor)" -P -F '#{pane_id}' 'sleep 600')"
  viewer_pid="$(pane_pid "$viewer")"

  "$MINIMIZE" "$session" toggle agents || fail "minimize with a | in a title exited non-zero"
  [[ "$(pane_window "$viewer")" != devws-minimized ]] ||
    fail "a | in the agent's title hid the panel beside the editor"
  [[ "$(tmux show-options -wqv -t "$session:" @devws_restore_agents)" == x:* ]] ||
    fail "a | in the agent's title made the side layout read as stacked"
  "$MINIMIZE" "$session" toggle agents || fail "restore with a | in a title exited non-zero"
  [[ "$(pane_pid "$viewer")" == "$viewer_pid" ]] || fail "the panel beside the editor was restarted"
}

test_side_minimize_and_restore() {
  local session="dev-side"
  local before
  create_workspace "$session"
  "$TOGGLE" "$session"
  before="$(window_layout "$session")"

  "$MINIMIZE" "$session" || fail "minimize in side layout exited non-zero"
  assert_minimized "$session"
  [[ "$(pane_size "$(pane_id "$session" minimized-agent)" pane_width)" == 9 ]] &&
    [[ "$(pane_size "$(pane_id "$session" minimized-terminal)" pane_width)" == 9 ]] ||
    fail "the right column did not shrink to a strip"

  "$MINIMIZE" "$session"
  assert_restored "$session"
  [[ "$(window_layout "$session")" == "$before" ]] ||
    fail "restore in side layout did not bring back the previous sizes"
}

test_restore_falls_back_when_panes_changed() {
  local session="dev-changed"
  create_workspace "$session"
  "$MINIMIZE" "$session"
  tmux split-window -d -h -t "$(pane_id "$session" minimized-terminal)" 'sleep 600'

  "$MINIMIZE" "$session" || fail "restore after adding a pane exited non-zero"
  (( $(pane_size "$(pane_id "$session" agent)" pane_height) >= 20 )) ||
    fail "restore did not fall back to the default proportions"
}

test_focus_leaves_minimized_pane() {
  local session="dev-focus"
  create_workspace "$session"
  tmux select-pane -t "$(pane_id "$session" agent)"

  "$MINIMIZE" "$session"
  [[ "$(tmux display-message -p -t "$session:" '#{pane_id}')" == "$(pane_id "$session" editor)" ]] ||
    fail "focus stayed on a minimized pane"
}

test_layout_toggle_expands_minimized_panes() {
  local session="dev-toggle"
  create_workspace "$session"
  "$MINIMIZE" "$session"

  "$TOGGLE" "$session" || fail "layout toggle while minimized exited non-zero"
  (( $(pane_size "$(pane_id "$session" agent)" pane_width) > 9 )) &&
    (( $(pane_size "$(pane_id "$session" agent)" pane_height) > 1 )) ||
    fail "layout toggle left the panes minimized"
}

test_hidden_window_stays_in_its_session() {
  local session="dev-older"
  create_workspace "$session"
  # A newer session becomes tmux's "current" one when no client is attached.
  create_workspace "dev-newer"

  "$MINIMIZE" "$session"
  tmux list-windows -t "$session" -F '#{window_name}' | grep -qx devws-minimized ||
    fail "the hidden window was created in another session"
  [[ -n "$(pane_id "$session" agent)" ]] ||
    fail "the minimized agent left its own session"
}

# A real client, running inside a second tmux server, so headers are drawn
# and mouse events go through tmux's own key bindings.
attach_client() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t $1"
  sleep 1
}

client_screen() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" capture-pane -p -t client
}

# SGR mouse report: button;column;row, M for press and m for release.
send_mouse() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" send-keys -t client -l $'\e[<'"$1"'M'
}

release_mouse() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" send-keys -t client -l $'\e[<'"$1"'m'
}

is_minimized_now() {
  [[ -n "$(pane_id "$1" minimized-agent)" ]]
}

wait_until() {
  local attempt
  for attempt in 1 2 3 4 5 6 7 8 9 10; do
    "$@" && return 0
    sleep 0.3
  done
  return 1
}

click_header() {
  local label="$1"
  local row
  row="$(grep -n "$label" <<< "$(client_screen)" | head -1 | cut -d: -f1)"
  [[ -n "$row" ]] || fail "no header showing '$label'"
  send_mouse "0;60;$row"
  release_mouse "0;60;$row"
}

test_headers_and_mouse() {
  local session="dev-mouse"
  local row screen height_before
  create_workspace "$session"
  attach_client "$session"

  screen="$(client_screen)"
  grep -q '▾ 󰚩 Agent · claude' <<< "$screen" || fail "the agent header is missing"
  grep -q $'▾ \uF489 Terminal' <<< "$screen" || fail "the terminal header is missing"

  click_header 'Agent · claude'
  wait_until is_minimized_now "$session" || fail "clicking the agent header did not minimize"
  sleep 0.5
  screen="$(client_screen)"
  grep -q '▸ Agent─' <<< "$screen" && grep -q '▸ Term─' <<< "$screen" ||
    fail "the headers do not show the short minimized labels"
  grep -q ' Agent · claude · minimized' <<< "$screen" ||
    fail "the agent placeholder text is not shown"
  grep -q ' Terminal · minimized' <<< "$screen" ||
    fail "the terminal placeholder text is not shown"

  # Keys reaching a placeholder (Esc typed while it has focus, focus events)
  # must not be echoed into the bar.
  tmux send-keys -t "$(pane_id "$session" minimized-agent)" Escape x
  sleep 0.5
  ! grep -qE '\^\[|minimized.*x$' <<< "$(client_screen)" ||
    fail "a placeholder echoed typed keys"

  click_header '▸ Agent'
  wait_until eval '! is_minimized_now "$session"' ||
    fail "clicking the minimized header did not restore"

  # Dragging the header resizes, and must not toggle.
  sleep 0.5
  height_before="$(pane_size "$(pane_id "$session" agent)" pane_height)"
  row="$(grep -n 'Agent · claude' <<< "$(client_screen)" | head -1 | cut -d: -f1)"
  send_mouse "0;60;$row"
  sleep 0.2
  send_mouse "32;60;$((row - 3))"
  sleep 0.2
  release_mouse "0;60;$((row - 3))"
  sleep 1
  (( $(pane_size "$(pane_id "$session" agent)" pane_height) == height_before + 3 )) &&
    ! is_minimized_now "$session" ||
    fail "dragging the header did not just resize it"

  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server
}

test_stacked_minimize_and_restore
test_minimize_without_an_agent_pane
test_minimize_leaves_a_pane_beside_the_editor
test_new_panels_minimize_with_the_group
test_new_panels_beside_the_editor_stay
test_a_pane_title_with_a_bar
test_side_minimize_and_restore
test_restore_falls_back_when_panes_changed
test_focus_leaves_minimized_pane
test_layout_toggle_expands_minimized_panes
test_hidden_window_stays_in_its_session
test_headers_and_mouse

printf 'Workspace minimize tests passed.\n'
