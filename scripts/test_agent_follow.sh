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
# The editor runner's sockets (nvim_servers.sh), apart from the real ones.
export DEVWS_NVIM_SOCKET_PREFIX="/tmp/devws-nvim-test-$$-" DEVWS_NVIM_GRACE=1
NVIM_PID=""

cleanup() {
  [ -z "$NVIM_PID" ] || kill "$NVIM_PID" 2>/dev/null || true
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  pkill -KILL -f -- "--listen $DEVWS_NVIM_SOCKET_PREFIX" 2>/dev/null || true
  rm -rf "$TEST_ROOT" "$NVIM_SOCKET" "$DEVWS_NVIM_SOCKET_PREFIX"*
}
trap cleanup EXIT

fail() {
  printf 'Agent follow test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server.
# The agents are stubs that record the arguments devws starts them with, and
# the account settings in their environment.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home" "$TEST_ROOT/project/src" "$TEST_ROOT/agents"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
for agent in claude codex; do
  cat > "$TEST_ROOT/bin/$agent" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" > "$TEST_ROOT/agents/$agent.args"
env | grep -E '^(CLAUDE_CONFIG_DIR|CODEX_HOME|DEVWS_AGENT)=' > "$TEST_ROOT/agents/$agent.env"
sleep 600
EOF
done
chmod +x "$TEST_ROOT/bin/"*
# Every script but the .env: the agents registered on this machine (a
# DEVWS_AGENT_claude of the user's own) must not change what the test sees.
mkdir -p "$TEST_ROOT/home/.tmuxinator"
for script in "$SCRIPTS"/*; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/${script##*/}"
done
printf 'DEVWS_AGENTS="claude codex"\n' > "$TEST_ROOT/home/.tmuxinator/.env"
# The scripts run from there, as from ~/.tmuxinator: each reads the .env
# beside the path it was started by.
SCRIPTS="$TEST_ROOT/home/.tmuxinator"
# TMPDIR keeps devws's temporary files (frozen workspaces' records among them)
# apart from the real ones.
mkdir -p "$TEST_ROOT/tmp"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT/tmp"
unset TMUX

printf 'line one\nline two\nfunction total()\nline four\n' > "$TEST_ROOT/project/src/a.php"
seq 1 60 | sed 's/^/row /' > "$TEST_ROOT/project/src/b.txt"

# A real Neovim, headless, with devws's Lua modules on its runtime path.
start_editor() {
  [ -z "$NVIM_PID" ] || kill "$NVIM_PID" 2>/dev/null || true
  rm -f "$NVIM_SOCKET"
  # exec, so NVIM_PID is Neovim itself and killing it stops Neovim. No swap
  # files (-n): a test that leaves unsaved changes would otherwise leave the
  # next Neovim at a swap-file prompt.
  (cd "$TEST_ROOT/project" &&
    exec "$REAL_NVIM" --headless --clean -n --listen "$NVIM_SOCKET" \
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
EDITOR_PANE="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" 'sleep 600')"
tmux set-option -p -t "$EDITOR_PANE" @devws_role editor
AGENT_PANE="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" 'sleep 600')"
tmux set-option -p -t "$AGENT_PANE" @devws_role agent
tmux set-option -t work @devws_follow follow

# Runs a script the way the agent does: from the agent pane, in the project.
from_agent() {
  (cd "$TEST_ROOT/project" && TMUX_PANE="$AGENT_PANE" "$@")
}

test_editor_runner_links_neovim() {
  local session="follow-runner-$$" pane socket stuck
  # A Neovim an earlier editor left stuck on the socket (busy, deaf to SIGTERM
  # and to the socket, like one at its exit prompt): the runner stops it.
  (exec "$REAL_NVIM" --clean --headless --listen "${DEVWS_NVIM_SOCKET_PREFIX}$session.sock" \
    --cmd 'lua vim.uv.sleep(600000)' >/dev/null 2>&1 </dev/null &)
  stuck_started() { stuck="$(pgrep -f -- "--listen ${DEVWS_NVIM_SOCKET_PREFIX}$session.sock")"; }
  wait_until stuck_started || fail "the stuck Neovim did not start"
  # A minimal Neovim config with only devws's Lua modules, instead of LazyVim.
  mkdir -p "$TEST_ROOT/config/nvim"
  printf 'vim.opt.rtp:prepend([[%s]])\n' "$ROOT/config/nvim" > "$TEST_ROOT/config/nvim/init.lua"
  tmux new-session -d -s "$session" -x 120 -y 30 -c "$TEST_ROOT/project" 'sleep 600'
  pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "XDG_CONFIG_HOME='$TEST_ROOT/config' '$SCRIPTS/editor_runner.sh' nvim")"
  socket_recorded() { [ -n "$(tmux show-option -qv -t "$session" @devws_editor_socket)" ]; }
  wait_until socket_recorded || fail "the editor runner did not record Neovim's socket"
  socket="$(tmux show-option -qv -t "$session" @devws_editor_socket)"
  [ "$socket" = "${DEVWS_NVIM_SOCKET_PREFIX}$session.sock" ] || fail "the runner's socket is $socket"
  ! kill -0 "$stuck" 2>/dev/null || fail "the runner left the stuck Neovim $stuck on its socket"
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

# The new text's first line also appears earlier in the file, so only the
# patch Claude Code reports says where the change really is.
test_claude_edit_follows_the_reported_patch() {
  start_editor
  printf '{"tool_name":"Edit","tool_input":{"file_path":"%s","new_string":"line two"},"tool_response":{"structuredPatch":[{"newStart":3,"lines":[" function total()","-line four","+line two"]}]}}' \
    "$TEST_ROOT/project/src/a.php" | from_agent "$SCRIPTS/agent_follow_hook.sh" claude
  wait_until location_is "a.php:4" || fail "a Claude edit was not followed to its patch: $(current_location)"
}

# A git project for the tests of changes found by snapshot: a.txt committed,
# b.txt untracked, ignored/ in .gitignore.
new_repo() {
  local repo="$TEST_ROOT/repo"
  rm -rf "$repo"
  mkdir -p "$repo/ignored" "$repo/src"
  printf 'one\ntwo\nthree\nfour\nfive\n' > "$repo/a.txt"
  printf 'ignored/\n' > "$repo/.gitignore"
  git -C "$repo" init -q
  git -C "$repo" add a.txt .gitignore
  git -C "$repo" -c user.name=test -c user.email=test@example.com commit -q -m start
  printf 'untracked\n' > "$repo/b.txt"
  printf 'old\n' > "$repo/ignored/z.txt"
  printf '%s' "$repo"
}

changes() {
  "$SCRIPTS/agent_changes.sh" "$@"
}

# Whatever changed the files — a shell command too — the lines it changed are
# found from the project itself, before and after; ignored files are left out,
# and the project's own index is never touched.
test_changes_are_found_by_snapshot() {
  local repo real index_before output expected
  repo="$(new_repo)"
  real="$(cd "$repo" && pwd -P)"
  index_before="$(shasum < "$repo/.git/index")"
  changes snapshot "$repo/src" pane-1 || fail "the snapshot failed"
  sed -i '' 's/^three$/THREE/' "$repo/a.txt"
  printf 'added\n' >> "$repo/b.txt"
  printf 'new\nfile\n' > "$repo/src/c.txt"
  printf 'changed\n' > "$repo/ignored/z.txt"
  output="$(changes changes "$repo/src" pane-1 | sort)"
  expected="$(printf '%s\t3\t3\n%s\t2\t2\n%s\t1\t2' "$real/a.txt" "$real/b.txt" "$real/src/c.txt")"
  [ "$output" = "$expected" ] || fail "the changes found are wrong: $output"
  [ "$(shasum < "$repo/.git/index")" = "$index_before" ] || fail "the project's index was changed"
  [ -z "$(changes changes "$repo" pane-1)" ] || fail "changes were reported twice for one snapshot"
  # Deleted lines are marked where they were.
  changes snapshot "$repo" pane-1
  sed -i '' '/^four$/d' "$repo/a.txt"
  [ "$(changes changes "$repo" pane-1)" = "$(printf '%s\t3\t3' "$real/a.txt")" ] ||
    fail "a deletion is not marked where it was: $(changes changes "$repo" pane-1)"
  # Outside a git project nothing is found, and nothing fails.
  changes snapshot "$TEST_ROOT/project" pane-1 || fail "a snapshot outside git failed"
  [ -z "$(changes changes "$TEST_ROOT/project" pane-1)" ] || fail "changes were found outside git"
}

# The lines the agent changed, marked in the sign column of the current
# buffer: their numbers, one a line.
marked_lines() {
  editor_eval "join(map(nvim_buf_get_extmarks(0, nvim_create_namespace('devws_follow_marks'), 0, -1, {}), {_, mark -> mark[1] + 1}), ' ')"
}

# Every line the agent changed is marked, also in a file opened later, and
# stays marked when the file reloads, until the marks are cleared.
test_changed_lines_are_marked() {
  local a="$TEST_ROOT/project/src/a.php" b="$TEST_ROOT/project/src/b.txt"
  start_editor
  editor_eval "v:lua.require'devws.follow'.mark([['$a', 2, 3], ['$b', 10, 10]])" >/dev/null
  from_agent "$SCRIPTS/editor_open.sh" src/a.php:1
  wait_until location_is "a.php:1" || fail "setup: a.php did not open"
  [ "$(marked_lines)" = "2 3" ] || fail "a.php's changed lines are not marked: $(marked_lines)"
  from_agent "$SCRIPTS/editor_open.sh" src/b.txt:1
  wait_until location_is "b.txt:1" || fail "setup: b.txt did not open"
  [ "$(marked_lines)" = "10" ] || fail "b.txt's changed line is not marked: $(marked_lines)"
  # Changed on disk again and reloaded: still marked.
  printf 'row 0\n' >> "$b"
  editor_eval 'execute("checktime")' >/dev/null
  [ "$(marked_lines)" = "10" ] || fail "the marks went with a reload: $(marked_lines)"
  editor_eval "v:lua.require'devws.follow'.clear_marks()" >/dev/null
  [ -z "$(marked_lines)" ] || fail "the marks were not cleared: $(marked_lines)"
  from_agent "$SCRIPTS/editor_open.sh" src/a.php:1
  wait_until location_is "a.php:1" || fail "setup: a.php did not open again"
  [ -z "$(marked_lines)" ] || fail "a cleared mark came back on a.php: $(marked_lines)"
}

# A tool call as the agent's hooks report it: the follow hook's phase (pre,
# post or prompt) for <agent>, with a payload naming <tool> run in <dir>.
tool_call() {
  local agent="$1" phase="$2" tool="$3" dir="$4"
  printf '{"tool_name":"%s","tool_input":{"command":"a shell command"},"cwd":"%s"}' "$tool" "$dir" |
    (cd "$dir" && TMUX_PANE="$AGENT_PANE" "$SCRIPTS/agent_follow_hook.sh" "$agent" ${phase:+"$phase"})
}

# A change made through a shell command is followed too: the editor shows the
# file changed last at its change, and every changed line of every changed
# file is marked, until the user's next prompt.
test_a_shell_edit_is_followed_and_marked() {
  local repo agent tool
  for agent in claude:Bash codex:exec; do
    tool="${agent#*:}"
    agent="${agent%%:*}"
    repo="$(new_repo)"
    start_editor
    tool_call "$agent" pre "$tool" "$repo"
    sed -i '' 's/^three$/THREE/' "$repo/a.txt"
    sleep 0.1
    printf 'added\n' >> "$repo/b.txt"
    tool_call "$agent" "" "$tool" "$repo"
    wait_until location_is "b.txt:2" || fail "$agent: a shell edit was not followed to the file changed last: $(current_location)"
    [ "$(marked_lines)" = "2" ] || fail "$agent: b.txt's changed line is not marked: $(marked_lines)"
    from_agent "$SCRIPTS/editor_open.sh" "$repo/a.txt:1"
    wait_until location_is "a.txt:1" || fail "setup: a.txt did not open"
    [ "$(marked_lines)" = "3" ] || fail "$agent: a.txt's changed line is not marked: $(marked_lines)"
    tool_call "$agent" prompt "" "$repo"
    clear() { [ -z "$(marked_lines)" ]; }
    wait_until clear || fail "$agent: a new prompt did not clear the marks: $(marked_lines)"
  done

  # Following off: nothing shown, nothing marked.
  start_editor
  tmux set-option -t work @devws_follow off
  tool_call claude pre Bash "$repo"
  sed -i '' 's/^one$/ONE/' "$repo/a.txt"
  tool_call claude "" Bash "$repo"
  sleep 1
  [ "$(current_location)" = ":1" ] || fail "following off still showed a shell edit: $(current_location)"
  tmux set-option -t work @devws_follow follow
}

# LazyVim opened on a folder (nvim ., as the editor runner does) shows its
# dashboard and focuses the explorer, a floating list: no window holds a file.
# The change must still be shown at its line, in a real window, with the focus.
test_a_change_is_shown_from_the_explorer() {
  start_editor
  editor_eval 'execute("lua local dashboard = vim.api.nvim_create_buf(false, true); vim.bo[dashboard].filetype = \"dashboard\"; vim.api.nvim_win_set_buf(0, dashboard); local list = vim.api.nvim_create_buf(false, true); vim.bo[list].filetype = \"explorer\"; vim.api.nvim_open_win(list, true, { relative = \"editor\", row = 1, col = 1, width = 30, height = 10 })")' >/dev/null
  [ "$(editor_eval 'nvim_win_get_config(0).relative')" = editor ] || fail "setup: the explorer float is not focused"
  printf '{"tool_name":"Edit","tool_input":{"file_path":"%s","new_string":"function total()"}}' \
    "$TEST_ROOT/project/src/a.php" | from_agent "$SCRIPTS/agent_follow_hook.sh" claude
  shown_in_a_real_window() {
    location_is "a.php:3" && [ -z "$(editor_eval 'nvim_win_get_config(0).relative')" ]
  }
  wait_until shown_in_a_real_window ||
    fail "a change made while the explorer had the focus was not shown at it: $(current_location) in a $(editor_eval 'nvim_win_get_config(0).relative == "" ? "real" : "floating"') window"
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
  tmux set-option -t work @devws_follow follow
}

claude_edit() {
  printf '{"tool_name":"Edit","tool_input":{"file_path":"%s","new_string":"%s"}}' \
    "$TEST_ROOT/project/src/$1" "$2" | from_agent "$SCRIPTS/agent_follow_hook.sh" claude
}

claude_read() {
  printf '{"tool_name":"Read","tool_input":{"file_path":"%s","offset":%s}}' \
    "$TEST_ROOT/project/src/$1" "$2" | from_agent "$SCRIPTS/agent_follow_hook.sh" claude
}

test_notify_mode_only_notifies() {
  start_editor
  tmux set-option -t work @devws_follow notify
  claude_edit a.php "line four"
  notified() { editor_eval 'execute("messages")' | grep -q "a.php:4"; }
  wait_until notified || fail "notify mode did not notify about the change"
  [ "$(current_location)" = ":1" ] || fail "notify mode moved the editor"
  tmux set-option -t work @devws_follow follow
}

test_focus_mode_moves_the_focus_to_the_editor() {
  start_editor
  tmux select-pane -t "$AGENT_PANE"
  tmux set-option -t work @devws_follow focus
  claude_edit a.php "line two"
  wait_until location_is "a.php:2" || fail "focus mode did not follow the change: $(current_location)"
  editor_focused() { [ "$(tmux display-message -p -t work: '#{pane_id}')" = "$EDITOR_PANE" ]; }
  wait_until editor_focused || fail "focus mode did not move the focus to the editor"
  tmux select-pane -t "$AGENT_PANE"
  tmux set-option -t work @devws_follow follow
}

test_reads_are_followed_only_in_explore_mode() {
  start_editor
  claude_read b.txt 30
  sleep 1
  [ "$(current_location)" = ":1" ] || fail "a read was followed in follow mode"
  tmux set-option -t work @devws_follow explore
  claude_read b.txt 30
  wait_until location_is "b.txt:30" || fail "explore mode did not follow a read: $(current_location)"
  claude_edit a.php "line four"
  wait_until location_is "a.php:4" || fail "explore mode did not follow an edit"
  tmux set-option -t work @devws_follow follow
}

test_follow_mode_sets_modes() {
  local mode
  for mode in off notify focus explore follow; do
    "$SCRIPTS/follow_mode.sh" work "$mode"
    [ "$(tmux show-option -qv -t work @devws_follow)" = "$mode" ] || fail "could not set the $mode mode"
  done
  ! "$SCRIPTS/follow_mode.sh" work sideways 2>/dev/null || fail "an unknown mode was accepted"
  [ "$(tmux show-option -qv -t work @devws_follow)" = "follow" ] || fail "an unknown mode changed the mode"
}

test_workspaces_from_before_the_modes_still_follow() {
  start_editor
  tmux set-option -t work @devws_follow on
  claude_edit a.php "line two"
  wait_until location_is "a.php:2" || fail "the old \"on\" no longer follows"
  tmux set-option -t work @devws_follow follow
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

test_unsaved_changes_elsewhere_do_not_stop_following() {
  start_editor
  from_agent "$SCRIPTS/editor_open.sh" src/a.php:2
  wait_until location_is "a.php:2" || fail "setup: a.php did not open"
  editor_eval 'execute("normal! Ounsaved")' >/dev/null
  from_agent "$SCRIPTS/editor_open.sh" --follow src/b.txt:10
  wait_until location_is "b.txt:10" || fail "an unsaved buffer stopped following: $(current_location)"
}

test_unsaved_changes_in_the_changed_file_only_notify() {
  start_editor
  from_agent "$SCRIPTS/editor_open.sh" src/a.php:2
  wait_until location_is "a.php:2" || fail "setup: a.php did not open"
  editor_eval 'execute("normal! Ounsaved")' >/dev/null
  from_agent "$SCRIPTS/editor_open.sh" --follow src/a.php:4
  sleep 1
  [ "$(current_location)" = "a.php:2" ] || fail "the editor moved over the user's unsaved changes"
  editor_eval 'execute("messages")' | grep -q "a.php:4" ||
    fail "no notification about the change to the unsaved file"
}

# Follow requests reach Neovim in the background and may arrive out of order;
# one stamped earlier than a request already shown must not move the editor back.
test_an_older_follow_does_not_override_a_newer_one() {
  start_editor
  local show="v:lua.require'devws.follow'.show"
  editor_eval "$show('$TEST_ROOT/project/src/b.txt', 20, '', v:false, 200)" >/dev/null
  editor_eval "$show('$TEST_ROOT/project/src/a.php', 3, '', v:false, 100)" >/dev/null
  [ "$(current_location)" = "b.txt:20" ] || fail "an older follow overrode a newer one: $(current_location)"
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

test_agent_header_shows_the_mode() {
  local format
  format="$(sed -n 's/^set -g pane-border-format "\(.*\)"$/\1/p' "$ROOT/config/tmux/tmux.conf")"
  tmux set-option -t work @devws_agent claude
  header_for() {
    tmux set-option -t work @devws_follow "$1"
    tmux display-message -p -t "$AGENT_PANE" "$format"
  }
  header_for follow | grep -q "· following" || fail "the header does not show following"
  header_for on | grep -q "· following" || fail "the header does not show the old \"on\" as following"
  header_for notify | grep -q "· notify" || fail "the header does not show notify"
  header_for focus | grep -q "· following + focus" || fail "the header does not show focus"
  header_for explore | grep -q "· following reads" || fail "the header does not show explore"
  ! header_for off | grep -qE "following|notify" || fail "the header shows a mode while following is off"
  tmux set-option -t work @devws_follow follow
}

# An agent registered with a folder runs there, even its kind's default one (a
# folder set, even the default one, is a login of its own in Claude Code, as
# `CLAUDE_CONFIG_DIR=~/.claude claude` is); a bare name runs with no folder
# set, as the plain command would.
test_registered_agents_start_with_their_folder() {
  local pane
  rm -f "$TEST_ROOT/agents/claude.env" "$TEST_ROOT/agents/codex.env"
  pane="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "DEVWS_AGENT_work='claude ~/.claude-work' CLAUDE_CONFIG_DIR=/elsewhere '$SCRIPTS/agent_runner.sh' work")"
  wait_until test -s "$TEST_ROOT/agents/claude.env" || fail "the work agent did not start claude"
  grep -qx "CLAUDE_CONFIG_DIR=$HOME/.claude-work" "$TEST_ROOT/agents/claude.env" ||
    fail "the work agent runs outside its folder: $(cat "$TEST_ROOT/agents/claude.env")"
  grep -qx "DEVWS_AGENT=work" "$TEST_ROOT/agents/claude.env" || fail "the work agent does not know its name"
  tmux kill-pane -t "$pane"

  rm -f "$TEST_ROOT/agents/claude.env"
  pane="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "CLAUDE_CONFIG_DIR=/elsewhere '$SCRIPTS/agent_runner.sh' claude")"
  wait_until test -s "$TEST_ROOT/agents/claude.env" || fail "the claude agent did not start"
  ! grep -q "^CLAUDE_CONFIG_DIR=" "$TEST_ROOT/agents/claude.env" ||
    fail "the default-folder agent got a folder: $(cat "$TEST_ROOT/agents/claude.env")"
  grep -qx "DEVWS_AGENT=claude" "$TEST_ROOT/agents/claude.env" || fail "the claude agent does not know its name"
  tmux kill-pane -t "$pane"

  rm -f "$TEST_ROOT/agents/claude.env"
  pane="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "DEVWS_AGENT_claude='claude ~/.claude' '$SCRIPTS/agent_runner.sh' claude")"
  wait_until test -s "$TEST_ROOT/agents/claude.env" || fail "the claude agent with a folder did not start"
  grep -qx "CLAUDE_CONFIG_DIR=$HOME/.claude" "$TEST_ROOT/agents/claude.env" ||
    fail "the agent registered in the default folder got no folder: $(cat "$TEST_ROOT/agents/claude.env")"
  tmux kill-pane -t "$pane"

  ! "$SCRIPTS/agent_runner.sh" ghost 2>/dev/null || fail "an unregistered agent was started"
}

test_agents_start_with_the_link() {
  local pane settings
  pane="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh claude")"
  wait_until test -s "$TEST_ROOT/agents/claude.args" || fail "claude did not start"
  grep -qx -- "--settings" "$TEST_ROOT/agents/claude.args" || fail "claude got no hook settings"
  settings="$(sed -n '/^--settings$/{n;p;}' "$TEST_ROOT/agents/claude.args")"
  # Shell commands are followed as edits are: snapshot before, changes after;
  # a new prompt clears the marks.
  follow_hook() {
    jq -r --arg event "$1" --arg phase "$2" '.hooks[$event][] | select(any(.hooks[]; .command
      | test("agent_follow_hook.sh\" claude" + (if $phase == "" then "$" else " " + $phase + "$" end)))) | .matcher // "*"' "$settings"
  }
  [ "$(follow_hook PreToolUse pre)" = "Bash|Edit|MultiEdit|Write|NotebookEdit" ] ||
    fail "claude's tools are not snapshot before they run: $(follow_hook PreToolUse pre)"
  [ "$(follow_hook PostToolUse "")" = "Bash|Edit|MultiEdit|Write|NotebookEdit|Read" ] ||
    fail "claude's follow hook does not see shell commands and reads: $(follow_hook PostToolUse "")"
  [ "$(follow_hook UserPromptSubmit prompt)" = "*" ] || fail "claude's prompts do not clear the marks"
  grep -qx -- "--append-system-prompt" "$TEST_ROOT/agents/claude.args" ||
    fail "claude was not told about the editor"
  grep -q "editor_open.sh" "$TEST_ROOT/agents/claude.args" || fail "claude's prompt does not name editor_open.sh"
  tmux kill-pane -t "$pane"

  pane="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh codex")"
  wait_until test -s "$TEST_ROOT/agents/codex.args" || fail "codex did not start"
  # Codex's tools vary (exec, shell, apply_patch): every one is followed.
  grep -q '^hooks.PreToolUse=.*agent_follow_hook.sh\\" codex pre' "$TEST_ROOT/agents/codex.args" ||
    fail "codex's tools are not snapshot before they run"
  grep '^hooks.PostToolUse=' "$TEST_ROOT/agents/codex.args" | grep -q 'agent_follow_hook.sh\\" codex"' ||
    fail "codex got no follow hook"
  ! grep '^hooks.PostToolUse=' "$TEST_ROOT/agents/codex.args" | grep -q 'matcher' ||
    fail "codex's follow hook is limited to some tools"
  grep -q '^hooks.UserPromptSubmit=.*agent_follow_hook.sh\\" codex prompt' "$TEST_ROOT/agents/codex.args" ||
    fail "codex's prompts do not clear the marks"
  grep -q '^developer_instructions=.*editor_open.sh' "$TEST_ROOT/agents/codex.args" ||
    fail "codex was not told about the editor"
  grep -qx 'tui.status_line=\["model-with-reasoning","git-branch","five-hour-limit","weekly-limit","context-used"\]' \
    "$TEST_ROOT/agents/codex.args" || fail "codex got no usage status line"
  tmux kill-pane -t "$pane"

  # A status line of the user's own wins.
  rm "$TEST_ROOT/agents/codex.args"
  mkdir -p "$HOME/.codex"
  printf '[tui]\nstatus_line = ["model-name"]\n' > "$HOME/.codex/config.toml"
  pane="$(tmux split-window -t work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh codex")"
  wait_until test -s "$TEST_ROOT/agents/codex.args" || fail "codex did not start again"
  ! grep -q '^tui.status_line=' "$TEST_ROOT/agents/codex.args" ||
    fail "devws's status line replaced the user's own"
  tmux kill-pane -t "$pane"
  rm "$HOME/.codex/config.toml"
}

test_follow_skips_repository_work_without_a_consumer() {
  local repo real_git state_dir snapshot_file probe
  repo="$(new_repo)"
  start_editor
  real_git="$(command -v git)"
  probe="$TEST_ROOT/git-probe"
  mkdir -p "$probe"
  cat > "$probe/git" <<EOF
#!/usr/bin/env bash
printf '%s\\n' "\$*" >> "$TEST_ROOT/git-calls"
exec "$real_git" "\$@"
EOF
  chmod +x "$probe/git"
  state_dir="$TMPDIR/devws-changes-$(id -u)"
  snapshot_file="$state_dir/$(printf '%s' "pane-$AGENT_PANE" | tr -c 'A-Za-z0-9_.-' '_').before"
  tmux set-option -t work @devws_follow follow
  PATH="$probe:$PATH" tool_call codex pre exec "$repo"
  [ -f "$snapshot_file" ] || fail "eligible follow did not snapshot"

  tmux set-option -t work @devws_follow off
  : > "$TEST_ROOT/git-calls"
  PATH="$probe:$PATH" tool_call codex "" exec "$repo"
  PATH="$probe:$PATH" tool_call codex pre exec "$repo"
  [ ! -s "$TEST_ROOT/git-calls" ] || fail "disabled follow still inspected Git"
  [ ! -f "$snapshot_file" ] || fail "disabled follow kept an obsolete snapshot"

  tmux set-option -t work @devws_follow follow
  : > "$TEST_ROOT/git-calls"
  PATH="$probe:$PATH" tool_call codex "" exec "$repo"
  ! grep -q ' add -A' "$TEST_ROOT/git-calls" || fail "reenabling follow reused a skipped snapshot"
  PATH="$probe:$PATH" tool_call codex pre exec "$repo"
  tmux set-option -t work @devws_editor_socket "$TEST_ROOT/missing.sock"
  : > "$TEST_ROOT/git-calls"
  PATH="$probe:$PATH" tool_call codex "" exec "$repo"
  [ ! -s "$TEST_ROOT/git-calls" ] || fail "a missing editor still inspected Git"
  tmux set-option -t work @devws_editor_socket "$NVIM_SOCKET"
}

test_editor_runner_links_neovim
test_registered_agents_start_with_their_folder
test_open_file_at_line
test_open_file_without_line
test_claude_edit_follows_the_change
test_claude_edit_follows_the_reported_patch
test_claude_write_follows_the_file
test_a_change_is_shown_from_the_explorer
test_changes_are_found_by_snapshot
test_changed_lines_are_marked
test_a_shell_edit_is_followed_and_marked
test_codex_patch_follows_the_last_file
test_following_off_ignores_hooks_but_not_requests
test_typing_user_is_not_interrupted
test_unsaved_changes_elsewhere_do_not_stop_following
test_unsaved_changes_in_the_changed_file_only_notify
test_an_older_follow_does_not_override_a_newer_one
test_notify_mode_only_notifies
test_focus_mode_moves_the_focus_to_the_editor
test_reads_are_followed_only_in_explore_mode
test_follow_mode_sets_modes
test_workspaces_from_before_the_modes_still_follow
test_hooks_never_fail_without_an_editor
test_agent_header_shows_the_mode
test_agents_start_with_the_link
test_follow_skips_repository_work_without_a_consumer

printf 'Agent follow tests passed.\n'
