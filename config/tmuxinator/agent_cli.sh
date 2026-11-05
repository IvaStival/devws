#!/usr/bin/env bash
# The agent commands of `devws agent …`: how a program (devAgentics) reads and
# drives the agents of devws workspaces without touching tmux options itself.
# A pane is named by its tmux id (%12). Their output and exit codes are stable;
# docs/guide/commands.md lists them.
#
#   agent_cli.sh state <pane>             working, waiting, done or idle; gone
#                                         when the pane no longer exists
#   agent_cli.sh send [--force] <pane> <text>
#                                         types the text (- reads it from
#                                         stdin) as one message, then Enter,
#                                         when the pane's own agent is done or
#                                         idle; a text with line breaks is
#                                         pasted, not typed line by line. Exit 3 "busy (<state>)" when it
#                                         works or waits on you, 4 when the
#                                         pane is gone. --force sends anyway.
#   agent_cli.sh mark <pane> waiting|done sets the agent's state as its hooks
#                                         would: the sidebar and the
#                                         notification react
#   agent_cli.sh usage <pane>             what the agent used: "<input> <output>
#                                         <cache> <US$> <unpriced>" (tokens, API
#                                         prices), - when not known yet
#   agent_cli.sh spawn <session> <agent> [--beside <pane>] [--position right|left|below|above] [--agent-args <file>] [--label <text>] [--place team] [--group <name>] [--folder <dir>]
#                                         adds an extra agent panel to the
#                                         workspace and prints its pane id;
#                                         the focus stays where it was.
#                                         --place team: in the team area of an
#                                         agent team (devws agentic), a grid
#                                         right of the leader
#                                         --agent-args: arguments for the
#                                         program, NUL-separated (agent_runner.sh).
#                                         --label: what the panel's header says
#                                         in place of "Agent"
#                                         --group: in an agent team's group, a
#                                         workspace of its own (dev-app-floor),
#                                         four to a group (team_groups.sh)
#                                         --folder: start it in that folder (a
#                                         lane's worktree), not the workspace's
#   agent_cli.sh go <pane>                shows a group's agent: its workspace,
#                                         focused on it
#   agent_cli.sh peek <pane>              brings a group's agent under the
#                                         board for a look; again sends it back
#   agent_cli.sh run <pane> <agent> [--agent-args <file>]
#                                         starts <agent> in an agent pane, in
#                                         place of the one running there; the
#                                         pane keeps its place and role
#   agent_cli.sh kind <agent>             the program the agent runs: claude or
#                                         codex (an account like claude_work
#                                         runs claude); 1 when unregistered
#   agent_cli.sh limits <agent>           "<5h> <week>": the per cent of its
#                                         plan's limits that account has used,
#                                         a - for what is not known
#   agent_cli.sh stop <pane>              asks the agent to exit, closes the
#                                         pane if it did not, and updates the
#                                         workspace's state
#
# Exit 2 is a usage error, 1 another failure.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
[ -r "$script_dir/.env" ] && . "$script_dir/.env"
# shellcheck source=agents.sh
. "$script_dir/agents.sh"

fail() {
  printf 'devws agent: %s\n' "$*" >&2
  exit "${code:-1}"
}

usage_fail() {
  code=2 fail "$*"
}

# A pane id only: any other name would resolve to a window or a session.
require_pane() {
  case "${1:-}" in
    %[0-9]*) ;;
    *) usage_fail "not a pane id: ${1:-} (like %12)" ;;
  esac
}

# tmux answers a pane that is gone with an empty line and success, so the id it
# prints is what counts.
pane_exists() {
  [ "$(tmux display-message -p -t "$1" '#{pane_id}' 2>/dev/null)" = "$1" ]
}

pane_session() {
  tmux display-message -p -t "$1" '#{session_name}'
}

pane_state() {
  local state
  pane_exists "$1" || { printf 'gone\n'; return; }
  state="$(tmux show-options -pqv -t "$1" @devws_pane_state 2>/dev/null)"
  printf '%s\n' "${state:-idle}"
}

state() {
  [ "$#" -eq 1 ] || usage_fail "usage: devws agent state <pane>"
  require_pane "$1"
  pane_state "$1"
}

# One line is typed, as the menu's actions always did. A message with line
# breaks goes in as a bracketed paste, so it stays one message instead of
# being sent line by line; its buffer is named, and removed by the paste, so
# concurrent sends do not meet.
send() {
  local force="" pane text current buffer
  if [ "${1:-}" = --force ]; then
    force=1
    shift
  fi
  [ "$#" -eq 2 ] || usage_fail "usage: devws agent send [--force] <pane> <text|->"
  pane="$1"
  require_pane "$pane"
  text="$2"
  [ "$text" != - ] || text="$(cat)"

  current="$(pane_state "$pane")"
  case "$current" in
    gone) code=4 fail "$pane is gone" ;;
    working | waiting)
      [ -n "$force" ] || code=3 fail "busy ($current)"
      ;;
  esac

  case "$text" in
    *$'\n'*)
      buffer="devws-send-$$"
      printf '%s' "$text" | tmux load-buffer -b "$buffer" - || fail "could not load the text"
      tmux paste-buffer -p -r -d -b "$buffer" -t "$pane" || fail "could not paste into $pane"
      ;;
    *) tmux send-keys -t "$pane" -l "$text" ;;
  esac
  tmux send-keys -t "$pane" Enter
}

mark() {
  local pane state session
  [ "$#" -eq 2 ] || usage_fail "usage: devws agent mark <pane> waiting|done"
  pane="$1"
  state="$2"
  require_pane "$pane"
  case "$state" in
    waiting | done) ;;
    *) usage_fail "mark sets waiting or done, not: $state" ;;
  esac
  pane_exists "$pane" || code=4 fail "$pane is gone"
  session="$(pane_session "$pane")"
  "$script_dir/agent_status.sh" "$session" "$state" "$pane"
}

spawn() {
  local session agent position=right beside="" args_file="" label="" place="" group="" folder="" options=()
  [ "$#" -ge 2 ] || usage_fail "usage: devws agent spawn <session> <agent> [--beside <pane>] [--position p] [--agent-args <file>] [--label <text>] [--place team] [--group <name>] [--folder <dir>]"
  session="$1"
  agent="$2"
  shift 2
  while [ "$#" -gt 0 ]; do
    [ "$#" -ge 2 ] || usage_fail "$1 needs a value"
    case "$1" in
      --beside) beside="$2" ;;
      --position) position="$2" ;;
      --agent-args) args_file="$2" ;;
      --label) label="$2" ;;
      --place) place="$2" ;;
      --group) group="$2" ;;
      --folder) folder="$2" ;;
      *) usage_fail "unknown option: $1" ;;
    esac
    shift 2
  done
  case "$place" in
    "" | team) ;;
    *) usage_fail "unknown place: $place (team)" ;;
  esac
  [ -z "$place" ] || [ -z "$beside" ] || usage_fail "--place and --beside both say where it goes"
  [ -z "$group" ] || [ -z "$place$beside" ] || usage_fail "--group and --place or --beside both say where it goes"
  [ -z "$place" ] || options+=(--place "$place")
  [ -z "$group" ] || options+=(--group "$group")
  [ -z "$folder" ] || options+=(--folder "$folder")
  [ -z "$label" ] || options+=(--label "$label")
  [ -z "$beside" ] || options+=(--beside "$beside")
  [ -z "$args_file" ] || options+=(--args-file "$args_file")
  "$script_dir/new_panel.sh" "$session" "$position" agent "$agent" --no-focus ${options[@]+"${options[@]}"}
}

# respawn-pane starts the runner in the pane's own folder in place of whatever
# ran there; the runner marks the role, and the pane's old state goes.
run() {
  local pane agent args_file="" role session root command
  [ "$#" -ge 2 ] || usage_fail "usage: devws agent run <pane> <agent> [--agent-args <file>]"
  pane="$1"
  agent="$2"
  shift 2
  if [ "$#" -gt 0 ]; then
    [ "$#" -eq 2 ] && [ "$1" = --agent-args ] || usage_fail "usage: devws agent run <pane> <agent> [--agent-args <file>]"
    args_file="$2"
  fi
  require_pane "$pane"
  pane_exists "$pane" || code=4 fail "$pane is gone"
  role="$(tmux show-options -pqv -t "$pane" @devws_role)"
  case "$role" in
    agent | extra-agent) ;;
    *) usage_fail "$pane is not an agent pane" ;;
  esac
  agent_kind "$agent" >/dev/null 2>&1 || fail "unknown agent: $agent (register it in the devws menu)"
  session="$(pane_session "$pane")"
  root="$(tmux show-options -qv -t "=$session:" @devws_root)"

  command="$(printf '%q %q' "$script_dir/agent_runner.sh" "$agent")"
  [ "$role" = agent ] || command="$command --extra"
  [ -z "$args_file" ] || command="$command $(printf -- '--args-file %q' "$args_file")"
  # The sidebar and the saved workspace name the main agent.
  [ "$role" != agent ] || tmux set-option -t "=$session:" @devws_agent "$agent"
  tmux respawn-pane -k -t "$pane" ${root:+-c "$root"} "$command" || fail "could not start $agent in $pane"
  "$script_dir/agent_status.sh" "$session" idle "$pane"
}

kind() {
  [ "$#" -eq 1 ] || usage_fail "usage: devws agent kind <agent>"
  agent_kind "$1" 2>/dev/null || fail "unknown agent: $1 (register it in the devws menu)"
}

limits() {
  local value
  [ "$#" -eq 1 ] || usage_fail "usage: devws agent limits <agent>"
  agent_kind "$1" >/dev/null 2>&1 || fail "unknown agent: $1 (register it in the devws menu)"
  value="$(agent_limits "$1")"
  printf '%s\n' "${value:-- -}"
}

# What the pane's agent has used: "<input> <output> <cache> <US$> <unpriced>"
# (tokens; dollars at API prices), "-" while it has not said which conversation
# it is in.
usage() {
  local transcript value
  [ "$#" -eq 1 ] || usage_fail "usage: devws agent usage <pane>"
  require_pane "$1"
  pane_exists "$1" || code=4 fail "$1 is gone"
  transcript="$(tmux show-options -pqv -t "$1" @devws_transcript 2>/dev/null)"
  value=""
  [ -z "$transcript" ] || value="$("$script_dir/header_tokens.sh" usage "$transcript")"
  printf '%s\n' "${value:--}"
}

# Ctrl-C twice is how Claude Code and Codex quit; what is still there after a
# few seconds is closed.
stop() {
  local pane session attempt
  [ "$#" -eq 1 ] || usage_fail "usage: devws agent stop <pane>"
  pane="$1"
  require_pane "$pane"
  pane_exists "$pane" || code=4 fail "$pane is gone"
  session="$(pane_session "$pane")"
  tmux send-keys -t "$pane" C-c
  sleep 0.2
  pane_exists "$pane" && tmux send-keys -t "$pane" C-c
  for attempt in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
    pane_exists "$pane" || break
    sleep 0.2
  done
  pane_exists "$pane" && tmux kill-pane -t "$pane"
  "$script_dir/agent_status.sh" "$session" idle "$pane"
}

command="${1:-}"
[ "$#" -eq 0 ] || shift
case "$command" in
  state) state "$@" ;;
  send) send "$@" ;;
  mark) mark "$@" ;;
  stop) stop "$@" ;;
  spawn) spawn "$@" ;;
  run) run "$@" ;;
  limits) limits "$@" ;;
  usage) usage "$@" ;;
  kind) kind "$@" ;;
  go | peek)
    [ "$#" -eq 1 ] || usage_fail "usage: devws agent $command <pane>"
    exec "$script_dir/team_groups.sh" "$command" "$1"
    ;;
  *) usage_fail "usage: devws agent state|send|mark|stop|spawn|run|limits|usage|kind|go|peek …" ;;
esac
