#!/usr/bin/env bash
# The agents' status line in devws, for a program that runs a status line
# command (Claude Code's settings.json in its module; Antigravity's session settings):
# records the plan's limits — the 5-hour session and the week, as % used — in
# the tmux option @devws_limits_<agent> ("<session> <week> <epoch read>"),
# which the header's agent column shows, then prints the user's own status
# line from the account's settings, unchanged, or devws's usage bar
# (agent_usage_bar.sh) when the account has none. What the payload says is
# the program's module's to read (status_line_limits, status_line_model,
# status_line_extra, settings_file). The agent is the devws name
# agent_runner.sh exports as DEVWS_AGENT, so each account keeps its own
# limits; without one, it is claude.
#
#   agent_statusline.sh   (status line JSON on stdin)

set -u

payload="$(cat)"

scripts_dir="$(cd "$(dirname "$0")" && pwd)"
# The registry, for the agent's program: the runner does not pass it on.
# shellcheck source=/dev/null
[ -r "$scripts_dir/.env" ] && . "$scripts_dir/.env"
# shellcheck source=agents.sh
. "$scripts_dir/agents.sh"
agent="${DEVWS_AGENT:-claude}"
kind="$(agent_kind "$agent" 2>/dev/null)" || kind=claude
home="$(agent_home "$agent" 2>/dev/null)" || home="$(agent_default_home "$kind")"

# The plan's limits, % used, as the program's module reads its payload.
read -r session_used week_used <<< "$(agent_hook "$kind" status_line_limits "$payload")"
if [ -n "${TMUX:-}" ] && [ -n "${session_used:-}${week_used:-}" ]; then
  tmux set-option -g "@devws_limits_$agent" "${session_used:--} ${week_used:--} $(date +%s)" 2>/dev/null
fi
agent_hook "$kind" status_line_extra "$payload"

model="$(agent_hook "$kind" status_line_model "$payload")"
if [ -n "${TMUX_PANE:-}" ] && [ -n "$model" ]; then
  "$scripts_dir/agent_model.sh" record "$TMUX_PANE" "$model" >/dev/null 2>&1
fi

# The user's own status line, from the account's settings, else devws's bar.
settings="$(agent_hook "$kind" settings_file "$home")"
user_command=""
[ -z "$settings" ] || user_command="$(jq -r '.statusLine.command // empty' "$settings" 2>/dev/null)"
[ -n "$user_command" ] || user_command="\"$scripts_dir/agent_usage_bar.sh\""
printf '%s' "$payload" | bash -c "$user_command"
