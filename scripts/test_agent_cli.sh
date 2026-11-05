#!/usr/bin/env bash
# The agent commands devAgentics drives (agent_cli.sh), and the runner's
# --args-file they rely on.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-agent-cli.XXXXXX")"
SCRIPTS_SRC="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-agent-cli-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT" 2>/dev/null || { sleep 0.5; rm -rf "$TEST_ROOT"; }
}
trap cleanup EXIT

fail() {
  printf 'Agent CLI test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call goes to an isolated server. The agents are stubs that record
# their arguments, NUL-separated, and wait.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator" "$TEST_ROOT/project" "$TEST_ROOT/started"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
for program in claude codex nvim; do
  cat > "$TEST_ROOT/bin/$program" <<EOF
#!/usr/bin/env bash
printf '%s\0' "\$@" > "$TEST_ROOT/started/$program.\$\$"
exec sleep 600
EOF
done
chmod +x "$TEST_ROOT/bin/"*
for script in "$SCRIPTS_SRC"/*.sh "$SCRIPTS_SRC"/*.json "$SCRIPTS_SRC"/*.md "$SCRIPTS_SRC"/*.tsv; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
printf 'DEVWS_AGENTS="claude codex"\n' > "$TEST_ROOT/home/.tmuxinator/.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home"
# agent_status.sh would post a macOS notification when an agent is done.
export DEVWS_NOTIFY_COMMAND=true
unset TMUX
SCRIPTS="$HOME/.tmuxinator"

wait_until() {
  local attempt
  for attempt in $(seq 1 30); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

status_of() {
  local code=0
  "$@" >/dev/null 2>&1 || code=$?
  printf '%s' "$code"
}

# The layout dev.yml opens a workspace with: picker, editor, agent, terminal.
DEFAULT_LAYOUT="12f9,200x50,0,0{25x50,0,0,58,174x50,26,0[174x24,26,0,57,174x25,26,25{86x25,26,25,59,87x25,113,25,60}]}"

# A workspace like devws opens, its agent pane writing what it is typed to
# $TEST_ROOT/received.<session>. Prints the agent pane's id.
create_workspace() {
  local session="$1" role pane agent=""
  tmux new-session -d -s "$session" -x 200 -y 50 -c "$TEST_ROOT/project" 'sleep 600'
  tmux set-option -t "$session" @devws_root "$TEST_ROOT/project"
  tmux select-pane -t "$session:" -T devws-picker
  for role in editor agent terminal; do
    if [ "$role" = agent ]; then
      pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' "cat > $TEST_ROOT/received.$session")"
      agent="$pane"
    else
      pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' 'sleep 600')"
    fi
    tmux set-option -p -t "$pane" @devws_role "$role"
    tmux select-layout -t "$session:" tiled
  done
  tmux select-layout -t "$session:" "$DEFAULT_LAYOUT"
  printf '%s\n' "$agent"
}

field() {
  tmux display-message -p -t "$1" "#{$2}"
}

# <pane> right of <of>, at its top.
right_of() {
  [ "$(field "$1" pane_top)" = "$(field "$2" pane_top)" ] &&
    [ "$(field "$1" pane_left)" -gt "$(field "$2" pane_left)" ]
}

received() {
  cat "$TEST_ROOT/received.$1" 2>/dev/null || true
}

RS=$'\036'

# The newest recording of a stub agent: its arguments, each followed by the
# record separator, so one with a line break of its own stays one.
started_args() {
  local file
  file="$(ls -t "$TEST_ROOT/started/$1".* 2>/dev/null | head -1)"
  [ -n "$file" ] || return 1
  tr '\0' "$RS" < "$file"
}

test_the_runner_appends_the_args_file() {
  local file="$TEST_ROOT/args" recorded
  # A prompt with spaces and a line break must stay one argument.
  printf '%s\0' --model opus 'two words
and a second line' > "$file"

  tmux new-session -d -s runner-claude -x 100 -y 30 -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh claude --args-file $file"
  wait_until started_args claude >/dev/null || fail "the runner did not start claude"
  recorded="$(started_args claude)"
  case "$recorded" in
    *"--append-system-prompt$RS"*"${RS}--model${RS}opus${RS}two words
and a second line$RS") ;;
    *) fail "claude did not get the file's arguments after devws's: $recorded" ;;
  esac

  tmux new-session -d -s runner-codex -x 100 -y 30 -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh codex --args-file $file"
  wait_until started_args codex >/dev/null || fail "the runner did not start codex"
  case "$(started_args codex)" in
    *"-c${RS}developer_instructions="*"${RS}--model${RS}opus${RS}two words
and a second line$RS") ;;
    *) fail "codex did not get the file's arguments last: $(started_args codex)" ;;
  esac

  # Without the option nothing changes.
  rm -f "$TEST_ROOT"/started/claude.*
  tmux new-session -d -s runner-plain -x 100 -y 30 -c "$TEST_ROOT/project" \
    "$SCRIPTS/agent_runner.sh claude"
  wait_until started_args claude >/dev/null || fail "the plain runner did not start claude"
  case "$(started_args claude)" in
    *"--model"*) fail "a plain runner passed extra arguments" ;;
  esac
}

test_the_runner_refuses_an_unreadable_args_file() {
  local output
  output="$("$SCRIPTS/agent_runner.sh" claude --args-file "$TEST_ROOT/missing" 2>&1)" &&
    fail "the runner accepted a missing args file"
  case "$output" in
    *"missing"*) ;;
    *) fail "the runner did not name the missing file: $output" ;;
  esac
}

test_state_follows_the_agent() {
  local pane
  pane="$(create_workspace state)"
  [ "$("$SCRIPTS/agent_cli.sh" state "$pane")" = idle ] || fail "a fresh agent is not idle"
  "$SCRIPTS/agent_status.sh" state working "$pane"
  [ "$("$SCRIPTS/agent_cli.sh" state "$pane")" = working ] || fail "state did not follow working"
  "$SCRIPTS/agent_status.sh" state waiting "$pane"
  [ "$("$SCRIPTS/agent_cli.sh" state "$pane")" = waiting ] || fail "state did not follow waiting"
  "$SCRIPTS/agent_status.sh" state done "$pane"
  [ "$("$SCRIPTS/agent_cli.sh" state "$pane")" = done ] || fail "state did not follow done"
  tmux kill-pane -t "$pane"
  [ "$("$SCRIPTS/agent_cli.sh" state "$pane")" = gone ] || fail "a killed pane is not gone"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" state notapane)" = 2 ] || fail "state accepted a name that is not a pane id"
}

test_send_types_into_an_idle_agent_only() {
  local pane
  pane="$(create_workspace send)"

  "$SCRIPTS/agent_cli.sh" send "$pane" 'first line
second line'
  wait_until test "$(received send)" = 'first line
second line' || fail "send did not deliver the text as typed: $(received send)"

  "$SCRIPTS/agent_status.sh" send working "$pane"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" send "$pane" 'while busy')" = 3 ] || fail "send did not refuse a working agent"
  "$SCRIPTS/agent_status.sh" send waiting "$pane"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" send "$pane" 'while waiting')" = 3 ] || fail "send did not refuse a waiting agent"
  sleep 0.5
  case "$(received send)" in *busy*|*waiting*) fail "a refused send reached the agent" ;; esac

  "$SCRIPTS/agent_cli.sh" send --force "$pane" 'forced'
  wait_until grep -q forced "$TEST_ROOT/received.send" || fail "send --force did not deliver to a busy agent"

  printf 'from stdin' | "$SCRIPTS/agent_cli.sh" send --force "$pane" -
  wait_until grep -q 'from stdin' "$TEST_ROOT/received.send" || fail "send did not read the text from stdin"

  "$SCRIPTS/agent_status.sh" send done "$pane"
  "$SCRIPTS/agent_cli.sh" send "$pane" 'after done'
  wait_until grep -q 'after done' "$TEST_ROOT/received.send" || fail "send did not deliver to a done agent"

  tmux kill-pane -t "$pane"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" send "$pane" 'nobody')" = 4 ] || fail "send did not report a missing pane"
}

test_mark_sets_the_state_the_sidebar_shows() {
  local pane
  pane="$(create_workspace mark)"
  "$SCRIPTS/agent_cli.sh" mark "$pane" waiting
  [ "$(tmux show-options -pqv -t "$pane" @devws_pane_state)" = waiting ] || fail "mark did not set the pane's state"
  [ "$(tmux show-option -qv -t =mark: @devws_agent_state)" = waiting ] || fail "mark did not reach the workspace's state"
  "$SCRIPTS/agent_cli.sh" mark "$pane" done
  [ "$(tmux show-option -qv -t =mark: @devws_agent_state)" = done ] || fail "mark done did not reach the workspace"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" mark "$pane" working)" = 2 ] || fail "mark accepted a state agents report themselves"
}

test_stop_ends_the_agent_and_clears_its_state() {
  local pane
  pane="$(create_workspace stop)"
  "$SCRIPTS/agent_status.sh" stop working "$pane"
  "$SCRIPTS/agent_cli.sh" stop "$pane"
  [ "$("$SCRIPTS/agent_cli.sh" state "$pane")" = gone ] || fail "stop left the pane"
  [ -z "$(tmux show-option -qv -t =stop: @devws_agent_state)" ] || fail "stop left the workspace marked working"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" stop "$pane")" = 4 ] || fail "stop did not report a missing pane"
}

test_spawn_adds_an_agent_beside_a_pane_without_taking_the_focus() {
  local base other pane
  base="$(create_workspace spawn)"
  other="$(tmux list-panes -t spawn: -F '#{pane_id} #{@devws_role}' | awk '$2 == "terminal" { print $1 }')"
  tmux select-pane -t "$other"
  printf '%s\0' --model opus > "$TEST_ROOT/spawn-args"
  rm -f "$TEST_ROOT"/started/claude.*

  pane="$("$SCRIPTS/agent_cli.sh" spawn spawn claude --beside "$base" --agent-args "$TEST_ROOT/spawn-args")" ||
    fail "spawn failed"
  case "$pane" in %[0-9]*) ;; *) fail "spawn did not print a pane id: $pane" ;; esac
  wait_until started_args claude >/dev/null || fail "the spawned agent did not start"
  case "$(started_args claude)" in
    *"--append-system-prompt$RS"*"${RS}--model${RS}opus$RS") ;;
    *) fail "the spawned agent did not get devws's and the file's arguments: $(started_args claude)" ;;
  esac
  [ "$(tmux show-options -pqv -t "$pane" @devws_role)" = extra-agent ] || fail "the spawned agent is not an extra agent"
  right_of "$pane" "$base" || fail "the agent is not right of the pane it was spawned beside"
  [ "$(field spawn: pane_id)" = "$other" ] || fail "spawn moved the focus"

  [ "$(status_of "$SCRIPTS/agent_cli.sh" spawn spawn nobody)" = 1 ] || fail "spawn accepted an unregistered agent"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" spawn spawn claude --position diagonal)" = 1 ] || fail "spawn accepted an unknown position"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" spawn spawn claude --beside %999)" = 1 ] || fail "spawn accepted a pane that is not there"
}

test_run_replaces_the_agent_in_its_pane() {
  local base layout
  base="$(create_workspace run)"
  printf '%s\0' --model opus > "$TEST_ROOT/run-args"
  tmux set-option -p -t "$base" @devws_pane_state working
  layout="$(field run: window_layout)"
  rm -f "$TEST_ROOT"/started/codex.*

  "$SCRIPTS/agent_cli.sh" run "$base" codex --agent-args "$TEST_ROOT/run-args" || fail "run failed"
  wait_until started_args codex >/dev/null || fail "run did not start the new agent"
  case "$(started_args codex)" in
    *"developer_instructions="*"${RS}--model${RS}opus$RS") ;;
    *) fail "run did not pass the file's arguments: $(started_args codex)" ;;
  esac
  [ "$(field run: window_layout)" = "$layout" ] || fail "run changed the layout"
  [ "$(field "$base" pane_id)" = "$base" ] || fail "run replaced the pane"
  [ "$(tmux show-options -pqv -t "$base" @devws_role)" = agent ] || fail "run changed the pane's role"
  [ "$(tmux show-option -qv -t =run: @devws_agent)" = codex ] || fail "run did not record the main agent's name"
  [ "$("$SCRIPTS/agent_cli.sh" state "$base")" = idle ] || fail "run kept the old agent's state"

  [ "$(status_of "$SCRIPTS/agent_cli.sh" run %999 codex)" = 4 ] || fail "run did not report a missing pane"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" run "$base" nobody)" = 1 ] || fail "run accepted an unregistered agent"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" run "$(tmux list-panes -t run: -F '#{pane_id} #{@devws_role}' | awk '$2 == "editor" { print $1 }')" codex)" = 2 ] ||
    fail "run accepted a pane that is not an agent's"
}

test_kind_names_the_program_an_agent_runs() {
  printf 'DEVWS_AGENTS="claude codex work"\nDEVWS_AGENT_work="codex %s"\n' "$HOME/.codex-work" > "$HOME/.tmuxinator/.env"
  [ "$("$SCRIPTS/agent_cli.sh" kind claude)" = claude ] || fail "claude is not claude"
  [ "$("$SCRIPTS/agent_cli.sh" kind work)" = codex ] || fail "an account named work running codex is not codex"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" kind nobody)" = 1 ] || fail "kind accepted an unregistered agent"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" kind)" = 2 ] || fail "kind accepted no agent"
  printf 'DEVWS_AGENTS="claude codex"\n' > "$HOME/.tmuxinator/.env"
}

test_usage_is_what_the_panes_conversation_used() {
  local pane transcript="$TEST_ROOT/usage-transcript.jsonl"
  tmux new-session -d -s usage -x 100 -y 30 'sleep 600'
  pane="$(tmux display-message -p -t usage: '#{pane_id}')"

  [ "$("$SCRIPTS/agent_cli.sh" usage "$pane")" = "-" ] || fail "usage before any conversation is not -"
  printf '{"type":"assistant","timestamp":"2026-11-04T10:00:00.000Z","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":1000,"output_tokens":500,"cache_creation_input_tokens":200,"cache_read_input_tokens":3000}}}\n' > "$transcript"
  tmux set-option -p -t "$pane" @devws_transcript "$transcript"
  [ "$("$SCRIPTS/agent_cli.sh" usage "$pane")" = "1000 500 3200 0.02 0" ] ||
    fail "a Claude conversation's usage: $("$SCRIPTS/agent_cli.sh" usage "$pane")"

  printf '{"type":"turn_context","payload":{"turn_id":"t1","model":"gpt-5.6-sol"}}\n{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10000,"cached_input_tokens":4000,"output_tokens":800,"total_tokens":10800},"last_token_usage":{"input_tokens":1,"total_tokens":1},"model_context_window":200000}}}\n' > "$transcript"
  [ "$("$SCRIPTS/agent_cli.sh" usage "$pane")" = "6000 800 4000 0.04 0" ] ||
    fail "a Codex conversation's usage: $("$SCRIPTS/agent_cli.sh" usage "$pane")"

  [ "$(status_of "$SCRIPTS/agent_cli.sh" usage %9999)" = 4 ] || fail "usage of a gone pane is not exit 4"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" usage)" = 2 ] || fail "usage accepted no pane"
}

test_limits_are_the_ones_the_header_shows() {
  local logs="$HOME/.codex/sessions/2026/11/03"
  tmux new-session -d -s limits -x 100 -y 30 'sleep 600'

  [ "$("$SCRIPTS/agent_cli.sh" limits claude)" = "- -" ] || fail "unknown Claude limits are not - -"
  tmux set-option -g @devws_limits_claude "42 63"
  [ "$("$SCRIPTS/agent_cli.sh" limits claude)" = "42 63" ] || fail "Claude's limits are not its status line's"

  [ "$("$SCRIPTS/agent_cli.sh" limits codex)" = "- -" ] || fail "Codex limits with no logs are not - -"
  mkdir -p "$logs"
  printf '{"timestamp":"2026-11-03T12:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":10},"last_token_usage":{"input_tokens":5,"total_tokens":1},"model_context_window":258400},"rate_limits":{"primary":{"used_percent":37.6,"window_minutes":300,"resets_at":4102444800},"secondary":{"used_percent":12.2,"window_minutes":10080,"resets_at":4102444800}}}}\n' \
    > "$logs/rollout-limits.jsonl"
  [ "$("$SCRIPTS/agent_cli.sh" limits codex)" = "37 12" ] || fail "Codex limits are not its log's: $("$SCRIPTS/agent_cli.sh" limits codex)"

  [ "$(status_of "$SCRIPTS/agent_cli.sh" limits nobody)" = 1 ] || fail "limits accepted an unregistered agent"
  [ "$(status_of "$SCRIPTS/agent_cli.sh" limits)" = 2 ] || fail "limits accepted no agent"
}

# The keys of the sidebar's rows, in order: a record starts with its key.
sidebar_keys() {
  DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows | tr '\0' '\n' |
    grep -E '^(workspace|task|group|agent):' | cut -f1
}

# A group's workspace of dev-t1-api's team, with one agent: its pane.
team_group() {
  local session="$1" group="$2" id="$3" pane
  tmux new-session -d -s "$session" 'sleep 600'
  tmux set-option -t "=$session:" @devws_task team-1
  tmux set-option -t "=$session:" @devws_team_of dev-t1-api
  tmux set-option -t "=$session:" @devws_group "$group"
  pane="$(tmux display-message -p -t "=$session:" '#{pane_id}')"
  tmux set-option -p -t "$pane" @devagentics_agent "$id"
  printf '%s' "$pane"
}

test_the_sidebar_groups_a_tasks_workspaces() {
  local session expected peeked senior agent rows
  for session in dev-plain-a dev-t1-api dev-t1-web dev-plain-b dev-t2-x; do
    tmux new-session -d -s "$session" 'sleep 600'
  done
  tmux set-option -t =dev-t1-api: @devws_task team-1
  tmux set-option -t =dev-t1-web: @devws_task team-1
  tmux set-option -t =dev-t2-x: @devws_task team-2
  # A team's agent peeked in its workspace, and its groups' workspaces, with
  # their agents, are listed under it as a tree.
  peeked="$(tmux split-window -d -t =dev-t1-api: -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$peeked" @devagentics_agent planner-2
  agent="$(team_group dev-t1-api-floor Floor hammer-1)"
  tmux set-option -p -t "$agent" @devws_pane_state waiting
  tmux set-option -w -t =dev-t1-api-floor: @devws_tab_alert 1
  senior="$(team_group dev-t1-api-seniors Seniors ui-designer-1)"

  expected="workspace:dev-plain-a
workspace:dev-plain-b
task:team-1
workspace:dev-t1-api
agent:$peeked
group:dev-t1-api-seniors
agent:$senior
group:dev-t1-api-floor
agent:$agent
workspace:dev-t1-web
task:team-2
workspace:dev-t2-x"
  [ "$(sidebar_keys)" = "$expected" ] || fail "the sidebar's rows are not grouped by task: $(sidebar_keys | tr '\n' ' ')"

  # Without their colors.
  rows="$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows | tr '\0' '\n' | sed $'s/\033\\[[0-9;]*m//g')"
  grep -q "^agent:$peeked.*◦ planner-2.* · peeked" <<<"$rows" || fail "the peeked agent's row: $rows"
  grep -q "^group:dev-t1-api-seniors.*├ Seniors" <<<"$rows" || fail "the Seniors row: $rows"
  grep -q "^agent:$senior.*│  ◦ ui-designer-1" <<<"$rows" || fail "the senior's row: $rows"
  grep -q "^group:dev-t1-api-floor.*└ Floor.*●" <<<"$rows" || fail "the last group's row, with its alert: $rows"
  grep "^agent:$agent" <<<"$rows" | grep -q "   ◦ hammer-1.*● needs you" || fail "the floor agent's row: $rows"
  ! grep "^agent:$agent" <<<"$rows" | grep -q 'Floor' || fail "an agent under its group repeats the group's name"

  # fzf is asked to put the cursor on an item by its place among all of them,
  # the headings included.
  [ "$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --position dev-t1-web)" = 10 ] ||
    fail "the cursor position ignores the task headings and the agents"
  [ "$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --position dev-t1-api-floor)" = 8 ] ||
    fail "the cursor is not put on the group you are in"
  [ "$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --position dev-plain-b)" = 2 ] ||
    fail "the position of a workspace without a task changed"

  # A group whose team is gone is a workspace of its own again.
  tmux kill-session -t dev-t1-api
  sidebar_keys | grep -qx 'workspace:dev-t1-api-floor' || fail "a group without its team is not listed: $(sidebar_keys | tr '\n' ' ')"
  for session in dev-plain-a dev-t1-web dev-plain-b dev-t2-x dev-t1-api-floor dev-t1-api-seniors; do tmux kill-session -t "$session"; done
}

test_the_runner_appends_the_args_file
test_the_runner_refuses_an_unreadable_args_file
test_state_follows_the_agent
test_send_types_into_an_idle_agent_only
test_mark_sets_the_state_the_sidebar_shows
test_stop_ends_the_agent_and_clears_its_state
test_spawn_adds_an_agent_beside_a_pane_without_taking_the_focus
test_run_replaces_the_agent_in_its_pane
test_kind_names_the_program_an_agent_runs
test_limits_are_the_ones_the_header_shows
test_usage_is_what_the_panes_conversation_used
test_the_sidebar_groups_a_tasks_workspaces

printf 'Agent CLI tests passed\n'
