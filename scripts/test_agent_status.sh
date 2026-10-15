#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="${TMPDIR:-/tmp}"
TEST_ROOT="$(mktemp -d "${tmp_dir%/}/devws-status.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-status-test-$$"
CLIENT_SOCKET="devws-status-client-$$"

cleanup() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server 2>/dev/null || true
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Agent status test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server.
# The agents are stubs that record the arguments devws starts them with.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home" "$TEST_ROOT/project" "$TEST_ROOT/agents"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
for agent in claude codex; do
  cat > "$TEST_ROOT/bin/$agent" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" > "$TEST_ROOT/agents/$agent.args"
sleep 600
EOF
done
chmod +x "$TEST_ROOT/bin/"*
ln -s "$SCRIPTS" "$TEST_ROOT/home/.tmuxinator"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX
# The scripts run through the link, so the tickers' command lines name this
# test's directory and pgrep never counts a real workspace's ticker.
SCRIPTS="$HOME/.tmuxinator"

wait_until() {
  local attempt
  for attempt in $(seq 1 30); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

# A workspace's agent pane, and the state formats tmux.conf defines (with the
# colors they use).
new_workspace() {
  tmux new-session -d -s "$1" -x 200 -y 50 -c "$TEST_ROOT/project" 'sleep 600'
  tmux set-option -t "$1" @devws_agent claude
  tmux set-option -t "$1" @devws_editor nvim
  tmux set-option -p -t "$1:" @devws_role agent
}
new_workspace dev-work
AGENT_PANE="$(tmux display-message -p -t dev-work: '#{pane_id}')"
grep -E '^(color_[a-z_]+=|set -g @devws_agent_)' "$ROOT/config/tmux/tmux.conf" > "$TEST_ROOT/formats.conf"
tmux source-file "$TEST_ROOT/formats.conf"

state_of() {
  tmux show-option -qv -t "=$1:" @devws_agent_state
}

state_is() {
  [ "$(state_of dev-work)" = "$1" ]
}

# Sends a hook payload the way the agents do: from the agent pane.
hook() {
  printf '%s' "$1" | TMUX_PANE="$AGENT_PANE" "$SCRIPTS/agent_state.sh"
}

claude_event() {
  hook "{\"session_id\":\"s\",\"cwd\":\"$TEST_ROOT/project\",\"hook_event_name\":\"$1\"${2:+,$2}}"
}

test_claude_events_set_the_state() {
  claude_event SessionStart '"source":"startup"'
  state_is "" || fail "SessionStart is not idle: $(state_of dev-work)"
  claude_event UserPromptSubmit '"prompt":"fix it"'
  state_is working || fail "a prompt did not set working"
  claude_event Notification '"notification_type":"permission_prompt","message":"Claude needs your permission to use Bash"'
  state_is waiting || fail "a permission prompt did not set waiting"
  claude_event PostToolUse '"tool_name":"Bash","tool_input":{"command":"ls"}'
  state_is working || fail "an approved tool did not set working again"
  claude_event Notification '"notification_type":"elicitation_dialog","message":"Choose"'
  state_is waiting || fail "a question did not set waiting"
  claude_event PreToolUse '"tool_name":"Read","tool_input":{}'
  claude_event Notification '"notification_type":"idle_prompt","message":"Claude is waiting for your input"'
  state_is working || fail "an idle reminder changed the state"
  claude_event Stop '"stop_hook_active":false'
  state_is done || fail "Stop did not set done"
  claude_event SessionEnd '"reason":"exit"'
  state_is "" || fail "SessionEnd did not set idle"
}

test_codex_events_set_the_state() {
  local base="\"session_id\":\"s\",\"turn_id\":\"t\",\"cwd\":\"$TEST_ROOT/project\",\"model\":\"gpt-5\""
  hook "{$base,\"hook_event_name\":\"UserPromptSubmit\",\"prompt\":\"go\"}"
  state_is working || fail "a Codex prompt did not set working"
  hook "{$base,\"hook_event_name\":\"PermissionRequest\",\"tool_name\":\"shell\",\"tool_input\":{}}"
  state_is waiting || fail "a Codex permission request did not set waiting"
  hook "{$base,\"hook_event_name\":\"PreToolUse\",\"tool_name\":\"shell\",\"tool_input\":{}}"
  state_is working || fail "a Codex tool did not set working"
  hook "{$base,\"hook_event_name\":\"Stop\",\"stop_hook_active\":false}"
  state_is done || fail "a Codex Stop did not set done"
  hook "{$base,\"hook_event_name\":\"SessionEnd\",\"reason\":\"exit\"}"
  state_is "" || fail "a Codex SessionEnd did not set idle"
}

test_the_hook_never_fails() {
  local output
  output="$(printf 'not json' | TMUX_PANE="$AGENT_PANE" "$SCRIPTS/agent_state.sh" 2>&1)" ||
    fail "the hook failed on bad input"
  [ -z "$output" ] || fail "the hook printed on bad input: $output"
  output="$(printf '{"hook_event_name":"Stop"}' | env -u TMUX_PANE "$SCRIPTS/agent_state.sh" 2>&1)" ||
    fail "the hook failed outside tmux"
  [ -z "$output" ] || fail "the hook printed outside tmux: $output"
  output="$(printf '{"hook_event_name":"Stop"}' | TMUX_PANE=%999 "$SCRIPTS/agent_state.sh" 2>&1)" ||
    fail "the hook failed for a pane that is gone"
  [ -z "$output" ] || fail "the hook printed for a pane that is gone: $output"
  ! "$SCRIPTS/agent_status.sh" dev-work sideways 2>/dev/null || fail "an unknown state was accepted"
  ! "$SCRIPTS/agent_status.sh" dev-none done 2>/dev/null || fail "a missing workspace was accepted"
}

frame() {
  tmux show-option -qv -t "=$1:" @devws_agent_frame
}

has_frame() {
  [ -n "$(frame "$1")" ]
}

tickers() {
  pgrep -f "$SCRIPTS/agent_status.sh .* --tick" | wc -l | tr -d ' '
}

test_the_spinner_turns_only_while_working() {
  local first
  "$SCRIPTS/agent_status.sh" dev-work working
  "$SCRIPTS/agent_status.sh" dev-work working
  "$SCRIPTS/agent_status.sh" dev-work working
  wait_until has_frame dev-work || fail "the spinner did not start"
  first="$(frame dev-work)"
  frame_changed() { [ "$(frame dev-work)" != "$first" ]; }
  wait_until frame_changed || fail "the spinner does not turn"
  one_ticker() { [ "$(tickers)" = 1 ]; }
  wait_until one_ticker || fail "more than one spinner runs: $(tickers)"

  "$SCRIPTS/agent_status.sh" dev-work done
  no_ticker() { [ "$(tickers)" = 0 ]; }
  wait_until no_ticker || fail "the spinner kept running after done"

  new_workspace dev-gone
  "$SCRIPTS/agent_status.sh" dev-gone working
  wait_until has_frame dev-gone || fail "the second spinner did not start"
  tmux kill-session -t dev-gone
  wait_until no_ticker || fail "the spinner kept running after its workspace closed"
}

render() {
  tmux display-message -p -t "$AGENT_PANE" "$1"
}

conf_option() {
  sed -n "s/^set -g $1 \"\\(.*\\)\"\$/\\1/p" "$ROOT/config/tmux/tmux.conf"
}

test_header_strip_and_status_bar_show_the_state() {
  local header status_left
  header="$(conf_option pane-border-format)"
  status_left="$(sed -n 's/.*tmux set-option status-left "\([^"]*\)".*/\1/p' "$SCRIPTS/dev.yml")"
  [ -n "$header" ] && [ -n "$status_left" ] || fail "could not read the formats"

  "$SCRIPTS/agent_status.sh" dev-work idle
  ! render "$header" | grep -qE "working|needs you|done" || fail "the idle header shows a state"
  [ "$(render "$status_left")" = "[work]" ] || fail "the idle status bar shows a state: $(render "$status_left")"

  "$SCRIPTS/agent_status.sh" dev-work waiting
  render "$header" | grep -q "Agent · claude .*● needs you" || fail "the header does not show waiting"
  render "$status_left" | grep -q "^\[work\] .*●" || fail "the status bar does not show waiting"

  "$SCRIPTS/agent_status.sh" dev-work done
  render "$header" | grep -q "Agent · claude .*✓ done" || fail "the header does not show done"
  render "$status_left" | grep -q "^\[work\] .*✓" || fail "the status bar does not show done"

  "$SCRIPTS/agent_status.sh" dev-work working
  render "$header" | grep -qE "Agent · claude [⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] working" || fail "the header does not show working"
  render "$status_left" | grep -qE "^\[work\] [⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]$" || fail "the status bar does not show working"

  tmux set-option -p -t "$AGENT_PANE" @devws_role minimized-agent
  render "$header" | grep -qE "▸ Agent [⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]$" || fail "the minimized strip does not show working"
  ! render "$header" | grep -q "working" || fail "the minimized strip is not just the icon"
  tmux set-option -p -t "$AGENT_PANE" @devws_role agent
  "$SCRIPTS/agent_status.sh" dev-work idle
}

# A real client, running inside a second tmux server, so the header is drawn.
test_a_real_client_redraws_the_spinner() {
  local first
  tmux set-option -g pane-border-format "$(conf_option pane-border-format)"
  tmux set-option -w -t dev-work: pane-border-status top
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t dev-work"
  client_header() {
    "$REAL_TMUX" -L "$CLIENT_SOCKET" capture-pane -p -t client | grep -m 1 "Agent ·"
  }
  "$SCRIPTS/agent_status.sh" dev-work working
  shows_working() { client_header | grep -q "working"; }
  wait_until shows_working || fail "the client does not draw working: $(client_header)"
  first="$(client_header)"
  redrawn() { [ "$(client_header)" != "$first" ]; }
  wait_until redrawn || fail "the client does not redraw the spinner"
  "$SCRIPTS/agent_status.sh" dev-work done
  shows_done() { client_header | grep -q "✓ done"; }
  wait_until shows_done || fail "the client does not draw done: $(client_header)"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server
  "$SCRIPTS/agent_status.sh" dev-work idle
}

test_the_sidebar_shows_each_workspace_state() {
  local pane screen
  new_workspace dev-other
  "$SCRIPTS/agent_status.sh" dev-work waiting
  "$SCRIPTS/agent_status.sh" dev-other done
  pane="$(tmux split-window -t dev-work: -h -l 60 -P -F '#{pane_id}' "$SCRIPTS/session_picker.sh")"
  sidebar() { tmux capture-pane -p -t "$pane"; }
  shows_both() { sidebar | grep -q "claude ● needs you · nvim" && sidebar | grep -q "claude ✓ done · nvim"; }
  wait_until shows_both || fail "the sidebar does not show each state: $(sidebar)"

  "$SCRIPTS/agent_status.sh" dev-other working
  shows_working() { sidebar | grep -q "claude ⠋ working · nvim"; }
  wait_until shows_working || fail "the sidebar did not redraw on a state change: $(sidebar)"
  "$SCRIPTS/agent_status.sh" dev-other idle
  shows_idle() { sidebar | grep -q "claude · nvim"; }
  wait_until shows_idle || fail "the sidebar shows a state while idle: $(sidebar)"
  tmux kill-pane -t "$pane"
  tmux kill-session -t dev-other
  "$SCRIPTS/agent_status.sh" dev-work idle
}

agent_settings() {
  sed -n '/^--settings$/{n;p;}' "$TEST_ROOT/agents/claude.args"
}

test_agents_start_with_the_state_hooks() {
  local pane settings event
  pane="$(tmux split-window -t dev-work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh claude")"
  wait_until test -s "$TEST_ROOT/agents/claude.args" || fail "claude did not start"
  settings="$(agent_settings)"
  for event in SessionStart UserPromptSubmit PreToolUse PostToolUse Notification Stop SessionEnd; do
    jq -e --arg event "$event" '.hooks[$event][].hooks[] | select(.command | contains("agent_state.sh"))' \
      "$settings" >/dev/null || fail "claude has no state hook for $event"
  done
  jq -e '.hooks.Notification[] | select(.matcher == "permission_prompt|elicitation_dialog")' "$settings" >/dev/null ||
    fail "claude's notification hook is not limited to prompts"
  jq -e '.hooks.PostToolUse[].hooks[] | select(.command | contains("agent_follow_hook.sh"))' "$settings" >/dev/null ||
    fail "claude lost its follow hook"
  tmux kill-pane -t "$pane"

  pane="$(tmux split-window -t dev-work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh codex")"
  wait_until test -s "$TEST_ROOT/agents/codex.args" || fail "codex did not start"
  for event in SessionStart UserPromptSubmit PreToolUse PermissionRequest PostToolUse Stop SessionEnd; do
    grep -q "^hooks.$event=.*agent_state.sh" "$TEST_ROOT/agents/codex.args" ||
      fail "codex has no state hook for $event"
  done
  grep -q '^hooks.PostToolUse=.*apply_patch.*agent_follow_hook.sh.* codex' "$TEST_ROOT/agents/codex.args" ||
    fail "codex lost its follow hook"
  tmux kill-pane -t "$pane"
}

test_claude_events_set_the_state
test_codex_events_set_the_state
test_the_hook_never_fails
test_the_spinner_turns_only_while_working
test_header_strip_and_status_bar_show_the_state
test_a_real_client_redraws_the_spinner
test_the_sidebar_shows_each_workspace_state
test_agents_start_with_the_state_hooks

printf 'Agent status tests passed.\n'
