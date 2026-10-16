#!/usr/bin/env bash
# Claude Code's status line in devws (agent_settings.json): records the plan's
# limits — the 5-hour session and the week, as % used — in the tmux option
# @devws_limits_<agent> ("<session> <week>"), which the header's agent column
# shows, then prints the user's own status line from the account's
# settings.json (in CLAUDE_CONFIG_DIR, else ~/.claude), unchanged. Claude Code
# passes the limits only to a status line, and only on plans that have them.
# The agent is the devws name agent_runner.sh exports as DEVWS_AGENT, so each
# account keeps its own limits.
#
#   agent_statusline.sh   (status line JSON on stdin)

set -u

payload="$(cat)"

json() {
  printf '%s' "$payload" | jq -r "$1" 2>/dev/null
}

session_used="$(json '.rate_limits.five_hour.used_percentage // empty | floor')"
week_used="$(json '.rate_limits.seven_day.used_percentage // empty | floor')"
if [ -n "${TMUX:-}" ] && [ -n "$session_used$week_used" ]; then
  tmux set-option -g "@devws_limits_${DEVWS_AGENT:-claude}" "${session_used:--} ${week_used:--}" 2>/dev/null
fi

user_command="$(jq -r '.statusLine.command // empty' "${CLAUDE_CONFIG_DIR:-$HOME/.claude}/settings.json" 2>/dev/null)"
[ -n "$user_command" ] || exit 0
printf '%s' "$payload" | bash -c "$user_command"
