#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-reload.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-reload-test-$$"
# The same layout dev.yml opens a workspace with: picker, editor, agent, terminal.
DEFAULT_LAYOUT="12f9,200x50,0,0{25x50,0,0,58,174x50,26,0[174x24,26,0,57,174x25,26,25{86x25,26,25,59,87x25,113,25,60}]}"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Reload test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server.
# ~/.tmuxinator holds the real scripts, but the sidebar is a stub that only
# waits: the reload must restart it, not run fzf.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
chmod +x "$TEST_ROOT/bin/tmux"
for script in "$SCRIPTS"/*.sh; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
rm "$TEST_ROOT/home/.tmuxinator/session_picker.sh"
printf '#!/usr/bin/env bash\nexec sleep 600\n' > "$TEST_ROOT/home/.tmuxinator/session_picker.sh"
chmod +x "$TEST_ROOT/home/.tmuxinator/session_picker.sh"
# The tmux.conf the reload sources: an option to see it applied.
printf 'set -g @reload_marker before\n' > "$TEST_ROOT/home/.tmux.conf"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX
SCRIPTS="$HOME/.tmuxinator"
RELOAD="$SCRIPTS/reload_devws.sh"

wait_until() {
  local attempt
  for attempt in $(seq 1 30); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

pane_pid() {
  tmux display-message -p -t "$1" '#{pane_pid}'
}

# Searches every window: placeholders' real panes live in a hidden one.
pane_id() {
  tmux list-panes -s -t "$1" -F '#{pane_id} #{@devws_role}' |
    awk -v role="$2" '$2 == role { print $1; exit }'
}

create_workspace() {
  local session="$1" role pane
  tmux new-session -d -s "$session" -x 200 -y 50 "$SCRIPTS/session_picker.sh"
  tmux set-option -t "$session" @devws_agent claude
  tmux select-pane -t "$session:" -T devws-picker
  for role in editor agent terminal; do
    pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' 'sleep 600')"
    tmux set-option -p -t "$pane" @devws_role "$role"
    tmux select-layout -t "$session:" tiled
  done
  tmux select-layout -t "$session:" "$DEFAULT_LAYOUT"
}

picker_of() {
  tmux list-panes -t "$1:" -F '#{pane_id} #{pane_title}' | awk '$2 == "devws-picker" { print $1; exit }'
}

process_gone() {
  ! kill -0 "$1" 2>/dev/null
}

test_no_server_is_nothing_to_reload() {
  local output
  output="$("$RELOAD")" || fail "reload without a server exited non-zero"
  [ "$output" = "devws: no tmux server, nothing to reload" ] || fail "unexpected message without a server: $output"
}

test_reload_restarts_devws_scripts_only() {
  local picker editor agent placeholder other old_ticker output
  local picker_pid editor_pid agent_pid placeholder_pid other_pid real_terminal
  create_workspace dev-work
  "$SCRIPTS/minimize_panes.sh" dev-work toggle agents || fail "could not minimize the agent group"
  "$SCRIPTS/agent_status.sh" dev-work working
  # The spinner an older devws ran per working agent, and the frame it left.
  sleep 600 &
  old_ticker=$!
  disown "$old_ticker"
  tmux set-option -t "=dev-work:" @devws_agent_ticker "$old_ticker"
  tmux set-option -t "=dev-work:" @devws_agent_frame ⠙
  # Not a workspace: left alone, even with a sidebar's title.
  tmux new-session -d -s other -x 200 -y 50 'sleep 600'
  tmux select-pane -t other: -T devws-picker
  other="$(tmux display-message -p -t other: '#{pane_id}')"

  picker="$(picker_of dev-work)"
  editor="$(pane_id dev-work editor)"
  agent="$(pane_id dev-work agent)"
  placeholder="$(pane_id dev-work minimized-terminal)"
  real_terminal="$(tmux show-options -pqv -t "$placeholder" @devws_hidden_pane)"
  picker_pid="$(pane_pid "$picker")"
  editor_pid="$(pane_pid "$editor")"
  agent_pid="$(pane_pid "$agent")"
  placeholder_pid="$(pane_pid "$placeholder")"
  other_pid="$(pane_pid "$other")"
  printf 'set -g @reload_marker after\n' > "$HOME/.tmux.conf"

  output="$("$RELOAD")" || fail "reload exited non-zero"
  [ "$output" = "devws: reloaded tmux.conf, 1 sidebar(s), 2 placeholder(s)" ] ||
    fail "unexpected summary: $output"

  [ "$(tmux show-option -gqv @reload_marker)" = after ] || fail "tmux.conf was not reloaded"
  [ "$(pane_pid "$picker")" != "$picker_pid" ] || fail "the sidebar was not restarted"
  [ "$(pane_pid "$placeholder")" != "$placeholder_pid" ] || fail "the placeholder was not restarted"
  [ "$(pane_pid "$editor")" = "$editor_pid" ] && [ "$(pane_pid "$agent")" = "$agent_pid" ] ||
    fail "the editor or the agent was restarted"
  [ "$(pane_pid "$other")" = "$other_pid" ] || fail "a pane outside the workspaces was restarted"

  # The placeholder is still the terminal's, with its label, and restores.
  [ "$(tmux show-options -pqv -t "$placeholder" @devws_role)" = minimized-terminal ] &&
    [ "$(tmux show-options -pqv -t "$placeholder" @devws_hidden_pane)" = "$real_terminal" ] ||
    fail "the placeholder lost its role or its pane"
  tmux display-message -p -t "$placeholder" '#{pane_start_command}' | grep -q 'Terminal' ||
    fail "the placeholder lost its label"

  wait_until process_gone "$old_ticker" || fail "the old spinner still runs"
  [ -z "$(tmux show-option -qv -t "=dev-work:" @devws_agent_ticker)" ] &&
    [ -z "$(tmux show-option -qv -t "=dev-work:" @devws_agent_frame)" ] ||
    fail "the old spinner's options are left on the workspace"
  [ "$(tmux show-option -qv -t "=dev-work:" @devws_agent_state)" = working ] ||
    fail "the reload changed the agent's state"

  "$SCRIPTS/minimize_panes.sh" dev-work toggle agents || fail "restoring after the reload failed"
  [ -z "$(pane_id dev-work minimized-terminal)" ] && [ "$(pane_pid "$agent")" = "$agent_pid" ] ||
    fail "the agent group did not come back after the reload"
  "$SCRIPTS/agent_status.sh" dev-work idle
}

# A server started with an older tmux.conf keeps its hooks through a reload
# unless tmux.conf removes them: the old after-new-window rename prompt
# blocked tmuxinator's new-window, leaving new workspaces as one bare pane.
test_reloading_removes_the_old_rename_hook() {
  tmux new-session -d -s hooks 'sleep 600'
  tmux set-hook -g after-new-window 'command-prompt -I "#{window_name}" "rename-window %%"'
  grep -E '^set-hook ' "$ROOT/config/tmux/tmux.conf" > "$TEST_ROOT/hooks.conf"
  tmux source-file "$TEST_ROOT/hooks.conf"
  [ -z "$(tmux show-hooks -g after-new-window 2>/dev/null | grep command-prompt)" ] ||
    fail "reloading tmux.conf kept the old rename prompt hook"
  tmux kill-session -t hooks
}

test_no_server_is_nothing_to_reload
test_reloading_removes_the_old_rename_hook
test_reload_restarts_devws_scripts_only

printf 'Reload tests passed.\n'
