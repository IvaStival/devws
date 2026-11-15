#!/usr/bin/env bash
# agent_interrupt.sh sweep: an agent whose turn was interrupted (no Stop hook
# comes) leaves "working"; one that works, waits or got a new prompt does not.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="${TMPDIR:-/tmp}"
TEST_ROOT="$(mktemp -d "${tmp_dir%/}/devws-interrupt.XXXXXX")"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-interrupt-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Agent interrupt test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home" "$TEST_ROOT/project"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
chmod +x "$TEST_ROOT/bin/tmux"
ln -s "$ROOT/config/tmuxinator" "$TEST_ROOT/home/.tmuxinator"
: > "$TEST_ROOT/devws.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT" \
  DEVWS_NOTIFY=0 DEVWS_ENV_FILE="$TEST_ROOT/devws.env"
unset TMUX
SCRIPTS="$HOME/.tmuxinator"

tmux new-session -d -s dev-work -x 200 -y 50 -c "$TEST_ROOT/project" 'sleep 600'
tmux set-option -p -t dev-work: @devws_role agent
PANE="$(tmux display-message -p -t dev-work: '#{pane_id}')"

# Transcript lines in each program's own shape, cut to the fields that count.
claude_prompt='{"type":"user","message":{"role":"user","content":"fix it"}}'
claude_tool_use='{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash"}]}}'
claude_tool_result='{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}'
claude_interrupt='{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}'
claude_after='{"type":"system","subtype":"turn_duration"}'
codex_started='{"type":"event_msg","payload":{"type":"task_started"}}'
codex_tool='{"type":"response_item","payload":{"type":"custom_tool_call"}}'
codex_tokens='{"type":"event_msg","payload":{"type":"token_count"}}'
codex_aborted='{"type":"event_msg","payload":{"type":"turn_aborted","reason":"interrupted"}}'
codex_settings='{"type":"event_msg","payload":{"type":"thread_settings_applied"}}'

# <state> <line…>: the pane in <state>, its transcript the lines given.
given() {
  local state="$1"
  shift
  printf '%s\n' "$@" > "$TEST_ROOT/transcript.jsonl"
  "$SCRIPTS/agent_status.sh" dev-work "$state" "$PANE" >/dev/null 2>&1
  tmux set-option -p -t "$PANE" @devws_transcript "$TEST_ROOT/transcript.jsonl"
}

sweep() {
  "$SCRIPTS/agent_interrupt.sh" sweep || fail "the sweep failed"
}

pane_state() {
  tmux show-options -pqv -t "$PANE" @devws_pane_state
}

expect_state() {
  [ "$(pane_state)" = "$1" ] || fail "$2: the pane is '$(pane_state)', not '$1'"
}

reset_pane() {
  "$SCRIPTS/agent_status.sh" dev-work idle "$PANE" >/dev/null 2>&1
  tmux set-option -p -u -t "$PANE" @devws_interrupt_seen 2>/dev/null || true
  tmux set-option -p -u -t "$PANE" @devws_transcript 2>/dev/null || true
}

test_an_interrupted_claude_turn_turns_idle_on_the_second_sweep() {
  reset_pane
  given working "$claude_prompt" "$claude_tool_use" "$claude_tool_result" "$claude_interrupt" "$claude_after"
  sweep
  expect_state working "one sweep is not enough (the prompt may not be written yet)"
  sweep
  expect_state "" "an interrupted Claude turn"
  [ "$(tmux show-option -qv -t '=dev-work:' @devws_agent_state)" = "" ] ||
    fail "the workspace still sums the interrupted agent up as working"
}

test_an_interrupted_codex_turn_turns_idle() {
  reset_pane
  given working "$codex_started" "$codex_tool" "$codex_tokens" "$codex_aborted" "$codex_settings"
  sweep
  sweep
  expect_state "" "an interrupted Codex turn"
}

test_a_new_prompt_after_the_interrupt_keeps_working() {
  reset_pane
  given working "$codex_started" "$codex_aborted" "$codex_settings" "$codex_started" "$codex_tool"
  sweep
  sweep
  expect_state working "a Codex turn started after the interrupt"
  reset_pane
  given working "$claude_prompt" "$claude_interrupt" "$claude_after" "$claude_prompt" "$claude_tool_use"
  sweep
  sweep
  expect_state working "a Claude prompt sent after the interrupt"
}

test_a_prompt_written_between_sweeps_keeps_working() {
  reset_pane
  given working "$claude_prompt" "$claude_interrupt"
  sweep
  printf '%s\n' "$claude_prompt" >> "$TEST_ROOT/transcript.jsonl"
  sweep
  expect_state working "a prompt that reached the transcript between two sweeps"
  [ -z "$(tmux show-options -pqv -t "$PANE" @devws_interrupt_seen)" ] ||
    fail "the sweep kept a pending interrupt the transcript no longer shows"
}

test_a_turn_still_working_is_left_alone() {
  reset_pane
  given working "$claude_prompt" "$claude_tool_use" "$claude_tool_result" "$claude_tool_use"
  sweep
  sweep
  expect_state working "a Claude turn in progress"
  reset_pane
  # A long turn: its start has left the transcript's tail.
  given working "$codex_tool" "$codex_tokens" "$codex_tool"
  sweep
  sweep
  expect_state working "a Codex turn whose start is out of the tail"
}

test_other_states_and_missing_transcripts_are_left_alone() {
  reset_pane
  given waiting "$claude_prompt" "$claude_interrupt"
  sweep
  sweep
  expect_state waiting "a waiting agent"
  given done "$codex_started" "$codex_aborted"
  sweep
  sweep
  expect_state done "a done agent"
  reset_pane
  "$SCRIPTS/agent_status.sh" dev-work working "$PANE" >/dev/null 2>&1
  sweep
  sweep
  expect_state working "a pane without a transcript"
  tmux set-option -p -t "$PANE" @devws_transcript "$TEST_ROOT/missing.jsonl"
  sweep
  sweep
  expect_state working "a pane whose transcript is missing"
}

test_a_pending_interrupt_does_not_outlive_the_working_state() {
  reset_pane
  given working "$claude_prompt" "$claude_interrupt"
  sweep
  # The agent leaves working another way (a Stop arrives) before the second sweep.
  "$SCRIPTS/agent_status.sh" dev-work done "$PANE" >/dev/null 2>&1
  sweep
  [ -z "$(tmux show-options -pqv -t "$PANE" @devws_interrupt_seen)" ] ||
    fail "a pending interrupt stayed on a pane that is no longer working"
}

test_an_interrupted_claude_turn_turns_idle_on_the_second_sweep
test_an_interrupted_codex_turn_turns_idle
test_a_new_prompt_after_the_interrupt_keeps_working
test_a_prompt_written_between_sweeps_keeps_working
test_a_turn_still_working_is_left_alone
test_other_states_and_missing_transcripts_are_left_alone
test_a_pending_interrupt_does_not_outlive_the_working_state

printf 'Agent interrupt tests passed.\n'
