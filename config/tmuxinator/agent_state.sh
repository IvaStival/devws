#!/usr/bin/env bash
# The agents' state hook: turns each hook event into the agent's state —
# working, waiting (a permission prompt or question needs the user), done or
# idle — and records it on the workspace (agent_status.sh), which shows it in
# the agent's header, the status bar and the sidebar.
#
#   agent_state.sh   (hook JSON on stdin; Claude Code and Codex send the same
#                    event names)
#
# It always exits 0 and prints nothing: a hook that fails or prints would
# interrupt the agent.

set -u

payload="$(cat)"

json() {
  printf '%s' "$payload" | jq -r "$1" 2>/dev/null
}

[ -n "${TMUX_PANE:-}" ] || exit 0
session="$(tmux display-message -p -t "$TMUX_PANE" '#{session_name}' 2>/dev/null)" || exit 0
[ -n "$session" ] || exit 0

event="$(json '.hook_event_name // empty')"
state=""
case "$event" in
  UserPromptSubmit|PreToolUse|PostToolUse) state=working ;;
  # Codex asks for approval with its own event.
  PermissionRequest) state=waiting ;;
  # Claude Code only notifies; an idle reminder changes nothing.
  Notification)
    case "$(json '.notification_type // empty')" in
      permission_prompt|elicitation_dialog) state=waiting ;;
    esac
    ;;
  Stop) state=done ;;
  SessionStart|SessionEnd) state=idle ;;
esac

# The state is this pane's: a workspace can run several agents.
[ -z "$state" ] || "$(dirname "$0")/agent_status.sh" "$session" "$state" "$TMUX_PANE" >/dev/null 2>&1
# The conversation's log, whose tokens tmux's own line shows
# (header_tokens.sh): the main agent's, not an extra one's.
transcript="$(json '.transcript_path // empty')"
role="$(tmux show-options -pqv -t "$TMUX_PANE" @devws_role 2>/dev/null)"
[ -z "$transcript" ] || [ "$role" = extra-agent ] ||
  tmux set-option -t "=$session:" @devws_agent_transcript "$transcript" 2>/dev/null
exit 0
