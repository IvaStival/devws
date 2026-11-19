#!/usr/bin/env bash
# The devws menu's ◆ OpenCode screen (agents/30-opencode/menu.sh): its lines turn the
# switches on and off, pick the groups' models and register the account,
# through devagentics opencode and register_agent.sh.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/devws-opencode-menu.XXXXXX")" && pwd -P)"
SCRIPTS="$ROOT/config/tmuxinator"

cleanup() { rm -rf "$TEST_ROOT"; }
trap cleanup EXIT

fail() {
  printf 'opencode menu test failed: %s\n' "$*" >&2
  exit 1
}

AGENTIC="$ROOT/agentic/target/debug/devagentics"
[ -x "$AGENTIC" ] || {
  printf 'opencode menu tests skipped: devagentics is not built (cargo build in agentic/)\n'
  exit 0
}

export HOME="$TEST_ROOT/home" DEVAGENTICS_HOME="$TEST_ROOT/agentics"
export DEVWS_ENV_FILE="$TEST_ROOT/env" DEVWS_AGENTIC_COMMAND="$AGENTIC"
mkdir -p "$HOME/.claude" "$HOME/.codex" "$TEST_ROOT/bin"
printf 'DEVWS_AGENTS="claude codex"\nDEVWS_DEFAULT_AGENT="claude"\n' > "$DEVWS_ENV_FILE"
# devagentics asks devws an account's program.
printf '#!/bin/sh\n[ "$1 $2" = "agent kind" ] && exec "%s/agent_cli.sh" kind "$3"\nexit 2\n' "$SCRIPTS" > "$TEST_ROOT/bin/devws"
# opencode lists the account's models; logging in is recorded.
printf '#!/bin/sh\ncase "$1" in models) printf "opencode/deepseek-v4.1-flash\\nopencode/glm-5.3\\n" ;; auth) echo "login $XDG_DATA_HOME" >> "%s/opencode.log" ;; esac\n' "$TEST_ROOT" > "$TEST_ROOT/bin/opencode"
chmod +x "$TEST_ROOT/bin/devws" "$TEST_ROOT/bin/opencode"
export PATH="$TEST_ROOT/bin:$PATH"

# The picker answers from $TEST_ROOT/answers, a line per question: the key of
# the line to choose (its first word), or the whole value for a model. No
# answer left is Esc.
cat > "$TEST_ROOT/picker" <<STUB
#!/usr/bin/env bash
answer="\$(head -n 1 "$TEST_ROOT/answers")"
sed -i.bak 1d "$TEST_ROOT/answers"
[ -n "\$answer" ] || exit 130
grep -m 1 -e "^\$answer " -e "^\$answer\\\$" || exit 130
STUB
chmod +x "$TEST_ROOT/picker"
export DEVWS_PICKER="$TEST_ROOT/picker"

# <answers…>: the screen, answered so, then left.
screen() {
  printf '%s\n' "$@" > "$TEST_ROOT/answers"
  "$SCRIPTS/agents/30-opencode/menu.sh" </dev/null >/dev/null 2>&1
}

test_the_switches_turn_on_and_off() {
  screen floor global
  [ "$("$AGENTIC" opencode show | head -2 | tr '\t\n' ' ')" = "global on floor on - " ] ||
    fail "after floor and global: $("$AGENTIC" opencode show)"
  screen global
  "$AGENTIC" opencode show | grep -qx 'floor	ignored	-' || fail "global off did not hold the floor off: $("$AGENTIC" opencode show)"
  screen floor
  "$AGENTIC" opencode show | grep -qx 'floor	off	-' || fail "an ignored floor did not turn off"
}

test_a_groups_model_is_picked_from_the_accounts_models() {
  screen floor-model opencode/deepseek-v4.1-flash
  "$AGENTIC" opencode show | grep -qx 'floor	off	opencode/deepseek-v4.1-flash' ||
    fail "floor model: $("$AGENTIC" opencode show)"
}

test_the_account_is_registered_then_logged_in() {
  screen account
  grep -qx 'DEVWS_AGENT_opencode="opencode ~/.local/share"' "$DEVWS_ENV_FILE" ||
    fail "the account was not registered: $(cat "$DEVWS_ENV_FILE")"
  grep -qx "login $HOME/.local/share" "$TEST_ROOT/opencode.log" || fail "no login in the account's folder"
}

test_the_switches_turn_on_and_off
test_a_groups_model_is_picked_from_the_accounts_models
test_the_account_is_registered_then_logged_in

printf 'opencode menu tests passed\n'
