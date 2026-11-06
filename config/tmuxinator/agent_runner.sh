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

# The account: the agent's own folder, or none for its kind's default folder,
# whatever folder the environment came with.
unset CLAUDE_CONFIG_DIR CODEX_HOME
if ! agent_is_default_home "$agent"; then
  case "$kind" in
    claude) export CLAUDE_CONFIG_DIR="$(agent_home "$agent")" ;;
    codex) export CODEX_HOME="$(agent_home "$agent")" ;;
  esac
fi
# The status line records this agent's limits under its name.
export DEVWS_AGENT="$agent"

if [ "$extra" = --extra ]; then
  tmux set-option -p -t "$TMUX_PANE" @devws_role extra-agent
  tmux set-option -p -t "$TMUX_PANE" @devws_agent "$agent"
else
  tmux set-option -p -t "$TMUX_PANE" @devws_role agent
fi

# Link the agent with the editor: an edit hook shows its changes there, and
# the prompt tells it how to open files in it. Passed on the command line, so
# the user's own agent settings are never changed.
# Absolute: the hook runs from wherever the agent works.
prompt="$(cat "$link_dir/agent_editor_prompt.md")"
case "$kind" in
  claude)
    claude ${resume:+--continue} --settings "$link_dir/agent_settings.json" --append-system-prompt "$prompt" \
      ${extra_args[@]+"${extra_args[@]}"}
    ;;
  codex)
    # Config overrides are TOML; jq's JSON strings are valid TOML strings, quotes escaped.
    # The follow hook in <phase> (agent_follow_hook.sh): every tool call, since
    # Codex's tools vary (exec, shell, apply_patch) and any may change files.
    follow_hook() {
      printf '{hooks=[{type="command",command=%s}]}' \
        "$(jq -n --arg path "$link_dir/agent_follow_hook.sh" --arg phase "${1:-}" \
          '"\"\($path)\" codex" + (if $phase == "" then "" else " " + $phase end)')"
    }
    # Every state event reports to agent_state.sh (working / needs you / done).
    state_hook="{hooks=[{type=\"command\",command=$(jq -n --arg path "$link_dir/agent_state.sh" '"\"\($path)\""')}]}"
    state_hooks=()
    for event in SessionStart PermissionRequest Stop SessionEnd; do
      state_hooks+=(-c "hooks.$event=[$state_hook]")
    done
    resume_args=()
    [ -n "$resume" ] && resume_args=(resume --last)
    codex ${resume_args[@]+"${resume_args[@]}"} \
      -c "hooks.UserPromptSubmit=[$state_hook,$(follow_hook prompt)]" \
      -c "hooks.PreToolUse=[$(follow_hook pre),$state_hook]" \
      -c "hooks.PostToolUse=[$(follow_hook),$state_hook]" \
      "${state_hooks[@]}" \
      -c "developer_instructions=$(jq -n --arg prompt "$prompt" '$prompt')" \
      ${extra_args[@]+"${extra_args[@]}"}
    ;;
esac
