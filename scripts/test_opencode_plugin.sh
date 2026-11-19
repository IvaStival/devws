#!/usr/bin/env bash
# devws's opencode plugin (agents/30-opencode/devws_plugin.js): opencode's events reach
# devws as the hook payloads Claude Code and Codex send, a provider error
# becomes a structured startup error, and edits reach the editor's hook.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/devws-opencode-plugin.XXXXXX")" && pwd -P)"
PLUGIN="$ROOT/config/tmuxinator/agents/30-opencode/devws_plugin.js"

cleanup() { rm -rf "$TEST_ROOT"; }
trap cleanup EXIT

fail() {
  printf 'opencode plugin test failed: %s\n' "$*" >&2
  exit 1
}

command -v node >/dev/null 2>&1 || {
  printf 'opencode plugin tests skipped: no node\n'
  exit 0
}

# The plugin runs the scripts beside it: stubs that record what they get.
SCRIPTS="$TEST_ROOT/scripts"
# It is a module's file: devws's scripts are two folders up.
mkdir -p "$SCRIPTS/agents/30-opencode" "$TEST_ROOT/bin"
cp "$PLUGIN" "$SCRIPTS/agents/30-opencode/plugin.mjs"
for stub in agent_state.sh agent_follow_hook.sh; do
  printf '#!/bin/sh\n{ printf "%s %%s " "$*"; cat; echo; } >> "%s/calls"\n' "$stub" "$TEST_ROOT" > "$SCRIPTS/$stub"
  chmod +x "$SCRIPTS/$stub"
done
printf '#!/bin/sh\necho "tmux $*" >> "%s/calls"\n' "$TEST_ROOT" > "$TEST_ROOT/bin/tmux"
chmod +x "$TEST_ROOT/bin/tmux"

# <events JSON>: each is sent to the plugin's event hook, in order.
drive() {
  : > "$TEST_ROOT/calls"
  PATH="$TEST_ROOT/bin:$PATH" TMUX_PANE=%9 DEVWS_OPENCODE_DB=/data/opencode.db node --input-type=module -e "
    const { DevwsPlugin } = await import('$SCRIPTS/agents/30-opencode/plugin.mjs');
    const hooks = await DevwsPlugin({ directory: '/work/app' });
    for (const step of $1) {
      if (step.tool) {
        await hooks['tool.execute.before']({ tool: step.tool, callID: 'c1' }, { args: { filePath: step.file } });
        await hooks['tool.execute.after']({ tool: step.tool, callID: 'c1' }, {});
      } else {
        await hooks.event({ event: step });
      }
    }"
}

test_a_turn_reports_working_then_done_with_its_conversation() {
  drive '[
    {"type":"session.created","properties":{"info":{"id":"ses_1"}}},
    {"type":"session.status","properties":{"sessionID":"ses_1","status":{"type":"busy"}}},
    {"type":"session.idle","properties":{"sessionID":"ses_1"}}]'
  local events
  events="$(grep -o '"hook_event_name":"[A-Za-z]*"' "$TEST_ROOT/calls" | cut -d'"' -f4 | xargs)"
  [ "$events" = "SessionStart UserPromptSubmit Stop" ] || fail "events: $events"
  grep -qF '"transcript_path":"/data/opencode.db#ses_1"' "$TEST_ROOT/calls" || fail "no conversation: $(cat "$TEST_ROOT/calls")"
}

test_a_subagents_session_is_not_the_agents_turn() {
  drive '[
    {"type":"session.created","properties":{"info":{"id":"ses_1"}}},
    {"type":"session.created","properties":{"info":{"id":"ses_child","parentID":"ses_1"}}},
    {"type":"session.idle","properties":{"sessionID":"ses_child"}}]'
  ! grep -q '"Stop"' "$TEST_ROOT/calls" || fail "a subagent's idle marked the agent done"
}

test_a_permission_prompt_waits_on_the_user() {
  drive '[
    {"type":"session.created","properties":{"info":{"id":"ses_1"}}},
    {"type":"permission.asked","properties":{"sessionID":"ses_1"}}]'
  grep -q '"PermissionRequest"' "$TEST_ROOT/calls" || fail "no PermissionRequest: $(cat "$TEST_ROOT/calls")"
}

test_a_provider_error_is_a_structured_startup_error() {
  local error expected
  for error in '{"name":"APIError","data":{"statusCode":402,"message":"x"}}=insufficient_quota' \
      '{"name":"APIError","data":{"statusCode":429}}=rate_limit' \
      '{"name":"APIError","data":{"statusCode":404}}=model_not_found' \
      '{"name":"ProviderAuthError","data":{}}=authentication_error'; do
    expected="${error##*=}"
    drive "[{\"type\":\"session.created\",\"properties\":{\"info\":{\"id\":\"ses_1\"}}},
      {\"type\":\"session.error\",\"properties\":{\"sessionID\":\"ses_1\",\"error\":${error%=*}}}]"
    grep -qF "tmux set-option -p -t %9 @devws_startup_error {\"type\":\"error\",\"error\":{\"code\":\"$expected\"}}" "$TEST_ROOT/calls" ||
      fail "expected $expected: $(cat "$TEST_ROOT/calls")"
  done
}

test_an_edit_is_snapshot_before_and_shown_after() {
  drive '[{"tool":"edit","file":"/work/app/main.rs"}, {"tool":"read","file":"/work/app/x"}]'
  [ "$(grep -c '^agent_follow_hook.sh' "$TEST_ROOT/calls")" -eq 2 ] || fail "follow calls: $(cat "$TEST_ROOT/calls")"
  grep -q '^agent_follow_hook.sh opencode pre ' "$TEST_ROOT/calls" || fail "no snapshot before the edit"
  grep -qF '"file_path":"/work/app/main.rs"' "$TEST_ROOT/calls" || fail "the edited file was not named"
}

test_a_turn_reports_working_then_done_with_its_conversation
test_a_subagents_session_is_not_the_agents_turn
test_a_permission_prompt_waits_on_the_user
test_a_provider_error_is_a_structured_startup_error
test_an_edit_is_snapshot_before_and_shown_after

printf 'opencode plugin tests passed\n'
