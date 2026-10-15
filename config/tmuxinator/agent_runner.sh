#!/usr/bin/env bash
# Marks the pane as the workspace agent pane, then replaces itself with the
# configured agent process. The pane marker makes restart/switch actions safe.

set -u

agent="${1:-}"
case "$agent" in
  claude|codex) ;;
  *)
    printf 'Unsupported devws agent: %s\n' "$agent" >&2
    exit 2
    ;;
esac

tmux set-option -p -t "$TMUX_PANE" @devws_role agent

# Link the agent with the editor: an edit hook shows its changes there, and
# the prompt tells it how to open files in it. Passed on the command line, so
# the user's own agent settings are never changed.
# Absolute: the hook runs from wherever the agent works.
link_dir="$(cd "$(dirname "$0")" && pwd)"
prompt="$(cat "$link_dir/agent_editor_prompt.md")"
case "$agent" in
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
