#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="${TMPDIR:-/tmp}"
TEST_ROOT="$(mktemp -d "${tmp_dir%/}/devws-register.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-register-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Register agent test failed: %s\n' "$*" >&2
  exit 1
}

# tmux goes to an isolated server, with no header running; the .env is reached
# through a symlink, as ~/.tmuxinator/.env is.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.claude" "$TEST_ROOT/home/.claude-work" "$TEST_ROOT/repo"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
chmod +x "$TEST_ROOT/bin/tmux"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX
ENV_FILE="$TEST_ROOT/repo/.env"
mkdir -p "$HOME/.tmuxinator"
ln -s "$ENV_FILE" "$HOME/.tmuxinator/.env"
export DEVWS_ENV_FILE="$HOME/.tmuxinator/.env"

reset_env() {
  cat > "$ENV_FILE" <<'EOF'
# Agents offered by devws.
DEVWS_AGENTS="claude codex"
DEVWS_DEFAULT_AGENT="claude"

# Editors offered by devws.
DEVWS_EDITORS="nvim vim"
EOF
}

register() {
  "$SCRIPTS/register_agent.sh" "$@"
}

# The registry as the scripts see it: the list, then each name's kind and folder.
registry() {
  (
    . "$DEVWS_ENV_FILE"
    . "$SCRIPTS/agents.sh"
    printf '%s|%s' "$DEVWS_AGENTS" "$DEVWS_DEFAULT_AGENT"
    for agent in $DEVWS_AGENTS; do
      printf '|%s=%s:%s' "$agent" "$(agent_kind "$agent")" "$(agent_home "$agent")"
    done
  )
}

test_add_registers_a_name_with_its_folder() {
  reset_env
  register add work claude "$HOME/.claude-work" >/dev/null || fail "add failed"
  [ "$(registry)" = "claude codex work|claude|claude=claude:$HOME/.claude|codex=codex:$HOME/.codex|work=claude:$HOME/.claude-work" ] ||
    fail "unexpected registry after add: $(registry)"
  grep -qx 'DEVWS_AGENT_work="claude ~/.claude-work"' "$ENV_FILE" || fail "the folder is not written under ~: $(cat "$ENV_FILE")"
  grep -qx 'DEVWS_EDITORS="nvim vim"' "$ENV_FILE" || fail "add changed other settings"
  [ -L "$DEVWS_ENV_FILE" ] || fail "add replaced the .env symlink"
}

test_add_rejects_bad_input() {
  reset_env
  register add work claude "$HOME/.claude-work" >/dev/null
  ! register add work claude "$HOME/.claude" 2>/dev/null || fail "a taken name was registered"
  ! register add codex claude "$HOME/.claude" 2>/dev/null || fail "a listed name was registered"
  ! register add my-work claude "$HOME/.claude-work" 2>/dev/null || fail "a name with a dash was registered"
  ! register add other vim "$HOME/.claude" 2>/dev/null || fail "an unknown kind was registered"
  ! register add other claude "$HOME/.missing" 2>/dev/null || fail "a missing folder was registered"
  [ "$(registry)" = "claude codex work|claude|claude=claude:$HOME/.claude|codex=codex:$HOME/.codex|work=claude:$HOME/.claude-work" ] ||
    fail "a rejected add changed the registry: $(registry)"
}

test_remove_drops_a_name() {
  reset_env
  register add work claude "~/.claude-work" >/dev/null
  register remove codex >/dev/null || fail "remove failed"
  register remove claude >/dev/null || fail "removing the default failed"
  [ "$(registry)" = "work|work|work=claude:$HOME/.claude-work" ] || fail "unexpected registry after remove: $(registry)"
  ! register remove work 2>/dev/null || fail "the last agent was removed"
  ! register remove ghost 2>/dev/null || fail "an unknown agent was removed"
  local output
  output="$(register remove work 2>&1)" || true
  grep -q "last agent" <<< "$output" || fail "removing the last agent does not say why: $output"
  register add claude claude "$HOME/.claude" >/dev/null
  register remove work >/dev/null
  ! grep -q "DEVWS_AGENT_work" "$ENV_FILE" || fail "a removed agent's entry stayed"
  [ -L "$DEVWS_ENV_FILE" ] || fail "remove replaced the .env symlink"
}

test_add_registers_a_name_with_its_folder
test_add_rejects_bad_input
test_remove_drops_a_name

printf 'Register agent tests passed.\n'
