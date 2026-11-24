#!/usr/bin/env bash
# Marks the pane as the workspace agent pane, then runs the configured agent
# and waits for it. The pane marker makes restart/switch actions safe. It
# waits rather than replacing itself (exec): an idle workspace's agent is
# frozen below this script (suspend_workspace.sh), and this script, without
# job control, never notices — the pane's shell would, and take it back.
#
#   agent_runner.sh <agent>           a name registered in .env (agents.sh)
#   agent_runner.sh <agent> --extra   an extra agent (New panel): marked
#                                     extra-agent with its agent's name, so the
#                                     main agent keeps its role
#   agent_runner.sh <agent> --resume  continues the agent's last conversation
#                                     in this folder (a pane reopened under
#                                     its runner, suspend_workspace.sh)
#   agent_runner.sh <agent> --args-file <file>
#                                     more arguments for the program, after
#                                     devws's own: NUL-separated, so one can
#                                     be a prompt with line breaks
#                                     (agent_cli.sh, devAgentics)

set -u

# A moved worktree leaves existing shells attached to its unlinked directory.
# Recover only from explicit session metadata; never launch in an arbitrary folder.
if ! /bin/pwd -P >/dev/null 2>&1; then
  workspace_root=""
  if [ -n "${TMUX_PANE:-}" ]; then
    workspace_root="$(tmux show-options -qv -t "$TMUX_PANE" @devws_root 2>/dev/null)"
  fi
  case "$workspace_root" in
    /*) [ -d "$workspace_root" ] && cd -- "$workspace_root" || workspace_root="" ;;
    *) workspace_root="" ;;
  esac
  if [ -z "$workspace_root" ]; then
    printf 'Agent working directory no longer exists. Change to the project directory and retry.\n' >&2
    exit 2
  fi
fi

agent="${1:-}"
[ "$#" -eq 0 ] || shift
extra=""
resume=""
args_file=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --extra) extra=--extra ;;
    --resume) resume=1 ;;
    --args-file)
      args_file="${2:-}"
      [ "$#" -lt 2 ] || shift
      ;;
  esac
  shift
done
link_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
[ -r "$link_dir/.env" ] && . "$link_dir/.env"
# shellcheck source=agents.sh
. "$link_dir/agents.sh"

if ! kind="$(agent_kind "$agent")"; then
  printf 'Unsupported devws agent: %s (register it in the devws menu)\n' "$agent" >&2
  exit 2
fi

extra_args=()
if [ -n "$args_file" ]; then
  if [ ! -r "$args_file" ]; then
    printf 'Cannot read the agent arguments: %s\n' "$args_file" >&2
    exit 2
  fi
  while IFS= read -r -d '' argument; do
    extra_args+=("$argument")
  done < "$args_file"
fi

# The account: the folder registered for the agent, or none for a bare name,
# whatever folder the environment came with (the module's account_env).
while IFS= read -r variable; do
  [ -z "$variable" ] || unset "$variable"
done < <(agent_account_variables)
if agent_has_folder "$agent" && ! agent_hook "$kind" account_env "$(agent_home "$agent")"; then
  printf 'Cannot run %s on its folder %s: register it with its program'"'"'s own\n' "$agent" "$(agent_home "$agent")" >&2
  exit 2
fi
# The status line records this agent's limits under its name.
export DEVWS_AGENT="$agent"

# Reviewed panels use a supervisor with piped provider input, never keystroke delivery.
if [ "${extra_args[0]:-}" = --devagentics-worker ]; then
  [ "$extra" != --extra ] || tmux set-option -p -t "$TMUX_PANE" @devws_role extra-agent
  tmux set-option -p -t "$TMUX_PANE" @devws_agent "$agent"
  executable="${extra_args[1]}"
  exec "$executable" worker-panel "${extra_args[@]:2}"
fi

# What the program needs before it starts here (the module's prepare: trust
# for a folder devAgentics made, a plugin).
agent_hook "$kind" prepare "$agent"

if [ "$extra" = --extra ]; then
  tmux set-option -p -t "$TMUX_PANE" @devws_role extra-agent
  tmux set-option -p -t "$TMUX_PANE" @devws_agent "$agent"
else
  tmux set-option -p -t "$TMUX_PANE" @devws_role agent
fi

# A respawn can reuse its pane. Never attribute the previous runtime's
# observed model or transcript to the replacement agent.
for option in @devws_startup_error @devws_model @devws_model_source @devws_model_seen @devws_model_transcript @devws_transcript @devws_model_configured \
    $(while IFS= read -r each; do agent_hook "$each" pane_options; done < <(agent_kinds)); do
  tmux set-option -p -u -t "$TMUX_PANE" "$option" 2>/dev/null
done
if [ "$extra" != --extra ]; then
  tmux set-option -u -t "$(tmux display-message -p -t "$TMUX_PANE" '#{session_id}')" @devws_agent_transcript 2>/dev/null
fi
configured_model=""
expects_model=""
for argument in ${extra_args[@]+"${extra_args[@]}"}; do
  if [ -n "$expects_model" ]; then
    configured_model="$argument"
    expects_model=""
    continue
  fi
  case "$argument" in
    --) break ;;
    --model|-m) expects_model=1 ;;
    --model=*) configured_model="${argument#--model=}" ;;
  esac
done
[ -z "$configured_model" ] || tmux set-option -p -t "$TMUX_PANE" @devws_model_configured "$configured_model"

# Link the agent with the editor: an edit hook shows its changes there, and
# the prompt tells it how to open files in it. Passed on the command line (the
# module's launch), so the user's own agent settings are never changed.
program="$(agent_program "$kind")"
if ! command -v "$program" >/dev/null 2>&1; then
  tmux set-option -p -t "$TMUX_PANE" @devws_startup_error '{"type":"error","error":{"code":"provider_unavailable"}}'
  printf 'Agent program is unavailable: %s\n' "$program" >&2
  exit 127
fi
# A reviewed leader on a program whose devws agents may run anything
# (opencode): devAgentics starts it in its own isolated configuration, with
# the module's folder for the plugin that reports the pane's state.
if [ "${extra_args[0]:-}" = --devagentics-leader ]; then
  export DEVWS_AGENT_MODULE
  DEVWS_AGENT_MODULE="$(agent_module_dir "$kind")"
  exec "${extra_args[1]}" "${extra_args[@]:2}"
fi
agent_hook "$kind" launch "$resume" "$link_dir/agent_editor_prompt.md" ${extra_args[@]+"${extra_args[@]}"}

provider_exit=$?
if [ "$provider_exit" -ne 0 ]; then
  # A generic exit is actionable evidence, never a quota/model guess. The
  # transcript may supply a more specific typed rejection to the dispatcher.
  tmux set-option -p -t "$TMUX_PANE" @devws_startup_error '{"type":"error","error":{"code":"provider_exit"}}'
fi
exit "$provider_exit"
