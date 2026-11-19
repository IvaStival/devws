#!/usr/bin/env bash
# Antigravity's hooks in devws: the devws plugin (plugin.sh) runs
# this for each of its events. agy reads hooks only from its config folder,
# so the plugin is there for every agy session: outside a devws pane (no
# DEVWS_AGENT, which agent_runner.sh exports) it answers what changes nothing.
# In a devws pane it reports to the same scripts Claude Code's and Codex's
# hooks do:
#
#   SessionStart    idle; the editor prompt is injected as a system message
#   PreInvocation   working (the model is about to answer)
#   PreToolUse      the project is snapshot for the follow hook
#   PostToolUse     working; the file it changed or read is shown
#   Stop            done
#
#   hook.sh <event>   (hook JSON on stdin, agy's camelCase keys)
#
# agy reads the answer on stdout as JSON, and a PreToolUse answer is a
# decision: "ask" leaves it to agy's permission mode and rules. A hook's
# "allow" does not spare a prompt, so what devAgentics' agents may do without
# asking is in their session's settings (agent_args.rs). It always exits 0.

set -u

event="${1:-}"
payload="$(cat)"
scripts="$(cd "$(dirname "$0")/../.." && pwd)"

answer() {
  case "$event" in
    PreToolUse) printf '{"decision":"ask"}\n' ;;
    *) printf '{}\n' ;;
  esac
}

if [ -z "${DEVWS_AGENT:-}" ] || [ -z "${TMUX_PANE:-}" ]; then
  answer
  exit 0
fi

# What Claude Code would have sent: agent_state.sh and agent_follow_hook.sh
# read its keys.
report() {
  printf '%s' "$payload" | jq -c --arg event "$1" '{hook_event_name: $event,
    transcript_path: (.transcriptPath // ""), model: (.modelName // ""), cwd: (.cwd // "")}' 2>/dev/null |
    "$scripts/agent_state.sh" >/dev/null 2>&1
}

case "$event" in
  SessionStart)
    report SessionStart
    prompt_file="${DEVWS_AGY_PROMPT_FILE:-$scripts/agent_editor_prompt.md}"
    if [ -r "$prompt_file" ]; then
      jq -n --rawfile prompt "$prompt_file" '{injectSteps: [{systemMessage: {systemMessage: $prompt}}]}'
    else
      answer
    fi
    ;;
  PreInvocation)
    report UserPromptSubmit
    answer
    ;;
  PreToolUse)
    printf '%s' "$payload" | jq -c '{cwd: (.cwd // "")}' 2>/dev/null |
      "$scripts/agent_follow_hook.sh" antigravity pre >/dev/null 2>&1
    answer
    ;;
  PostToolUse)
    report PostToolUse
    printf '%s' "$payload" | "$scripts/agent_follow_hook.sh" antigravity >/dev/null 2>&1
    answer
    ;;
  Stop)
    report Stop
    answer
    ;;
  *) answer ;;
esac
exit 0
