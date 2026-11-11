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
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home/.tmuxinator" "$TEST_ROOT/project" "$TEST_ROOT/started" "$TEST_ROOT/tmp"
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
for script in "$SCRIPTS_SRC"/*.sh "$SCRIPTS_SRC"/*.pl "$SCRIPTS_SRC"/*.json "$SCRIPTS_SRC"/*.md "$SCRIPTS_SRC"/*.tsv; do
  ln -s "$script" "$TEST_ROOT/home/.tmuxinator/$(basename "$script")"
done
printf 'DEVWS_AGENTS="claude codex"\n' > "$TEST_ROOT/home/.tmuxinator/.env"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" TMPDIR="$TEST_ROOT/tmp"
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

# Codex asks "Trust this folder?" in a folder its config does not trust, and
# each task's lanes are new folders: devws trusts the ones it made, only those.
test_codex_trusts_the_task_folders_devws_made() {
  local lane="$TEST_ROOT/home/.devagentics/tasks/t1/lanes/ops" config="$TEST_ROOT/home/.codex/config.toml" outside="$TEST_ROOT/project"
  mkdir -p "$lane" "$(dirname "$config")"
  printf 'model = "gpt-6.1-sol"\n' > "$config"
  lane="$(cd "$lane" && pwd -P)"

  rm -f "$TEST_ROOT"/started/codex.*
  tmux new-session -d -s trust-lane -x 100 -y 30 -c "$lane" "$SCRIPTS/agent_runner.sh codex"
  wait_until started_args codex >/dev/null || fail "the runner did not start codex in a lane"
  [ "$(head -n 1 "$config")" = 'model = "gpt-6.1-sol"' ] || fail "trusting a lane changed the config's own settings: $(cat "$config")"
  grep -qxF "[projects.\"$lane\"]" "$config" && grep -A1 -xF "[projects.\"$lane\"]" "$config" | grep -qx 'trust_level = "trusted"' ||
    fail "a lane devws made is not trusted: $(cat "$config")"

  rm -f "$TEST_ROOT"/started/codex.*
  tmux new-session -d -s trust-again -x 100 -y 30 -c "$lane" "$SCRIPTS/agent_runner.sh codex"
  wait_until started_args codex >/dev/null || fail "the runner did not start codex again"
  [ "$(grep -cxF "[projects.\"$lane\"]" "$config")" = 1 ] || fail "a trusted lane was added twice: $(cat "$config")"

  rm -f "$TEST_ROOT"/started/codex.*
  tmux new-session -d -s trust-outside -x 100 -y 30 -c "$outside" "$SCRIPTS/agent_runner.sh codex"
  wait_until started_args codex >/dev/null || fail "the runner did not start codex outside the tasks"
  ! grep -qF "$(cd "$outside" && pwd -P)\"]" "$config" || fail "devws trusted a folder it did not make: $(cat "$config")"
  tmux kill-session -t trust-lane; tmux kill-session -t trust-again; tmux kill-session -t trust-outside
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
  local session expected peeked senior agent rows record display leader normal_agent original_width long_agent_id row_limit
  for session in dev-plain-a dev-t1-api dev-t1-web dev-plain-b dev-t2-x; do
    tmux new-session -d -s "$session" 'sleep 600'
  done
  tmux set-option -t =dev-t1-api: @devws_task team-1
  tmux set-option -t =dev-t1-api: @devws_template agentic
  tmux set-option -t =dev-t1-api: @devws_root "$TEST_ROOT/projects/api"
  tmux set-option -t =dev-t1-api: @devws_agent hidden-program-sentinel
  tmux set-option -t =dev-t1-api: @devws_editor hidden-editor-sentinel
  tmux set-option -t =dev-t1-web: @devws_task team-1
  tmux set-option -t =dev-t1-web: @devws_agent normal-program-sentinel
  tmux set-option -t =dev-t1-web: @devws_editor normal-editor-sentinel
  tmux set-option -t =dev-t2-x: @devws_task team-2
  leader="$(tmux display-message -p -t =dev-t1-api: '#{pane_id}')"
  tmux set-option -p -t "$leader" @devws_role agent
  tmux set-option -p -t "$leader" @devws_model root-runtime-model
  tmux set-option -p -t "$leader" @devws_model_configured root-configured-model
  normal_agent="$(tmux display-message -p -t =dev-t1-web: '#{pane_id}')"
  tmux set-option -p -t "$normal_agent" @devws_role agent
  tmux set-option -p -t "$normal_agent" @devws_model normal-runtime-model
  tmux set-option -p -t "$normal_agent" @devws_model_configured normal-configured-model
  # A team's agent peeked in its workspace, and its groups' workspaces, with
  # their agents, are listed under it as a tree.
  peeked="$(tmux split-window -d -t =dev-t1-api: -P -F '#{pane_id}' 'sleep 600')"
  tmux set-option -p -t "$peeked" @devagentics_agent planner-2
  agent="$(team_group dev-t1-api-floor Floor hammer-1)"
  tmux set-option -p -t "$agent" @devws_pane_state waiting
  tmux set-option -p -t "$agent" @devws_model floor-runtime-model
  tmux set-option -p -t "$agent" @devws_model_configured floor-configured-model
  tmux set-option -w -t =dev-t1-api-floor: @devws_tab_alert 1
  senior="$(team_group dev-t1-api-seniors Seniors ui-designer-1)"
  tmux set-option -p -t "$senior" @devws_pane_state working
  tmux set-option -p -t "$senior" @devws_model_configured senior-configured-model

  export DEVWS_PICKER_CURRENT=dev-t1-api
  export TMUX_PANE="$(tmux display-message -p -t =dev-t1-api: '#{pane_id}')"
  expected="workspace:dev-plain-a
workspace:dev-plain-b
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

  # Team rows keep name/status above a dim model line; normal workspaces keep
  # branch/agent/editor details with a model below when known. No metadata leaks.
  DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows > "$TEST_ROOT/display.rows"
  while IFS= read -r -d '' record; do
    [[ "$record" == *$'\t'* ]] || fail "a sidebar record has no display field"
    display="${record#*$'\t'}"
    [[ "$display" != *$'\t'* ]] || fail "a sidebar record leaks hidden metadata: $record"
    case "$record" in
      task:*|group:*|agent:$peeked$'\t'*) expected_lines=0 ;;
      agent:*) expected_lines=1 ;;
      workspace:dev-t1-api$'\t'*) expected_lines=2 ;;
      workspace:dev-t1-web$'\t'*) expected_lines=3 ;;
      workspace:*) expected_lines=2 ;;
    esac
    [ "$(printf '%s' "$display" | wc -l | tr -d ' ')" = "$expected_lines" ] ||
      fail "sidebar record has incorrect name/model/detail line count: $record"
  done < "$TEST_ROOT/display.rows"
  rows="$(tr '\0' '\n' < "$TEST_ROOT/display.rows")"
  ! grep -qE 'hidden-(program|editor)-sentinel' <<<"$rows" ||
    fail "a team row includes program/editor detail: $rows"
  grep -q 'normal-program-sentinel.*normal-editor-sentinel' <<<"$rows" ||
    fail "a normal task workspace lost its program/editor detail: $rows"

  # Without their colors.
  rows="$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows | tr '\0' '\n' | sed $'s/\033\\[[0-9;]*m//g')"
  grep -q "^agent:$peeked.*◦ planner-2" <<<"$rows" || fail "the peeked agent's row: $rows"
  grep '^workspace:dev-t1-api' <<<"$rows" | grep -q 'dev-t1-api' && grep -q '^ *root-runtime-model' <<<"$rows" || fail "the team root lost its current model"
  grep -A1 "^agent:$agent" <<<"$rows" | tail -1 | grep -q 'floor-runtime-model' || fail "an agent's runtime model is not displayed"
  grep -A1 "^agent:$senior" <<<"$rows" | tail -1 | grep -q 'senior-configured-model' || fail "an agent's configured model is not the fallback"
  ! grep "^agent:$peeked" <<<"$rows" | grep -q 'planner-2 ·' || fail "an agent without a model has a dangling model separator"
  ! grep -qE '(root|floor|normal)-configured-model' <<<"$rows" || fail "a configured model overrode a runtime model"
  grep -q 'normal-program-sentinel · normal-editor-sentinel' <<<"$rows" && grep -q '^ *normal-runtime-model' <<<"$rows" || fail "a normal workspace lost its runtime model"
  ! grep -q ' · peeked' <<<"$rows" || fail "the sidebar includes pane-placement detail: $rows"
  # A team leads its task: the project's name, with the task below it, in
  # place of a separate heading that would list the project twice.
  grep -A1 '^workspace:dev-t1-api' <<<"$rows" | head -1 | grep -q '▾ api' &&
    grep -A1 '^workspace:dev-t1-api' <<<"$rows" | tail -1 | grep -qx ' *team-1' ||
    fail "the team does not lead its task with the project and task: $(grep -A1 '^workspace:dev-t1-api' <<<"$rows")"
  grep -q "^group:dev-t1-api-seniors.*├ ▾ Seniors" <<<"$rows" || fail "the Seniors row: $rows"
  grep -q "^agent:$senior.*│  ◦ ui-designer-1" <<<"$rows" || fail "the senior's row: $rows"
  # A group with another below it keeps its │ down its agents' model lines.
  grep -A1 "^agent:$senior" <<<"$rows" | tail -1 | grep -q '^    │    senior-configured-model' ||
    fail "the senior's model line breaks the tree: $(grep -A1 "^agent:$senior" <<<"$rows")"
  grep -A1 "^agent:$agent" <<<"$rows" | tail -1 | grep -q '^         floor-runtime-model' ||
    fail "the last group's model line: $(grep -A1 "^agent:$agent" <<<"$rows")"
  grep -q "^group:dev-t1-api-floor.*└ ▾ Floor.*●" <<<"$rows" || fail "the last group's row, with its alert: $rows"
  grep "^agent:$agent" <<<"$rows" | grep -q "   ◦ hammer-1.*● needs you" || fail "the floor agent's row: $rows"
  ! grep "^agent:$agent" <<<"$rows" | cut -f2 | grep -q 'Floor' || fail "an agent under its group repeats the group's name"

  # Normal-workspace models use the same runtime/configured/missing precedence.
  tmux set-option -pu -t "$normal_agent" @devws_model
  rows="$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows | tr '\0' '\n')"
  grep -q 'normal-program-sentinel · normal-editor-sentinel' <<<"$rows" && grep -q 'normal-configured-model' <<<"$rows" || fail "a normal workspace lost its configured model fallback"
  tmux set-option -pu -t "$normal_agent" @devws_model_configured
  rows="$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows | tr '\0' '\n')"
  grep -q 'normal-program-sentinel · normal-editor-sentinel' <<<"$rows" || fail "a normal workspace without model shows a placeholder or dangling separator"

  # Narrow team rows reserve their entire status suffix and keep full keys/search
  # identities even when a long display name must be shortened around its model.
  original_width="$(tmux display-message -p -t "$TMUX_PANE" '#{window_width}')"
  tmux resize-window -t =dev-t1-api: -x 50
  row_limit="$(tmux display-message -p -t "$TMUX_PANE" '#{pane_width}')"
  row_limit=$((row_limit - 2))
  long_agent_id=hammer-super-long-original-identity-for-rendering-check
  tmux set-option -p -t "$agent" @devagentics_agent "$long_agent_id"
  tmux set-option -p -t "$agent" @devws_model claude-haiku-5-5
  rows="$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows | tr '\0' '\n' | sed $'s/\033\\[[0-9;]*m//g')"
  record="$(grep "^agent:$agent" <<<"$rows")"
  display="${record#*$'\t'}"
  [ "${#display}" -le "$row_limit" ] || fail "a narrow waiting row exceeds its pane: $display"
  [[ "$display" == *'● needs you' ]] || fail "a long agent name hid the complete waiting status: $display"
  [[ "$display" == *'…'* ]] && grep -A1 "^agent:$agent" <<<"$rows" | tail -1 | grep -q 'claude-haiku-5-5'  || fail "a narrow row failed to retain its model and truncate its name: $display"
  tmux set-option -p -t "$agent" @devws_pane_state done
  rows="$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows | tr '\0' '\n' | sed $'s/\033\\[[0-9;]*m//g')"
  record="$(grep "^agent:$agent" <<<"$rows")"
  display="${record#*$'\t'}"
  [ "${#display}" -le "$row_limit" ] || fail "a narrow done row exceeds its pane: $display"
  [[ "$display" == *'✓ done' ]] && grep -A1 "^agent:$agent" <<<"$rows" | tail -1 | grep -q 'claude-haiku-5-5'  || fail "a long agent name hid its done status or model: $display"
  DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --search "$long_agent_id" > "$TEST_ROOT/narrow.rows"
  tr '\0' '\n' < "$TEST_ROOT/narrow.rows" | grep -q "^agent:$agent" || fail "truncation changed the agent's key or search identity"
  DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --search '' >/dev/null
  tmux set-option -p -t "$agent" @devagentics_agent hammer-1
  tmux set-option -p -t "$agent" @devws_model floor-runtime-model
  tmux set-option -p -t "$agent" @devws_pane_state waiting
  tmux resize-window -t =dev-t1-api: -x "$original_width"

  # fzf is asked to put the cursor on an item by its place among all of them,
  # the headings included.
  [ "$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --position dev-t1-web)" = 9 ] ||
    fail "the cursor position ignores the task headings and the agents"
  [ "$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --position dev-t1-api-floor)" = 7 ] ||
    fail "the cursor is not put on the group you are in"
  [ "$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --position dev-plain-b)" = 2 ] ||
    fail "the position of a workspace without a task changed"

  # Collapsing persists across redraws, and summaries retain hidden activity.
  "$SCRIPTS/session_picker.sh" --collapse group:dev-t1-api-floor
  ! sidebar_keys | grep -qx "agent:$agent" || fail "a collapsed group still shows its agent"
  rows="$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows | tr '\0' '\n')"
  grep '^group:dev-t1-api-floor' <<<"$rows" | grep -q '●1' || fail "a collapsed group's attention count disappeared"
  "$SCRIPTS/session_picker.sh" --collapse workspace:dev-t1-api
  ! sidebar_keys | grep -q '^group:' || fail "a collapsed team still shows its groups"
  rows="$(DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --rows | tr '\0' '\n')"
  grep '^workspace:dev-t1-api' <<<"$rows" | grep -q '⠋1 ●1' || fail "a collapsed team lost its working/attention counts"
  "$SCRIPTS/session_picker.sh" --expand workspace:dev-t1-api
  ! sidebar_keys | grep -qx "agent:$agent" || fail "expanding a team forgot its group's manual collapse"

  # Search finds hidden descendants and their parent paths without changing expansion.
  DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --search hammer > "$TEST_ROOT/search.rows"
  tr '\0' '\n' < "$TEST_ROOT/search.rows" | grep -q "^agent:$agent" || fail "search did not reveal a hidden descendant"
  rows="$(tr '\0' '\n' < "$TEST_ROOT/search.rows")"
  grep -q '^workspace:dev-t1-api' <<<"$rows" || fail "the matching agent has no workspace path"
  grep -q '^group:dev-t1-api-floor' <<<"$rows" || fail "the matching agent has no group path"
  ! grep -q "^agent:$senior" <<<"$rows" || fail "search included an unrelated sibling"
  python3 - "$TEST_ROOT/search.rows" <<'PYSEARCH'
from pathlib import Path
import sys
for record in Path(sys.argv[1]).read_text().split('\0'):
    if record:
        assert len(record.split('\t')) == 2, f'search leaked descendant metadata: {record!r}'
PYSEARCH
  ! grep '^workspace:dev-t1-api' <<<"$rows" | grep -q hammer || fail "a parent repeats its hidden agent index"
  DEVWS_PICKER_ROWS="$TEST_ROOT/rows" "$SCRIPTS/session_picker.sh" --search '' >/dev/null
  ! sidebar_keys | grep -qx "agent:$agent" || fail "clearing search discarded manual collapse"

  # A picker that never saw the team's workspace still shows every group's
  # agents: groups start expanded wherever you enter the team from.
  tmux set-option -pu -t "$TMUX_PANE" @devws_picker_expansion
  tmux set-option -pu -t "$TMUX_PANE" @devws_picker_current
  DEVWS_PICKER_CURRENT=dev-t1-api-floor sidebar_keys | grep -qx "agent:$senior" ||
    fail "entering one group of a team hid another group's agents"
  "$SCRIPTS/session_picker.sh" --collapse group:dev-t1-api-floor

  # Switching away and back automatically reopens the current workspace's path.
  export DEVWS_PICKER_CURRENT=dev-plain-a
  sidebar_keys >/dev/null
  export DEVWS_PICKER_CURRENT=dev-t1-api-floor
  sidebar_keys | grep -qx "agent:$agent" || fail "entering a group did not open its parent path"

  # A task added after the last redraw can be reached from its new heading.
  tmux new-session -d -s dev-live-task 'sleep 600'
  tmux set-option -t =dev-live-task: @devws_task live-task
  [ "$("$SCRIPTS/session_picker.sh" --task-workspace live-task)" = dev-live-task ] ||
    fail "navigation did not resolve a task added after the last snapshot"
  tmux kill-session -t dev-live-task
  [ -z "$("$SCRIPTS/session_picker.sh" --task-workspace live-task)" ] ||
    fail "navigation kept the workspace of a closed task"

  # A group whose team is gone is a workspace of its own again.
  tmux kill-session -t dev-t1-api
  sidebar_keys | grep -qx 'workspace:dev-t1-api-floor' || fail "a group without its team is not listed: $(sidebar_keys | tr '\n' ' ')"
  unset DEVWS_PICKER_CURRENT TMUX_PANE
  for session in dev-plain-a dev-t1-web dev-plain-b dev-t2-x dev-t1-api-floor dev-t1-api-seniors; do tmux kill-session -t "$session"; done
}

test_sidebar_models_on_secondary_lines() {
  local team=dev-model-team group=dev-model-team-floor leader worker
  tmux new-session -d -s "$team" -x 50 -y 40 'sleep 600'
  tmux set-option -t "=$team:" @devws_template agentic
  tmux set-option -t "=$team:" @devws_task model-task
  tmux set-option -t "=$team:" @devws_agent_state done
  leader="$(tmux display-message -p -t "=$team:" '#{pane_id}')"
  tmux set-option -p -t "$leader" @devws_role agent
  tmux set-option -p -t "$leader" @devws_model claude-opus-5-5
  tmux new-session -d -s "$group" -x 50 -y 40 'sleep 600'
  tmux set-option -t "=$group:" @devws_team_of "$team"
  tmux set-option -t "=$group:" @devws_group Floor
  worker="$(tmux display-message -p -t "=$group:" '#{pane_id}')"
  tmux set-option -p -t "$worker" @devagentics_agent hammer-claude-2
  tmux set-option -p -t "$worker" @devws_pane_state done
  tmux set-option -p -t "$worker" @devws_model claude-haiku-5-5
  local model_test_width
  for model_test_width in 50 25; do
    tmux resize-window -t "=$team:" -x "$model_test_width"
    DEVWS_PICKER_CURRENT="$team" TMUX_PANE="$leader" DEVWS_PICKER_ROWS="$TEST_ROOT/model-lines.rows" \
      "$SCRIPTS/session_picker.sh" --rows > "$TEST_ROOT/model-lines.output"
    python3 - "$TEST_ROOT/model-lines.output" "$worker" "$model_test_width" <<'PYTEST'
import re
import sys
from pathlib import Path
width = int(sys.argv[3])
records = dict(record.split('\t', 1) for record in Path(sys.argv[1]).read_text().split('\0') if record)
# The team leads its task, so its task sits between its name and its model.
for key, name, model, line_count in [
    ('workspace:dev-model-team', 'model-team' if width < 36 else 'dev-model-team', 'claude-opus-5-5', 3),
    ('agent:' + sys.argv[2], 'hammer-claude-2', 'claude-haiku-5-5', 2),
]:
    raw = records[key].splitlines()
    lines = [re.sub(r'\x1b\[[0-9;]*m', '', line) for line in raw]
    assert len(lines) == line_count, f'{key}: expected separate name/status and model lines, got {lines!r}'
    status = '✓' if width < 36 else '✓ done'
    assert name in lines[0] and status in lines[0], f'name or status cut at {width}: {lines[0]!r}'
    assert model not in lines[0] and model in lines[-1], f'model must be below name: {lines!r}'
    assert '\x1b[2m' in raw[-1], f'model should be dimmed: {raw[-1]!r}'
    assert all(len(line) <= width - 4 for line in lines), f'label overflows {width}-column pane: {lines!r}'
PYTEST
  done
  local wrapped_model=claude-model-with-a-long-version-and-date-20261108
  tmux set-option -p -t "$worker" @devws_model "$wrapped_model"
  DEVWS_PICKER_CURRENT="$team" TMUX_PANE="$leader" DEVWS_PICKER_ROWS="$TEST_ROOT/model-lines.rows" \
    "$SCRIPTS/session_picker.sh" --search "'$wrapped_model" > "$TEST_ROOT/wrapped-model.output"
  python3 - "$TEST_ROOT/wrapped-model.output" "$worker" "$wrapped_model" <<'PYWRAP'
import re
import sys
from pathlib import Path
records = dict(record.split('\t', 1) for record in Path(sys.argv[1]).read_text().split('\0') if record)
lines = [re.sub(r'\x1b\[[0-9;]*m', '', line) for line in records['agent:' + sys.argv[2]].splitlines()]
assert len(lines) > 2, f'long model was not wrapped: {lines!r}'
assert ''.join(line.strip() for line in lines[1:]) == sys.argv[3], f'model suffix lost: {lines!r}'
assert all(len(line) <= 21 for line in lines), f'wrapped model exceeds narrow pane: {lines!r}'
PYWRAP
  tmux kill-session -t "$group"
  tmux kill-session -t "$team"
}

if [ "${1:-}" = --sidebar-model-lines ]; then
  test_sidebar_models_on_secondary_lines
  printf 'Sidebar model-line tests passed\n'
  exit 0
fi

if [ "${1:-}" = --sidebar ]; then
  test_the_sidebar_groups_a_tasks_workspaces
  test_sidebar_models_on_secondary_lines
  printf 'Sidebar tests passed\n'
  exit 0
fi

test_the_runner_appends_the_args_file
test_the_runner_refuses_an_unreadable_args_file
test_codex_trusts_the_task_folders_devws_made
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
test_sidebar_models_on_secondary_lines

printf 'Agent CLI tests passed\n'
