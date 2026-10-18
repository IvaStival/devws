#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="${TMPDIR:-/tmp}"
TEST_ROOT="$(mktemp -d "${tmp_dir%/}/devws-jump.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-jump-test-$$"
CLIENT_SOCKET="devws-jump-client-$$"

cleanup() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server 2>/dev/null || true
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Jump to agent test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
chmod +x "$TEST_ROOT/bin/tmux"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
unset TMUX

wait_until() {
  local attempt
  for attempt in $(seq 1 30); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

# A workspace: editor on top, agent and terminal below, as dev.yml lays it out.
new_workspace() {
  local editor agent
  tmux new-session -d -s "$1" -x 200 -y 50 'sleep 600'
  editor="$(tmux display-message -p -t "$1:" '#{pane_id}')"
  tmux set-option -p -t "$editor" @devws_role editor
  agent="$(tmux split-window -v -t "$editor" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$agent" @devws_role agent
  tmux set-option -p -t "$(tmux split-window -h -t "$agent" -P -F '#{pane_id}' 'sleep 600')" @devws_role terminal
  # Focus away from the agent, so a jump has to move it.
  tmux select-pane -t "$editor"
}

# <session> <state> <since>
set_state() {
  tmux set-option -t "$1" @devws_agent_state "$2"
  tmux set-option -t "$1" @devws_agent_state_since "$3"
}

agent_of() {
  tmux list-panes -t "$1:" -F '#{pane_id} #{@devws_role}' | awk '$2 == "agent" { print $1 }'
}

new_workspace dev-home
new_workspace dev-old
new_workspace dev-new
new_workspace dev-finished
tmux new-session -d -s other 'sleep 600'
set_state other waiting 1

"$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
  "TMUX= $REAL_TMUX -L $SOCKET attach -t dev-home"
attached() { [ -n "$(tmux list-clients -F '#{client_name}')" ]; }
wait_until attached || fail "the client did not attach"
CLIENT="$(tmux list-clients -F '#{client_name}' | head -n 1)"

client_session() {
  tmux display-message -c "$CLIENT" -p '#{client_session}'
}

# <session>: the client is on it, with its agent pane focused.
lands_on() {
  [ "$(client_session)" = "$1" ] &&
    [ "$(tmux display-message -p -t "$1:" '#{pane_id}')" = "$(agent_of "$1")" ]
}

jump() {
  "$SCRIPTS/jump_agent.sh" "$CLIENT"
}

test_waiting_agents_go_first_the_longest_waiting_first() {
  set_state dev-old waiting 100
  set_state dev-new waiting 200
  set_state dev-finished done 50
  jump
  lands_on dev-old || fail "the first jump went to $(client_session), not the oldest waiting"
  jump
  lands_on dev-new || fail "the second jump went to $(client_session), not the next waiting"
  jump
  lands_on dev-old || fail "the third jump did not wrap round: $(client_session)"
}

test_a_done_agent_when_none_waits() {
  tmux set-option -u -t dev-old @devws_agent_state
  tmux set-option -u -t dev-new @devws_agent_state
  jump
  lands_on dev-finished || fail "with none waiting it went to $(client_session)"
}

test_nothing_to_go_to() {
  tmux set-option -u -t dev-finished @devws_agent_state
  tmux switch-client -c "$CLIENT" -t dev-home
  jump
  [ "$(client_session)" = dev-home ] || fail "with no agent to go to it moved to $(client_session)"
}

test_a_minimized_agent_is_restored() {
  "$SCRIPTS/minimize_panes.sh" dev-new toggle agents
  [ -z "$(agent_of dev-new)" ] || fail "the agent did not minimize"
  set_state dev-new waiting 300
  jump
  [ -n "$(agent_of dev-new)" ] || fail "the minimized agent was not restored"
  lands_on dev-new || fail "it did not land on the restored agent"
}

# An extra agent (New panel) that waits gets the focus, not the main agent.
test_the_waiting_extra_agent_gets_the_focus() {
  local extra
  extra="$(tmux split-window -h -t "$(agent_of dev-old)" -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$extra" @devws_role extra-agent
  tmux set-option -p -t "$extra" @devws_pane_state waiting
  set_state dev-old waiting 50
  tmux switch-client -c "$CLIENT" -t dev-home
  jump
  [ "$(client_session)" = dev-old ] && [ "$(tmux display-message -p -t dev-old: '#{pane_id}')" = "$extra" ] ||
    fail "the jump did not focus the waiting extra agent"
  tmux kill-pane -t "$extra"
  tmux set-option -u -t dev-old @devws_agent_state
}

test_waiting_agents_go_first_the_longest_waiting_first
test_a_done_agent_when_none_waits
test_nothing_to_go_to
test_the_waiting_extra_agent_gets_the_focus
test_a_minimized_agent_is_restored

printf 'Jump to agent tests passed.\n'
