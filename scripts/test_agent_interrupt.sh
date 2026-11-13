#!/usr/bin/env bash
# Exercise interruption with a stub terminal; never touch a real team.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-interrupt.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/scripts"
cp "$ROOT/config/tmuxinator/agent_cli.sh" "$TEST_ROOT/scripts/agent_cli.sh"
printf 'agent_kind() { case "$1" in claude|codex) printf "%%s\\n" "$1" ;; *) return 1 ;; esac; }\n' > "$TEST_ROOT/scripts/agents.sh"
cat > "$TEST_ROOT/bin/tmux" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  display-message) case "${*: -1}" in '#{pane_id}') echo %1 ;; '#{session_name}') echo test ;; esac ;;
  show-options) case "${*: -1}" in @devws_agent) cat "$INTERRUPT_TEST/agent" ;; @devws_pane_state) cat "$INTERRUPT_TEST/state" ;; esac ;;
  send-keys)
    [ "${*: -1}" = C-c ] || exit 1
    echo interrupt >> "$INTERRUPT_TEST/signals"
    [ -f "$INTERRUPT_TEST/unacknowledged" ] || echo done > "$INTERRUPT_TEST/state"
    ;;
  *) echo "Unexpected terminal action: $*" >&2; exit 1 ;;
esac
STUB
printf '#!/usr/bin/env bash\nexit 0\n' > "$TEST_ROOT/bin/sleep"
chmod +x "$TEST_ROOT/bin/tmux" "$TEST_ROOT/bin/sleep"
export PATH="$TEST_ROOT/bin:$PATH" INTERRUPT_TEST="$TEST_ROOT"
for agent in claude codex; do
  echo "$agent" > "$TEST_ROOT/agent"
  echo working > "$TEST_ROOT/state"
  : > "$TEST_ROOT/signals"
  bash "$TEST_ROOT/scripts/agent_cli.sh" interrupt %1
  [ "$(wc -l < "$TEST_ROOT/signals" | tr -d ' ')" = 1 ]
  bash "$TEST_ROOT/scripts/agent_cli.sh" interrupt %1
  [ "$(wc -l < "$TEST_ROOT/signals" | tr -d ' ')" = 1 ]
done
echo unknown > "$TEST_ROOT/agent"
echo working > "$TEST_ROOT/state"
if bash "$TEST_ROOT/scripts/agent_cli.sh" interrupt %1 2>/dev/null; then exit 1; fi
echo claude > "$TEST_ROOT/agent"
touch "$TEST_ROOT/unacknowledged"
: > "$TEST_ROOT/signals"
if bash "$TEST_ROOT/scripts/agent_cli.sh" interrupt %1 2>/dev/null; then exit 1; fi
[ "$(wc -l < "$TEST_ROOT/signals" | tr -d ' ')" = 1 ]
printf 'Agent interruption tests passed\n'
