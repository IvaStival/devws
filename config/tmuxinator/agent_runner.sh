#!/usr/bin/env bash
# Marks the pane as the workspace agent pane, then replaces itself with the
# configured agent process. The pane marker makes restart/switch actions safe.
#
#   agent_runner.sh <agent>           a name registered in .env (agents.sh)
#   agent_runner.sh <agent> --extra   an extra agent (New panel): marked
#                                     extra-agent with its agent's name, so the
#                                     main agent keeps its role

set -u

agent="${1:-}"
extra="${2:-}"
link_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
[ -r "$link_dir/.env" ] && . "$link_dir/.env"
# shellcheck source=agents.sh
. "$link_dir/agents.sh"

if ! kind="$(agent_kind "$agent")"; then
  printf 'Unsupported devws agent: %s (register it in the devws menu)\n' "$agent" >&2
  exit 2
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
    exec claude --settings "$link_dir/agent_settings.json" --append-system-prompt "$prompt"
    ;;
  codex)
    # Config overrides are TOML; jq's JSON strings are valid TOML strings, quotes escaped.
    hook_command="$(jq -n --arg path "$link_dir/agent_follow_hook.sh" '"\"\($path)\" codex"')"
    # Every state event reports to agent_state.sh (working / needs you / done).
    state_hook="{hooks=[{type=\"command\",command=$(jq -n --arg path "$link_dir/agent_state.sh" '"\"\($path)\""')}]}"
    state_hooks=()
    for event in SessionStart UserPromptSubmit PreToolUse PermissionRequest Stop SessionEnd; do
      state_hooks+=(-c "hooks.$event=[$state_hook]")
    done
    exec codex \
      -c "hooks.PostToolUse=[{matcher=\"apply_patch\",hooks=[{type=\"command\",command=$hook_command}]},$state_hook]" \
      "${state_hooks[@]}" \
      -c "developer_instructions=$(jq -n --arg prompt "$prompt" '$prompt')"
    ;;
esac
