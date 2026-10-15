#!/usr/bin/env bash
# The header's agent columns: three rows for each agent in DEVWS_AGENTS, one
# column each, side by side:
#
#   claude  42.4M today · 469M week
#   session ▓▓▓▓▓▓░░░░  62% left
#   week    ▓▓▓▓▓▓▓▓▓░  88% left
#
#   header_agents.sh
#
# "session" and "week" are the plan's limits, as Claude Code's /usage names
# them: Claude Code's from its status line (agent_statusline.sh, in the tmux
# option @devws_claude_limits), Codex's from its logs (header_tokens.sh). A
# limit not known yet (an agent started before devws recorded it, or a plan
# without limits) shows an empty bar and "—".

set -u

scripts_dir="$(cd "$(dirname "$0")" && pwd)"

# <label> <% used or "-">
limit_row() {
  local label="$1" used="$2"
  if [ -z "$used" ] || [ "$used" = - ]; then
    printf '%-7s #[dim]░░░░░░░░░░    —#[nodim]\n' "$label"
    return
  fi
  [ "$used" -gt 100 ] && used=100
  printf '%-7s %s %3s%% left\n' "$label" "$("$scripts_dir/header_bar.sh" $((100 - used)) "$used")" $((100 - used))
}

limits_of() {
  case "$1" in
    claude) tmux show-option -gqv @devws_claude_limits 2>/dev/null ;;
    codex) "$scripts_dir/header_tokens.sh" limits codex ;;
  esac
}

while read -r agent today week; do
  read -r session_used week_used <<< "$(limits_of "$agent")"
  printf '#[bold]%-7s#[nobold] %s today · %s week\n' "$agent" "$today" "$week"
  limit_row session "${session_used:-}"
  limit_row week "${week_used:-}"
done < <("$scripts_dir/header_tokens.sh" totals)
