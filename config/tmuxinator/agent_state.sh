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

[ -n "${TMUX_PANE:-}" ] || exit 0
event="" notification="" transcript="" model=""
# Parse once, preserving spaces, quotes and backslashes in transcript paths.
# NUL separators distinguish empty fields without evaluating payload text.
if ! {
  IFS= read -r -d '' event
  IFS= read -r -d '' notification
  IFS= read -r -d '' transcript
  IFS= read -r -d '' model
} < <(printf '%s' "$payload" | jq -j \
  '[.hook_event_name // "", .notification_type // "", .transcript_path // "", (if (.model | type) == "string" then .model else "" end)][]
   | if type == "string" then . else "" end | ., "\u0000"' 2>/dev/null); then
  exit 0
fi
metadata="$(tmux display-message -p -t "$TMUX_PANE" '#{session_name}|#{@devws_role}' 2>/dev/null)" || exit 0
session="${metadata%|*}"
role="${metadata##*|}"
[ -n "$session" ] || exit 0

state=""
case "$event" in
  UserPromptSubmit|PreToolUse|PostToolUse) state=working ;;
  # Codex asks for approval with its own event.
  PermissionRequest) state=waiting ;;
  # Claude Code only notifies; an idle reminder changes nothing.
  Notification)
    case "$notification" in
      permission_prompt|elicitation_dialog) state=waiting ;;
    esac
    ;;
  Stop) state=done ;;
  SessionStart|SessionEnd) state=idle ;;
esac

# The conversation's log, whose tokens tmux's own line shows
# (header_tokens.sh): the main agent's, not an extra one's.
if [ -n "$transcript" ]; then
  if [ "$role" != extra-agent ] &&
      [ "$(tmux show-option -qv -t "=$session:" @devws_agent_transcript 2>/dev/null)" != "$transcript" ]; then
    tmux set-option -t "=$session:" @devws_agent_transcript "$transcript" 2>/dev/null
  fi
  # Every agent's own, for what it used (devws agent usage <pane>).
  if [ "$(tmux show-options -pqv -t "$TMUX_PANE" @devws_transcript 2>/dev/null)" != "$transcript" ]; then
    tmux set-option -p -t "$TMUX_PANE" @devws_transcript "$transcript" 2>/dev/null
  fi
fi
# Hook models are runtime evidence; recording them never scans transcripts.
[ -z "$model" ] || "$(dirname "$0")/agent_model.sh" record "$TMUX_PANE" "$model" >/dev/null 2>&1
# Publish completion after transcript metadata: dispatchers must inspect provider
# rejection evidence before treating the opening turn as ready.
[ -z "$state" ] || "$(dirname "$0")/agent_status.sh" "$session" "$state" "$TMUX_PANE" >/dev/null 2>&1
exit 0
