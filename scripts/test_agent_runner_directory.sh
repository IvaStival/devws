#!/usr/bin/env bash
# Deleted working directories must recover only to the workspace's known root.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-runner-directory.XXXXXX")"
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/project" "$TEST_ROOT/valid"
cat > "$TEST_ROOT/bin/tmux" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  show-options) printf '%s\n' "$TEST_WORKSPACE_ROOT" ;;
  display-message) printf 'test-session\n' ;;
esac
STUB
cat > "$TEST_ROOT/bin/claude" <<'STUB'
#!/usr/bin/env bash
pwd -P > "$TEST_LAUNCH_PATH"
STUB
chmod +x "$TEST_ROOT/bin/"*
export PATH="$TEST_ROOT/bin:$PATH" TMUX_PANE=%test
export TEST_LAUNCH_PATH="$TEST_ROOT/launched" TEST_WORKSPACE_ROOT="$TEST_ROOT/project"
runner="$ROOT/config/tmuxinator/agent_runner.sh"
run_deleted() (
  mkdir "$TEST_ROOT/deleted"
  cd "$TEST_ROOT/deleted"
  rmdir "$TEST_ROOT/deleted"
  export PWD=.
  "$runner" claude
)
run_deleted 2>"$TEST_ROOT/error"
[ "$(cat "$TEST_LAUNCH_PATH")" = "$TEST_ROOT/project" ] || { echo 'Deleted directory did not recover to workspace root' >&2; exit 1; }
(cd "$TEST_ROOT/valid" && "$runner" claude)
[ "$(cat "$TEST_LAUNCH_PATH")" = "$TEST_ROOT/valid" ] || { echo 'Valid working directory was replaced' >&2; exit 1; }
for TEST_WORKSPACE_ROOT in . "$TEST_ROOT/missing"; do
  export TEST_WORKSPACE_ROOT
  rm -f "$TEST_LAUNCH_PATH"
  if run_deleted 2>"$TEST_ROOT/error"; then
    echo 'Invalid workspace root was accepted' >&2; exit 1
  fi
  [ ! -e "$TEST_LAUNCH_PATH" ] || { echo 'Provider started with an invalid root' >&2; exit 1; }
  grep -q 'working directory' "$TEST_ROOT/error"
done
printf 'Agent runner directory tests passed.\n'
