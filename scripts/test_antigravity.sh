#!/usr/bin/env bash
# Antigravity (agy) in devws: the devws plugin's hooks (agents/40-antigravity/hook.sh),
# its installer and folder trust (plugin.sh), its MCP server
# (mcp.sh), the Gemini quota its status line records, and the
# month's tokens the header shows.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-antigravity.XXXXXX")"
SCRIPTS_SRC="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-antigravity-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Antigravity test failed: %s\n' "$*" >&2
  exit 1
}

mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator" "$TEST_ROOT/project" "$TEST_ROOT/tmp"
printf '%s\n' '#!/usr/bin/env bash' "exec \"$REAL_TMUX\" -L \"$SOCKET\" -f /dev/null \"\$@\"" > "$TEST_ROOT/bin/tmux"
chmod +x "$TEST_ROOT/bin/tmux"
for script in "$SCRIPTS_SRC"/*.sh "$SCRIPTS_SRC"/agents "$SCRIPTS_SRC"/*.pl "$SCRIPTS_SRC"/*.md "$SCRIPTS_SRC"/*.tsv; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
printf 'DEVWS_AGENTS="gemini"\nDEVWS_AGENT_gemini="antigravity ~/.gemini/antigravity-cli"\n' > "$TEST_ROOT/home/.tmuxinator/.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT/tmp"
export DEVWS_NOTIFY_COMMAND=true DEVWS_TOKEN_CACHE_DISABLE=1 DEVWS_OFFERED_PROGRAMS=""
# What the workspace's scripts find in their environment, as from .env.
export DEVWS_AGENTS=gemini DEVWS_AGENT_gemini="antigravity ~/.gemini/antigravity-cli"
unset TMUX DEVWS_AGENT DEVWS_AGY_MCP_CONFIG DEVWS_AGY_PROMPT_FILE
SCRIPTS="$HOME/.tmuxinator"

# An agent pane in a workspace, as agent_runner.sh marks it.
tmux new-session -d -s work -x 120 -y 40 -c "$TEST_ROOT/project" 'sleep 600'
PANE="$(tmux display-message -p -t work: '#{pane_id}')"
tmux set-option -p -t "$PANE" @devws_role agent

hook() {
  "$SCRIPTS/agents/40-antigravity/hook.sh" "$@"
}

in_pane() {
  DEVWS_AGENT=gemini TMUX_PANE="$PANE" TMUX="/tmp/test,1,0" "$@"
}

state() {
  tmux show-options -pqv -t "$PANE" @devws_pane_state
}

test_outside_a_devws_pane_the_hooks_change_nothing() {
  [ "$(printf '{}' | hook PreToolUse)" = '{"decision":"ask"}' ] || fail "a tool call outside devws was not left to agy"
  [ "$(printf '{}' | hook SessionStart)" = '{}' ] || fail "a session outside devws got devws's prompt"
  [ "$(printf '{}' | TMUX_PANE="$PANE" hook Stop)" = '{}' ] || fail "an agy devws did not start reported to it"
  [ -z "$(state)" ] || fail "an agy devws did not start changed the pane's state"
}

test_the_hooks_report_the_turn_and_inject_the_editor_prompt() {
  local transcript="$TEST_ROOT/transcript_full.jsonl" answer
  : > "$transcript"
  printf 'Open files with editor_open.sh.\n' > "$TEST_ROOT/prompt.md"
  answer="$(printf '{"conversationId":"c1","transcriptPath":"%s","modelName":"gemini-4-carbon-medium"}' "$transcript" |
    in_pane env DEVWS_AGY_PROMPT_FILE="$TEST_ROOT/prompt.md" "$SCRIPTS/agents/40-antigravity/hook.sh" SessionStart)"
  [ "$(printf '%s' "$answer" | jq -r '.injectSteps[0].systemMessage.systemMessage')" = 'Open files with editor_open.sh.' ] ||
    fail "the editor prompt was not injected: $answer"
  # Idle is no state at all (agent_status.sh).
  [ -z "$(state)" ] || fail "a new session is not idle: $(state)"
  [ "$(tmux show-options -pqv -t "$PANE" @devws_transcript)" = "$transcript" ] || fail "the transcript was not recorded"
  [ "$(tmux show-options -pqv -t "$PANE" @devws_model)" = gemini-4-carbon-medium ] || fail "the model was not recorded"

  [ "$(printf '{}' | in_pane "$SCRIPTS/agents/40-antigravity/hook.sh" PreInvocation)" = '{}' ] || fail "PreInvocation answered something"
  [ "$(state)" = working ] || fail "a turn is not working: $(state)"
  printf '{}' | in_pane "$SCRIPTS/agents/40-antigravity/hook.sh" Stop >/dev/null
  [ "$(state)" = done ] || fail "a finished turn is not done: $(state)"
}

# What may run without asking is in the session's settings (agent_args.rs):
# the hook leaves every call to agy's own permission rules.
test_every_tool_call_is_left_to_agy_s_permissions() {
  local call
  for call in '{"toolCall":{"name":"run_command","args":{"CommandLine":"devagentics report done"}}}' \
    '{"toolCall":{"name":"call_mcp_tool","args":{"ServerName":"devws_devagentics","ToolName":"task_status"}}}'; do
    [ "$(printf '%s' "$call" | in_pane env DEVWS_AGY_MCP_CONFIG="$TEST_ROOT/mcp.json" \
      "$SCRIPTS/agents/40-antigravity/hook.sh" PreToolUse)" = '{"decision":"ask"}' ] || fail "the hook decided a call: $call"
  done
}

test_the_plugin_is_installed_once_and_a_folder_trusted_once() {
  local config="$TEST_ROOT/agy-config" home="$TEST_ROOT/agy-home" before
  DEVWS_AGY_CONFIG="$config" "$SCRIPTS/agents/40-antigravity/plugin.sh" install || fail "the plugin did not install"
  jq -e '.devws.PreToolUse[0].matcher == "" and (.devws.Stop[0].command | endswith("Stop"))' "$config/plugins/devws/hooks.json" >/dev/null ||
    fail "the hooks are not agy's format: $(cat "$config/plugins/devws/hooks.json")"
  [ "$(jq -r .name "$config/plugins/devws/plugin.json")" = devws ] || fail "the plugin has no manifest"
  before="$(stat -c %Y.%N "$config/plugins/devws/hooks.json" 2>/dev/null || stat -f %m "$config/plugins/devws/hooks.json")"
  sleep 1
  DEVWS_AGY_CONFIG="$config" "$SCRIPTS/agents/40-antigravity/plugin.sh" install
  [ "$(stat -c %Y.%N "$config/plugins/devws/hooks.json" 2>/dev/null || stat -f %m "$config/plugins/devws/hooks.json")" = "$before" ] ||
    fail "an unchanged plugin was written again"
  DEVWS_AGY_CONFIG="$config" "$SCRIPTS/agents/40-antigravity/plugin.sh" remove
  [ ! -e "$config/plugins/devws" ] || fail "the plugin was not removed"

  mkdir -p "$home"
  printf '{"colorScheme":"dark","trustedWorkspaces":["/elsewhere"]}' > "$home/settings.json"
  DEVWS_AGY_HOME="$home" "$SCRIPTS/agents/40-antigravity/plugin.sh" trust "$TEST_ROOT/project"
  DEVWS_AGY_HOME="$home" "$SCRIPTS/agents/40-antigravity/plugin.sh" trust "$TEST_ROOT/project"
  [ "$(jq -c .trustedWorkspaces "$home/settings.json")" = "[\"/elsewhere\",\"$(cd "$TEST_ROOT/project" && pwd -P)\"]" ] ||
    fail "the folder was not trusted once: $(cat "$home/settings.json")"
  [ "$(jq -r .colorScheme "$home/settings.json")" = dark ] || fail "trusting lost agy's other settings"
}

test_the_mcp_server_is_the_leader_s_or_has_no_tools() {
  local answers
  answers="$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' |
    "$SCRIPTS/agents/40-antigravity/mcp.sh")"
  [ "$(printf '%s\n' "$answers" | wc -l | tr -d ' ')" = 2 ] || fail "a notification was answered: $answers"
  [ "$(printf '%s\n' "$answers" | sed -n 1p | jq -r .result.protocolVersion)" = 2025-06-18 ] || fail "no handshake: $answers"
  [ "$(printf '%s\n' "$answers" | sed -n 2p | jq -c .result.tools)" = '[]' ] || fail "tools without the leader: $answers"

  printf '%s\n' '#!/usr/bin/env bash' 'printf "server %s\n" "$*"' > "$TEST_ROOT/bin/fake-devagentics"
  chmod +x "$TEST_ROOT/bin/fake-devagentics"
  printf '{"mcpServers":{"devagentics":{"command":"%s","args":["mcp","feat x"]}}}' "$TEST_ROOT/bin/fake-devagentics" > "$TEST_ROOT/mcp.json"
  [ "$(DEVWS_AGY_MCP_CONFIG="$TEST_ROOT/mcp.json" "$SCRIPTS/agents/40-antigravity/mcp.sh" </dev/null)" = "server mcp feat x" ] ||
    fail "the leader's server was not the one run"
}

test_the_status_line_records_the_gemini_quota() {
  printf '{"product":"antigravity","model":{"id":"Gemini 4 Carbon (Medium)"},"quota":{"gemini-5h":{"remaining_fraction":0.9887},"gemini-weekly":{"remaining_fraction":0.5}}}' |
    in_pane "$SCRIPTS/agent_statusline.sh" >/dev/null
  read -r session week at <<<"$(tmux show-option -gqv @devws_limits_gemini)"
  [ "$session $week" = "1 50" ] && [ -n "$at" ] || fail "the quota was not recorded as % used: $(tmux show-option -gqv @devws_limits_gemini)"
  # shellcheck source=/dev/null
  . "$SCRIPTS/.env"
  # shellcheck source=/dev/null
  . "$SCRIPTS/agents.sh"
  [ "$(agent_limits gemini)" = "1 50 $at" ] || fail "agents.sh does not read Antigravity's limits"
  [ "$(tmux show-options -pqv -t "$PANE" @devws_model)" = gemini-4-carbon-medium ] ||
    fail "the status line's display name replaced the model id"
}

test_the_header_shows_the_month_s_tokens() {
  local logs="$HOME/.gemini/antigravity-cli/brain/c1/.system_generated/logs" now
  now="$(date +%s)"
  mkdir -p "$logs"
  {
    printf '{"step_index":0,"type":"USER_INPUT","content":"hello"}\n'
    printf '{"step_index":1,"type":"PLANNER_RESPONSE","created_at":"%s","model":"gemini-4-carbon-medium","input_tokens":1000,"cache_read_tokens":500,"output_tokens":100}\n' "$(date -u -d "@$now" +%FT%TZ 2>/dev/null || date -u -r "$now" +%FT%TZ)"
    printf '{"step_index":2,"type":"PLANNER_RESPONSE","created_at":"%s","model":"gemini-4-carbon-medium","input_tokens":200,"cache_read_tokens":1200,"output_tokens":0}\n' "$(date -u -d "@$now" +%FT%TZ 2>/dev/null || date -u -r "$now" +%FT%TZ)"
  } > "$logs/transcript_full.jsonl"
  cp "$logs/transcript_full.jsonl" "$logs/transcript.jsonl"
  [ "$("$SCRIPTS/header_tokens.sh" costs)" = "gemini 0 0 3000" ] || fail "the month's tokens: $("$SCRIPTS/header_tokens.sh" costs)"
  [ "$("$SCRIPTS/header_tokens.sh" session "$logs/transcript_full.jsonl")" = "conv 3k · ctx 1k" ] ||
    fail "the conversation: $("$SCRIPTS/header_tokens.sh" session "$logs/transcript_full.jsonl")"
  [ "$("$SCRIPTS/header_tokens.sh" usage "$logs/transcript_full.jsonl")" = "1200 100 1700 0 1" ] ||
    fail "its usage: $("$SCRIPTS/header_tokens.sh" usage "$logs/transcript_full.jsonl")"
  [ "$("$SCRIPTS/header_tokens.sh" model "$logs/transcript_full.jsonl")" = gemini-4-carbon-medium ] || fail "its model"
  "$SCRIPTS/header_tokens.sh" costs > "$TEST_ROOT/costs"
  case "$("$SCRIPTS/header_agents.sh" "$TEST_ROOT/costs")" in
    *"5h"*"wk"*gemini*"3k tok"*) ;;
    *) fail "the header row: $("$SCRIPTS/header_agents.sh" "$TEST_ROOT/costs")" ;;
  esac
}

# agy runs no hook when Esc stops a turn: the sweep reads what its status
# line said it is (@devws_agy_state).
test_an_interrupted_turn_turns_idle_once_agy_says_it_is_idle() {
  local pane running
  tmux new-session -d -s esc -x 120 -y 30 'sleep 600'
  pane="$(tmux display-message -p -t esc: '#{pane_id}')"
  tmux new-session -d -s busy -x 120 -y 30 'sleep 600'
  running="$(tmux display-message -p -t busy: '#{pane_id}')"
  for each in "$pane" "$running"; do
    tmux set-option -p -t "$each" @devws_role agent
    tmux set-option -p -t "$each" @devws_transcript "$HOME/.gemini/antigravity-cli/brain/c1/.system_generated/logs/transcript_full.jsonl"
    tmux set-option -p -t "$each" @devws_pane_state working
  done
  printf '{"product":"antigravity","agent_state":"idle"}' | DEVWS_AGENT=gemini TMUX_PANE="$pane" TMUX="/tmp/test,1,0" "$SCRIPTS/agent_statusline.sh" >/dev/null
  printf '{"product":"antigravity","agent_state":"working"}' | DEVWS_AGENT=gemini TMUX_PANE="$running" TMUX="/tmp/test,1,0" "$SCRIPTS/agent_statusline.sh" >/dev/null
  [ "$(tmux show-options -pqv -t "$pane" @devws_agy_state)" = idle ] || fail "the status line did not record agy's state"
  "$SCRIPTS/agent_interrupt.sh" sweep
  [ "$(tmux show-options -pqv -t "$pane" @devws_pane_state)" = working ] || fail "one sighting ended the turn"
  "$SCRIPTS/agent_interrupt.sh" sweep
  [ -z "$(tmux show-options -pqv -t "$pane" @devws_pane_state)" ] || fail "an interrupted turn is still working"
  [ "$(tmux show-options -pqv -t "$running" @devws_pane_state)" = working ] || fail "a running turn was taken for interrupted"
}

test_outside_a_devws_pane_the_hooks_change_nothing
test_an_interrupted_turn_turns_idle_once_agy_says_it_is_idle
test_the_hooks_report_the_turn_and_inject_the_editor_prompt
test_every_tool_call_is_left_to_agy_s_permissions
test_the_plugin_is_installed_once_and_a_folder_trusted_once
test_the_mcp_server_is_the_leader_s_or_has_no_tools
test_the_status_line_records_the_gemini_quota
test_the_header_shows_the_month_s_tokens

printf 'Antigravity tests passed\n'
