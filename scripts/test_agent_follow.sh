#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-follow.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
REAL_NVIM="$(command -v nvim)"
SOCKET="devws-follow-test-$$"
# Unix socket paths are limited to ~100 characters, and TMPDIR is long on macOS.
NVIM_SOCKET="/tmp/devws-follow-$$.sock"
NVIM_PID=""

cleanup() {
  [ -z "$NVIM_PID" ] || kill "$NVIM_PID" 2>/dev/null || true
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT" "$NVIM_SOCKET"
}
trap cleanup EXIT

fail() {
  printf 'Agent follow test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server.
# The agents are stubs that record the arguments devws starts them with.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home" "$TEST_ROOT/project/src" "$TEST_ROOT/agents"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
for agent in claude codex; do
  cat > "$TEST_ROOT/bin/$agent" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" > "$TEST_ROOT/agents/$agent.args"
sleep 600
EOF
done
chmod +x "$TEST_ROOT/bin/"*
ln -s "$SCRIPTS" "$TEST_ROOT/home/.tmuxinator"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX

printf 'line one\nline two\nfunction total()\nline four\n' > "$TEST_ROOT/project/src/a.php"
seq 1 60 | sed 's/^/row /' > "$TEST_ROOT/project/src/b.txt"

# A real Neovim, headless, with devws's Lua modules on its runtime path.
start_editor() {
  [ -z "$NVIM_PID" ] || kill "$NVIM_PID" 2>/dev/null || true
  rm -f "$NVIM_SOCKET"
  # exec, so NVIM_PID is Neovim itself and killing it stops Neovim.
  (cd "$TEST_ROOT/project" &&
    exec "$REAL_NVIM" --headless --clean --listen "$NVIM_SOCKET" \
      --cmd "set rtp^=$ROOT/config/nvim" >/dev/null 2>&1) &
  NVIM_PID=$!
  wait_until test -S "$NVIM_SOCKET" || fail "Neovim did not start"
}

editor_eval() {
  "$REAL_NVIM" --server "$NVIM_SOCKET" --remote-expr "$1"
}

current_location() {
  editor_eval 'fnamemodify(bufname(), ":t") .. ":" .. line(".")'
}

wait_until() {
  local attempt
  for attempt in $(seq 1 30); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

location_is() {
  [ "$(current_location)" = "$1" ]
}

tmux new-session -d -s work -x 200 -y 50 -c "$TEST_ROOT/project" 'sleep 600'
tmux set-option -t work @devws_editor_socket "$NVIM_SOCKET"
AGENT_PANE="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" 'sleep 600')"
tmux set-option -p -t "$AGENT_PANE" @devws_role agent

# Runs a script the way the agent does: from the agent pane, in the project.
from_agent() {
  (cd "$TEST_ROOT/project" && TMUX_PANE="$AGENT_PANE" "$@")
}

test_editor_runner_links_neovim() {
  local session="follow-runner-$$" pane socket
  # A minimal Neovim config with only devws's Lua modules, instead of LazyVim.
  mkdir -p "$TEST_ROOT/config/nvim"
  printf 'vim.opt.rtp:prepend([[%s]])\n' "$ROOT/config/nvim" > "$TEST_ROOT/config/nvim/init.lua"
  tmux new-session -d -s "$session" -x 120 -y 30 -c "$TEST_ROOT/project" 'sleep 600'
  pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "XDG_CONFIG_HOME='$TEST_ROOT/config' '$SCRIPTS/editor_runner.sh' nvim")"
  socket_recorded() { [ -n "$(tmux show-option -qv -t "$session" @devws_editor_socket)" ]; }
  wait_until socket_recorded || fail "the editor runner did not record Neovim's socket"
  socket="$(tmux show-option -qv -t "$session" @devws_editor_socket)"
  wait_until test -S "$socket" || fail "Neovim is not listening on $socket"
  (cd "$TEST_ROOT/project" && TMUX_PANE="$pane" "$SCRIPTS/editor_open.sh" src/a.php:2)
  shown_through_runner() {
    [ "$("$REAL_NVIM" --server "$socket" --remote-expr 'fnamemodify(bufname(), ":t") .. ":" .. line(".")')" = "a.php:2" ]
  }
  wait_until shown_through_runner || fail "a file opened through the runner's socket was not shown"
  tmux kill-session -t "$session"
}

test_open_file_at_line() {
  start_editor
  from_agent "$SCRIPTS/editor_open.sh" src/a.php:3
  wait_until location_is "a.php:3" || fail "editor_open did not open a.php at line 3: $(current_location)"
}

test_open_file_without_line() {
  start_editor
  from_agent "$SCRIPTS/editor_open.sh" "$TEST_ROOT/project/src/b.txt"
  wait_until location_is "b.txt:1" || fail "editor_open did not open b.txt: $(current_location)"
}

test_claude_edit_follows_the_change() {
  start_editor
  printf '{"tool_name":"Edit","tool_input":{"file_path":"%s","old_string":"x","new_string":"function total()\\n  return 1;"}}' \
    "$TEST_ROOT/project/src/a.php" | from_agent "$SCRIPTS/agent_follow_hook.sh" claude
  wait_until location_is "a.php:3" || fail "a Claude edit was not followed: $(current_location)"
}

test_claude_write_follows_the_file() {
  start_editor
  printf '{"tool_name":"Write","tool_input":{"file_path":"%s","content":"row 1\\nrow 2"}}' \
    "$TEST_ROOT/project/src/b.txt" | from_agent "$SCRIPTS/agent_follow_hook.sh" claude
  wait_until location_is "b.txt:1" || fail "a Claude write was not followed: $(current_location)"
}

test_codex_patch_follows_the_last_file() {
  start_editor
  local patch='*** Begin Patch\n*** Update File: src/a.php\n@@\n-line one\n+line one!\n*** Update File: src/b.txt\n@@ row 41\n-row 42\n+row 42\n*** End Patch'
  printf '{"cwd":"%s","tool_name":"apply_patch","tool_input":{"command":"%s"}}' \
    "$TEST_ROOT/project" "$patch" | from_agent "$SCRIPTS/agent_follow_hook.sh" codex
  wait_until location_is "b.txt:42" || fail "a Codex patch was not followed: $(current_location)"
}

test_following_off_ignores_hooks_but_not_requests() {
  start_editor
  tmux set-option -t work @devws_follow off
  printf '{"tool_input":{"file_path":"%s","new_string":"line four"}}' "$TEST_ROOT/project/src/a.php" |
    from_agent "$SCRIPTS/agent_follow_hook.sh" claude
  sleep 1
  [ "$(current_location)" = ":1" ] || fail "an edit was followed while following was off"
  from_agent "$SCRIPTS/editor_open.sh" src/a.php:4
  wait_until location_is "a.php:4" || fail "a request was ignored while following was off"
  tmux set-option -t work @devws_follow on
}

test_typing_user_is_not_interrupted() {
  start_editor
  from_agent "$SCRIPTS/editor_open.sh" src/a.php:2
  wait_until location_is "a.php:2" || fail "setup: a.php did not open"
  "$REAL_NVIM" --server "$NVIM_SOCKET" --remote-send 'i'
  in_insert_mode() { [ "$(editor_eval 'mode()')" = "i" ]; }
  wait_until in_insert_mode || fail "setup: insert mode"
  from_agent "$SCRIPTS/editor_open.sh" --follow src/b.txt:10
  sleep 1
  [ "$(current_location)" = "a.php:2" ] || fail "the editor jumped while the user was typing"
  editor_eval 'execute("messages")' | grep -q "b.txt:10" ||
    fail "no notification about the agent's change"
}

test_hooks_never_fail_without_an_editor() {
  kill "$NVIM_PID" 2>/dev/null || true
  NVIM_PID=""
  rm -f "$NVIM_SOCKET"
  local output
  output="$(printf '{"tool_input":{"file_path":"x"}}' | from_agent "$SCRIPTS/agent_follow_hook.sh" claude 2>&1)" ||
    fail "the hook failed without an editor"
  [ -z "$output" ] || fail "the hook printed something without an editor: $output"
  output="$(printf 'not json' | from_agent "$SCRIPTS/agent_follow_hook.sh" codex 2>&1)" ||
    fail "the hook failed on bad input"
  output="$(printf '{}' | "$SCRIPTS/agent_follow_hook.sh" claude 2>&1)" ||
    fail "the hook failed outside tmux"
  ! from_agent "$SCRIPTS/editor_open.sh" src/a.php 2>/dev/null ||
    fail "a request without an editor should say so"
}

test_toggle_follow() {
  tmux set-option -t work -u @devws_follow
  "$SCRIPTS/toggle_follow.sh" work
  [ "$(tmux show-option -qv -t work @devws_follow)" = "off" ] || fail "C-b F did not turn following off"
  "$SCRIPTS/toggle_follow.sh" work
  [ "$(tmux show-option -qv -t work @devws_follow)" = "on" ] || fail "C-b F did not turn following on"
}

test_agent_header_shows_following() {
  local format
  format="$(sed -n 's/^set -g pane-border-format "\(.*\)"$/\1/p' "$ROOT/config/tmux/tmux.conf")"
  tmux set-option -t work @devws_agent claude
  tmux set-option -t work @devws_follow on
  tmux display-message -p -t "$AGENT_PANE" "$format" | grep -q "following" ||
    fail "the agent header does not show following"
  tmux set-option -t work @devws_follow off
  ! tmux display-message -p -t "$AGENT_PANE" "$format" | grep -q "following" ||
    fail "the agent header shows following while it is off"
  tmux set-option -t work @devws_follow on
}

test_agents_start_with_the_link() {
  local pane
  pane="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh claude")"
  wait_until test -s "$TEST_ROOT/agents/claude.args" || fail "claude did not start"
  grep -qx -- "--settings" "$TEST_ROOT/agents/claude.args" || fail "claude got no hook settings"
  grep -q 'agent_follow_hook.sh.* claude' "$(sed -n '/^--settings$/{n;p;}' "$TEST_ROOT/agents/claude.args")" ||
    fail "claude's settings have no follow hook"
  grep -qx -- "--append-system-prompt" "$TEST_ROOT/agents/claude.args" ||
    fail "claude was not told about the editor"
  grep -q "editor_open.sh" "$TEST_ROOT/agents/claude.args" || fail "claude's prompt does not name editor_open.sh"
  tmux kill-pane -t "$pane"

  pane="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh codex")"
  wait_until test -s "$TEST_ROOT/agents/codex.args" || fail "codex did not start"
  grep -q '^hooks.PostToolUse=.*apply_patch.*agent_follow_hook.sh.* codex' "$TEST_ROOT/agents/codex.args" ||
    fail "codex got no follow hook"
  grep -q '^developer_instructions=.*editor_open.sh' "$TEST_ROOT/agents/codex.args" ||
    fail "codex was not told about the editor"
  tmux kill-pane -t "$pane"
}

test_editor_runner_links_neovim
test_open_file_at_line
test_open_file_without_line
test_claude_edit_follows_the_change
test_claude_write_follows_the_file
test_codex_patch_follows_the_last_file
test_following_off_ignores_hooks_but_not_requests
test_typing_user_is_not_interrupted
test_hooks_never_fail_without_an_editor
test_toggle_follow
test_agent_header_shows_following
test_agents_start_with_the_link

printf 'Agent follow tests passed.\n'
