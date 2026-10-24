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
# Notifications go to a file, never to macOS, with settings of the test's own.
cat > "$TEST_ROOT/bin/notify" <<EOF
#!/usr/bin/env bash
printf '%s|%s\n' "\$1" "\$2" >> "$TEST_ROOT/notifications"
EOF
chmod +x "$TEST_ROOT/bin/notify"
: > "$TEST_ROOT/devws.env"
# TMPDIR keeps the popup flag (devws_menu.sh) apart from a real menu's.
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT" \
  DEVWS_NOTIFY_COMMAND="$TEST_ROOT/bin/notify" DEVWS_ENV_FILE="$TEST_ROOT/devws.env"
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
grep -E '^(color_[a-z_]+=|set -g @devws_(agent|pane)_)' "$ROOT/config/tmux/tmux.conf" > "$TEST_ROOT/formats.conf"
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

test_the_hook_records_the_conversation() {
  claude_event UserPromptSubmit "\"transcript_path\":\"$TEST_ROOT/one.jsonl\",\"prompt\":\"go\""
  [ "$(tmux show-option -qv -t =dev-work: @devws_agent_transcript)" = "$TEST_ROOT/one.jsonl" ] ||
    fail "the conversation's transcript is not recorded"
  claude_event Stop '"stop_hook_active":false'
  [ "$(tmux show-option -qv -t =dev-work: @devws_agent_transcript)" = "$TEST_ROOT/one.jsonl" ] ||
    fail "an event without a transcript forgot the conversation"
  claude_event SessionStart "\"transcript_path\":\"$TEST_ROOT/two.jsonl\",\"source\":\"clear\""
  [ "$(tmux show-option -qv -t =dev-work: @devws_agent_transcript)" = "$TEST_ROOT/two.jsonl" ] ||
    fail "a new conversation is not recorded"
  claude_event SessionEnd '"reason":"exit"'
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

# The frame the agents' headers and the status bar show: the session's own,
# else the global one.
frame() {
  tmux display-message -p -t "=$1:" '#{@devws_agent_frame}'
}

# One turn of the shared spinner, as header_ticker.sh runs it.
spin() {
  "$SCRIPTS/agent_spinner.sh" "$1"
}

test_the_spinner_turns_only_while_working() {
  tmux set-option -gu @devws_agent_frame
  "$SCRIPTS/agent_status.sh" dev-work idle
  spin 3
  [ -z "$(frame dev-work)" ] || fail "the spinner turned with no agent working"

  "$SCRIPTS/agent_status.sh" dev-work working
  spin 3
  [ "$(frame dev-work)" = ⠸ ] || fail "turn 3 did not show the fourth frame: $(frame dev-work)"
  spin 14
  [ "$(frame dev-work)" = ⠼ ] || fail "the frames do not wrap round: $(frame dev-work)"
  [ -z "$(pgrep -f "$SCRIPTS/agent_status.sh")" ] || fail "a state change left a process running"

  # Under a devws popup (the menu) the spinner holds still: a redraw would
  # show the panes behind it. It turns again once the popup closes.
  touch "$TEST_ROOT/devws-popup-$(id -u)"
  spin 15
  [ "$(frame dev-work)" = ⠼ ] || fail "the spinner turned under a popup: $(frame dev-work)"
  rm "$TEST_ROOT/devws-popup-$(id -u)"
  spin 15
  [ "$(frame dev-work)" = ⠴ ] || fail "the spinner did not turn again after the popup: $(frame dev-work)"

  # A frame left on the session by an older devws goes with the next change.
  tmux set-option -t dev-work @devws_agent_frame ⠏
  "$SCRIPTS/agent_status.sh" dev-work done
  "$SCRIPTS/agent_status.sh" dev-work working
  [ "$(frame dev-work)" = ⠴ ] || fail "an old session frame hides the shared one"
  "$SCRIPTS/agent_status.sh" dev-work idle
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
  ! render "$header" | grep -qE "[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]|needs you|done" || fail "the idle header shows a state"
  [ "$(render "$status_left")" = "[work]" ] || fail "the idle status bar shows a state: $(render "$status_left")"

  "$SCRIPTS/agent_status.sh" dev-work waiting
  render "$header" | grep -q "Agent · claude .*● needs you" || fail "the header does not show waiting"
  render "$status_left" | grep -q "^\[work\] .*●" || fail "the status bar does not show waiting"

  "$SCRIPTS/agent_status.sh" dev-work done
  render "$header" | grep -q "Agent · claude .*✓ done" || fail "the header does not show done"
  render "$status_left" | grep -q "^\[work\] .*✓" || fail "the status bar does not show done"

  "$SCRIPTS/agent_status.sh" dev-work working
  render "$header" | grep -qE "▾ [⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] Agent · claude( |$)" || fail "the header's icon is not the spinner: $(render "$header")"
  ! render "$header" | grep -q "working" || fail "the header still names working"
  render "$status_left" | grep -qE "^\[work\] [⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]$" || fail "the status bar does not show working"

  tmux set-option -p -t "$AGENT_PANE" @devws_role minimized-agent
  render "$header" | grep -qE "▸ Agent [⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]$" || fail "the minimized strip does not show working"
  ! render "$header" | grep -q "Agent · " || fail "the minimized strip is not just the icon"
  tmux set-option -p -t "$AGENT_PANE" @devws_role agent
  "$SCRIPTS/agent_status.sh" dev-work idle
}

# An extra agent (New panel) beside the main one: each pane keeps its own
# state, shown in its own header; the workspace's state sums them up —
# waiting, else working, else done, else none.
test_each_agent_pane_has_its_own_state() {
  local header extra
  header="$(conf_option pane-border-format)"
  extra="$(tmux split-window -t "$AGENT_PANE" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$extra" @devws_role extra-agent
  tmux set-option -p -t "$extra" @devws_agent work
  extra_event() {
    printf '{"session_id":"x","cwd":"%s","hook_event_name":"%s"%s}' "$TEST_ROOT/project" "$1" "${2:+,$2}" |
      TMUX_PANE="$extra" "$SCRIPTS/agent_state.sh"
  }
  pane_state() {
    tmux show-options -pqv -t "$1" @devws_pane_state
  }

  extra_event UserPromptSubmit '"prompt":"go"'
  [ "$(pane_state "$extra")" = working ] || fail "the extra agent's pane is not working"
  state_is working || fail "the workspace does not show its working agent: $(state_of dev-work)"
  ! [ "$(pane_state "$AGENT_PANE")" = working ] || fail "the main agent took the extra agent's state"

  claude_event Notification '"notification_type":"permission_prompt","message":"permission"'
  state_is waiting || fail "a waiting agent does not come first: $(state_of dev-work)"
  tmux display-message -p -t "$AGENT_PANE" "$header" | grep -q "Agent · claude .*● needs you" ||
    fail "the main agent's header does not show its own state: $(tmux display-message -p -t "$AGENT_PANE" "$header")"
  tmux display-message -p -t "$extra" "$header" | grep -qE "▾ [⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] Agent · work( |$)" ||
    fail "the extra agent's header does not show its own state: $(tmux display-message -p -t "$extra" "$header")"

  extra_event Stop '"stop_hook_active":false'
  state_is waiting || fail "an agent done hid the waiting one"
  claude_event SessionEnd '"reason":"exit"'
  state_is done || fail "the workspace does not show the done agent: $(state_of dev-work)"
  ! tmux display-message -p -t "$AGENT_PANE" "$header" | grep -q "done" ||
    fail "the idle main agent shows the workspace's state"

  # The conversation on tmux's line stays the main agent's.
  claude_event UserPromptSubmit "\"transcript_path\":\"$TEST_ROOT/main.jsonl\",\"prompt\":\"go\""
  extra_event UserPromptSubmit "\"transcript_path\":\"$TEST_ROOT/extra.jsonl\",\"prompt\":\"go\""
  [ "$(tmux show-option -qv -t =dev-work: @devws_agent_transcript)" = "$TEST_ROOT/main.jsonl" ] ||
    fail "the extra agent took over the conversation"

  extra_event SessionEnd '"reason":"exit"'
  claude_event SessionEnd '"reason":"exit"'
  state_is "" || fail "all agents idle is not idle: $(state_of dev-work)"
  tmux kill-pane -t "$extra"
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
  spin 1
  shows_working() { client_header | grep -qE "[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] Agent ·"; }
  wait_until shows_working || fail "the client does not draw working: $(client_header)"
  first="$(client_header)"
  spin 2
  redrawn() { [ "$(client_header)" != "$first" ]; }
  wait_until redrawn || fail "the client does not redraw the spinner"
  "$SCRIPTS/agent_status.sh" dev-work done
  shows_done() { client_header | grep -q "✓ done"; }
  wait_until shows_done || fail "the client does not draw done: $(client_header)"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server
  "$SCRIPTS/agent_status.sh" dev-work idle
}

since_of() {
  tmux show-option -qv -t "=$1:" @devws_agent_state_since
}

test_the_state_records_when_it_started() {
  local since
  "$SCRIPTS/agent_status.sh" dev-work idle
  [ -z "$(since_of dev-work)" ] || fail "idle has a start time"
  "$SCRIPTS/agent_status.sh" dev-work waiting
  since="$(since_of dev-work)"
  [[ "$since" =~ ^[0-9]+$ ]] || fail "waiting has no start time: $since"
  tmux set-option -t dev-work @devws_agent_state_since 1
  "$SCRIPTS/agent_status.sh" dev-work waiting
  [ "$(since_of dev-work)" = 1 ] || fail "the same state again reset its start time"
  "$SCRIPTS/agent_status.sh" dev-work done
  [ "$(since_of dev-work)" != 1 ] || fail "a new state kept the old start time"
  "$SCRIPTS/agent_status.sh" dev-work idle
  [ -z "$(since_of dev-work)" ] || fail "idle kept a start time"
}

notifications() {
  cat "$TEST_ROOT/notifications" 2>/dev/null
}

notified() {
  notifications | grep -qxF "$1"
}

test_notifications_name_the_workspace_and_agent() {
  "$SCRIPTS/agent_status.sh" dev-work idle
  rm -f "$TEST_ROOT/notifications"
  "$SCRIPTS/agent_status.sh" dev-work working
  "$SCRIPTS/agent_status.sh" dev-work waiting
  wait_until notified "work|claude needs you" || fail "waiting was not notified: $(notifications)"
  "$SCRIPTS/agent_status.sh" dev-work waiting
  "$SCRIPTS/agent_status.sh" dev-work done
  wait_until notified "work|claude is done" || fail "done was not notified: $(notifications)"
  sleep 0.3
  [ "$(notifications | wc -l | tr -d ' ')" = 2 ] || fail "working or a repeated state notified: $(notifications)"

  rm -f "$TEST_ROOT/notifications"
  printf 'DEVWS_NOTIFY=waiting\n' > "$DEVWS_ENV_FILE"
  "$SCRIPTS/agent_status.sh" dev-work waiting
  "$SCRIPTS/agent_status.sh" dev-work done
  wait_until notified "work|claude needs you" || fail "DEVWS_NOTIFY=waiting dropped waiting"
  sleep 0.3
  ! notified "work|claude is done" || fail "DEVWS_NOTIFY=waiting notified done"

  rm -f "$TEST_ROOT/notifications"
  printf 'DEVWS_NOTIFY=0\n' > "$DEVWS_ENV_FILE"
  "$SCRIPTS/agent_status.sh" dev-work waiting
  sleep 0.3
  [ -z "$(notifications)" ] || fail "DEVWS_NOTIFY=0 notified: $(notifications)"
  : > "$DEVWS_ENV_FILE"
  "$SCRIPTS/agent_status.sh" dev-work idle
}

# A client that shows the workspace sees the state already: no notification.
test_no_notification_for_the_workspace_on_screen() {
  rm -f "$TEST_ROOT/notifications"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t dev-work"
  attached() { [ -n "$(tmux list-clients -t dev-work)" ]; }
  wait_until attached || fail "the client did not attach"
  "$SCRIPTS/agent_status.sh" dev-work waiting
  sleep 0.3
  [ -z "$(notifications)" ] || fail "the workspace on screen was notified: $(notifications)"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server
  "$SCRIPTS/agent_status.sh" dev-work idle
}

sidebar_test_pane=""

sidebar() {
  tmux capture-pane -p -t "$sidebar_test_pane"
}

working_row() {
  sidebar | grep -oE "claude [⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] · nvim"
}

test_the_sidebar_shows_each_workspace_state() {
  local first fzf_before
  new_workspace dev-other
  "$SCRIPTS/agent_status.sh" dev-work waiting
  "$SCRIPTS/agent_status.sh" dev-other done
  sidebar_test_pane="$(tmux split-window -t dev-work: -h -l 60 -P -F '#{pane_id}' "$SCRIPTS/session_picker.sh")"
  shows_both() { sidebar | grep -q "claude ● needs you · nvim" && sidebar | grep -q "claude ✓ done · nvim"; }
  wait_until shows_both || fail "the sidebar does not show each state: $(sidebar)"
  sidebar_fzf() {
    pgrep -f -- "--listen=.*devws-picker-$(tmux display-message -p -t "$sidebar_test_pane" '#{pane_pid}').sock"
  }
  fzf_before="$(sidebar_fzf)"
  [ -n "$fzf_before" ] || fail "the sidebar's fzf is not listening"
  listening() { [ -S "$(tmux show-option -pqv -t "$sidebar_test_pane" @devws_picker_socket)" ]; }
  wait_until listening || fail "the sidebar did not publish its socket"

  # No client shows it: the state changes reach it, the spinner leaves it be.
  "$SCRIPTS/agent_status.sh" dev-other working
  shows_working() { [ -n "$(working_row)" ]; }
  wait_until shows_working || fail "the sidebar did not redraw on a state change: $(sidebar)"
  first="$(working_row)"
  spin 4
  sleep 0.5
  [ "$(working_row)" = "$first" ] || fail "a sidebar no one sees was spun: $(working_row)"

  # Shown by a client, it spins.
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t dev-work"
  attached() { [ -n "$(tmux list-clients -t dev-work)" ]; }
  wait_until attached || fail "the client did not attach"
  spin 4
  turned() { shows_working && [ "$(working_row)" != "$first" ]; }
  wait_until turned || fail "the sidebar on screen does not spin: $(sidebar)"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server

  "$SCRIPTS/agent_status.sh" dev-other idle
  shows_idle() { sidebar | grep -q "claude · nvim"; }
  wait_until shows_idle || fail "the sidebar shows a state while idle: $(sidebar)"
  [ "$(sidebar_fzf)" = "$fzf_before" ] || fail "the sidebar restarted on a state change"
  # Nothing polls while fzf runs: the sidebar starts no sleeps.
  local sidebar_pid sleeps
  sidebar_pid="$(tmux display-message -p -t "$sidebar_test_pane" '#{pane_pid}')"
  sleeps="$(for attempt in 1 2 3 4 5; do pgrep -P "$sidebar_pid" sleep; sleep 0.1; done)"
  [ -z "$sleeps" ] || fail "the sidebar polls: it started sleep $sleeps"
  tmux kill-pane -t "$sidebar_test_pane"
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

  # An extra agent (New panel) starts the same way, marked as an extra one,
  # with its agent's name on its pane; the main agent pane is not touched.
  rm -f "$TEST_ROOT/agents/claude.args"
  pane="$(tmux split-window -t dev-work: -P -F '#{pane_id}' -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh claude --extra")"
  wait_until test -s "$TEST_ROOT/agents/claude.args" || fail "the extra claude did not start"
  [ "$(tmux show-options -pqv -t "$pane" @devws_role)" = extra-agent ] &&
    [ "$(tmux show-options -pqv -t "$pane" @devws_agent)" = claude ] ||
    fail "the extra agent's pane is not marked as one"
  [ "$(tmux show-options -pqv -t "$AGENT_PANE" @devws_role)" = agent ] || fail "the main agent pane lost its role"
  grep -q -- '--settings' "$TEST_ROOT/agents/claude.args" || fail "the extra agent started without the devws hooks"
  tmux kill-pane -t "$pane"
}

test_claude_events_set_the_state
test_codex_events_set_the_state
test_the_hook_records_the_conversation
test_the_hook_never_fails
test_the_spinner_turns_only_while_working
test_header_strip_and_status_bar_show_the_state
test_each_agent_pane_has_its_own_state
test_a_real_client_redraws_the_spinner
test_the_sidebar_shows_each_workspace_state
test_the_state_records_when_it_started
test_notifications_name_the_workspace_and_agent
test_no_notification_for_the_workspace_on_screen
test_agents_start_with_the_state_hooks

printf 'Agent status tests passed.\n'
