#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-memory.XXXXXX")"
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
MEMORY="$ROOT/config/tmuxinator/memory.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Memory test failed: %s\n' "$*" >&2
  exit 1
}

# ai-memory logs each call with the config folder it ran with; curl answers
# /healthz from healthz.ok.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.claude" "$TEST_ROOT/home/.codex" "$TEST_ROOT/home/.claude-work"
# Its users are the lines of users.txt; add-human adds one. open logs the
# page it was asked to open.
cat > "$TEST_ROOT/bin/ai-memory" <<EOF
#!/usr/bin/env bash
printf '%s|claude=%s|codex=%s\n' "\$*" "\${CLAUDE_CONFIG_DIR:-}" "\${CODEX_HOME:-}" >> "$TEST_ROOT/ai-memory.log"
case "\$1 \${2:-}" in
  'user list') if [ -s "$TEST_ROOT/users.txt" ]; then cat "$TEST_ROOT/users.txt"; else echo '(no registered users)'; fi ;;
  'user add-human') echo "\$4" >> "$TEST_ROOT/users.txt"; echo 'add-human: one-time login shown here' ;;
esac
EOF
cat > "$TEST_ROOT/bin/open" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$TEST_ROOT/open.log"
EOF
cat > "$TEST_ROOT/bin/curl" <<EOF
#!/usr/bin/env bash
[ -f "$TEST_ROOT/healthz.ok" ] && { printf '{"status":"ok"}'; exit 0; }
exit 7
EOF
chmod +x "$TEST_ROOT/bin/"*
printf 'DEVWS_AGENTS="claude codex claude_work"\nDEVWS_AGENT_claude_work="claude ~/.claude-work"\n' > "$TEST_ROOT/devws.env"
# DEVWS_AI_MEMORY: the stub, never the ai-memory this machine may have.
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" DEVWS_ENV_FILE="$TEST_ROOT/devws.env" \
  DEVWS_AI_MEMORY="$TEST_ROOT/bin/ai-memory"

memory() {
  "$MEMORY" "$@"
}

calls() {
  cat "$TEST_ROOT/ai-memory.log" 2>/dev/null
}

test_available_and_up() {
  memory available || fail "a present ai-memory is not available"
  ! memory up || fail "a service that does not answer is up"
  : > "$TEST_ROOT/healthz.ok"
  memory up || fail "an answering service is not up"
}

test_wire_uses_each_accounts_folder() {
  : > "$TEST_ROOT/ai-memory.log"
  memory wire claude
  grep -qx 'install-mcp --client claude-code --apply|claude=|codex=' <<<"$(calls)" ||
    fail "the default claude was wired with a folder: $(calls)"
  grep -qx 'install-hooks --agent claude-code --apply|claude=|codex=' <<<"$(calls)" || fail "no hooks for claude"
  memory wire claude_work
  grep -qx "install-hooks --agent claude-code --apply|claude=$HOME/.claude-work|codex=" <<<"$(calls)" ||
    fail "claude_work was not wired in its folder: $(calls)"
  memory wire codex
  grep -qx 'install-mcp --client codex --apply|claude=|codex=' <<<"$(calls)" || fail "codex was not wired"
  DEVWS_AGENT_claude="claude ~/.claude" memory wire claude
  grep -qx "install-mcp --client claude-code --apply|claude=$HOME/.claude|codex=" <<<"$(calls)" ||
    fail "claude registered in the default folder was wired without it: $(calls)"
}

test_wire_all_wires_each_agent_once() {
  : > "$TEST_ROOT/ai-memory.log"
  rm -f "$HOME/.devws-state/memory-wired"
  memory wire-all
  [ "$(grep -c 'install-mcp' <<<"$(calls)")" = 3 ] || fail "not every agent was wired: $(calls)"
  : > "$TEST_ROOT/ai-memory.log"
  memory wire-all
  [ -z "$(calls)" ] || fail "wired agents were wired again: $(calls)"
}

# With no login yet, the web page first offers an admin account named after
# you; a no, or an existing login, opens it straight away.
test_web_offers_an_admin_login_first() {
  local output
  rm -f "$TEST_ROOT/users.txt" "$TEST_ROOT/open.log"
  output="$(printf 'n\n' | memory web)"
  grep -q 'no web login yet' <<<"$output" || fail "no login was not said: $output"
  ! grep -q 'add-human' <<<"$(calls)" || fail "a no created an account"
  grep -qx 'http://127.0.0.1:49374/web' "$TEST_ROOT/open.log" || fail "a no did not open the page"

  : > "$TEST_ROOT/ai-memory.log"
  output="$(printf '\n\n' | memory web)"
  grep -q "^user add-human --username $(id -un) --role root|" <<<"$(calls)" ||
    fail "Enter did not create a root account: $(calls)"
  grep -q 'add-human: one-time login shown here' <<<"$output" || fail "ai-memory's one-time login was not shown: $output"

  : > "$TEST_ROOT/ai-memory.log"
  : > "$TEST_ROOT/open.log"
  memory web </dev/null >/dev/null
  ! grep -q 'add-human' <<<"$(calls)" || fail "an account was offered with a login present"
  grep -qx 'http://127.0.0.1:49374/web' "$TEST_ROOT/open.log" || fail "the page did not open with a login present"
}

test_without_ai_memory_nothing_runs() {
  rm "$TEST_ROOT/bin/ai-memory"
  ! memory available || fail "a missing ai-memory is available"
  memory wire-all || fail "wire-all failed without ai-memory"
}

test_available_and_up
test_wire_uses_each_accounts_folder
test_wire_all_wires_each_agent_once
test_web_offers_an_admin_login_first
test_without_ai_memory_nothing_runs

printf 'Memory tests passed.\n'
