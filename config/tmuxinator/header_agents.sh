#!/usr/bin/env bash
# The header's agent column: a row for each agent in DEVWS_AGENTS with how much
# of its plan's limits it has used — the 5-hour session and the week — and
# what this month's tokens would cost on the API, then the month's total:
#
#   claude  5h ▓▓▓▓░░░░░░  42%  wk ▓▓▓▓▓▓░░░░  63%  $552
#   codex   5h ░░░░░░░░░░   2%  wk ░░░░░░░░░░   1%  $36
#   month $588 at API prices
#
#   header_agents.sh [costs file]
#
# The limits: Claude Code's from its status line (agent_statusline.sh, in the
# tmux option @devws_claude_limits), Codex's from its logs (header_tokens.sh).
# A limit not known yet (an agent started before devws recorded it, or a plan
# without limits) shows an empty bar and "—". The costs file holds
# header_tokens.sh costs' output, which is slower to work out; without it the
# costs are left out.

set -u

scripts_dir="$(cd "$(dirname "$0")" && pwd)"
costs_file="${1:-}"

# US$ with cents under 10, whole above; "+" when a model had no price.
dollars() {
  local amount="$1" unpriced="${2:-0}" sign=""
  [ "$unpriced" = 1 ] && sign="+"
  awk -v amount="$amount" -v sign="$sign" 'BEGIN {
    if (amount < 10) printf "$%.2f%s", amount, sign; else printf "$%.0f%s", amount, sign }'
}

# <label> <% used or "-">
limit() {
  local label="$1" used="$2"
  if [ -z "$used" ] || [ "$used" = - ]; then
    printf '%s #[dim]░░░░░░░░░░    —#[nodim]' "$label"
    return
  fi
  [ "$used" -gt 100 ] && used=100
  printf '%s %s %3s%%' "$label" "$("$scripts_dir/header_bar.sh" "$used")" "$used"
}

limits_of() {
  case "$1" in
    claude) tmux show-option -gqv @devws_claude_limits 2>/dev/null ;;
    codex) "$scripts_dir/header_tokens.sh" limits codex ;;
  esac
}

cost_of() {
  [ -r "$costs_file" ] && awk -v agent="$1" '$1 == agent { print $2, $3 }' "$costs_file"
}

rows=0
total=0
total_unpriced=0
for agent in ${DEVWS_AGENTS:-claude codex}; do
  case "$agent" in
    claude | codex) ;;
    *) continue ;;
  esac
  read -r session_used week_used <<< "$(limits_of "$agent")"
  row="$(printf '#[bold]%-7s#[nobold] %s  %s' "$agent" "$(limit 5h "${session_used:-}")" "$(limit wk "${week_used:-}")")"
  read -r cost unpriced <<< "$(cost_of "$agent")"
  if [ -n "${cost:-}" ]; then
    row+="  $(dollars "$cost" "$unpriced")"
    total="$(awk -v a="$total" -v b="$cost" 'BEGIN { print a + b }')"
    [ "$unpriced" = 1 ] && total_unpriced=1
  fi
  printf '%s\n' "$row"
  rows=$((rows + 1))
done
if [ "$rows" -lt 3 ] && [ -r "$costs_file" ]; then
  printf '#[dim]month %s at API prices#[nodim]\n' "$(dollars "$total" "$total_unpriced")"
fi
