#!/usr/bin/env bash
# A deleted folder must never become a workspace's directory: the tmux server
# starts from $HOME, and the templates record and enter the absolute root.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-server-directory.XXXXXX")"
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
# A socket path has a short length limit: the sockets get a folder of their own.
export TMUX_TMPDIR="$(mktemp -d /tmp/devws-sd.XXXXXX)"
unset TMUX TMUX_PANE
mkdir -p "$TEST_ROOT/home" "$TEST_ROOT/project" "$TEST_ROOT/deleted"
trap 'tmux kill-server 2>/dev/null || true; rm -rf "$TEST_ROOT" "$TMUX_TMPDIR"' EXIT

fail() { printf '%s\n' "$1" >&2; exit 1; }

(cd "$TEST_ROOT/deleted" && HOME="$TEST_ROOT/home" "$ROOT/config/tmuxinator/start_server.sh")
server_pid="$(tmux display-message -p '#{pid}')"
server_cwd="$(lsof -a -d cwd -p "$server_pid" -Fn | sed -n 's/^n//p')"
[ "$server_cwd" = "$TEST_ROOT/home" ] || fail "Server started in $server_cwd, not in HOME"

tmux new-session -d -s first -c "$TEST_ROOT/deleted"
tmux set-option -g exit-empty on
rmdir "$TEST_ROOT/deleted"
tmux new-session -d -s second -c "$TEST_ROOT/project" "/bin/pwd -P > '$TEST_ROOT/pane'; sleep 5"
for _ in $(seq 50); do [ -s "$TEST_ROOT/pane" ] && break; sleep 0.1; done
[ "$(cat "$TEST_ROOT/pane")" = "$TEST_ROOT/project" ] || fail 'New pane did not open in its folder'

(cd "$TEST_ROOT/home" && HOME="$TEST_ROOT/home" "$ROOT/config/tmuxinator/start_server.sh")
[ "$(tmux display-message -p '#{pid}')" = "$server_pid" ] || fail 'A running server was replaced'

for template in dev agentic; do
  rendered="$(tmuxinator debug -p "$ROOT/config/tmuxinator/$template.yml" project_root="$TEST_ROOT/project")"
  # tmuxinator escapes the pre_window command for send-keys.
  grep -qF "cd\\ \\\"$TEST_ROOT/project\\\"" <<<"$rendered" || fail "$template panes do not enter the root"
  grep -qF "@devws_root\\ \\\"$TEST_ROOT/project\\\"" <<<"$rendered" || fail "$template does not record the absolute root"
done

printf 'Server directory tests passed.\n'
