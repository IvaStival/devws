#!/usr/bin/env bash
# The header's agent column: a title row, then a row for each agent devws runs
# (agents.sh's agent_names: those in DEVWS_AGENTS, then each installed program
# none of them runs) with how much of its plan's limits it has used — the
# 5-hour session and the week — and what this month's tokens would cost on the
# API. The title row heads the bars and has the month's total:
#
#          5h               wk               month $588
#   claude ▓▓▓▓░░░░░░  42%  ▓▓▓▓▓▓░░░░  63%  $552
#   work   ░░░░░░░░░░   2%  ░░░░░░░░░░   1%  $36
#
# The header has three rows, so two agents show at a time: with more, the
# mouse wheel over the column scrolls them (tmux.conf's WheelUpStatus and
# WheelDownStatus on the devws-agents range), and the title says which show:
# "↕ 2–3/5". The first one shown is the tmux option @devws_agents_offset.
#
#   header_agents.sh [costs file]        the column
#   header_agents.sh --scroll <up|down>  one agent back or on; the column is
#                                        drawn again at once, in the header
#                                        ticker's state folder
#
# The limits come from each program's module (agent_limits): a status line's
# reading (@devws_limits_<agent>) or the program's logs. A program on a plan
# without API prices (Antigravity) shows its month in tokens, not in the
# total. A limit not known yet (an agent started before devws recorded it, or
# a plan without limits) shows an empty bar and "—". An agent whose folder is
# gone has no row; with no agent to show, the column says where to register
# one. The costs file holds header_tokens.sh costs' output, which is slower to
# work out; without it the costs are left out.

set -u

scripts_dir="$(cd "$(dirname "$0")" && pwd)"
costs_file="${1:-}"
# Where the header ticker keeps the column (header_ticker.sh).
state_dir="${TMPDIR:-/tmp}"
state_dir="${state_dir%/}/devws-header-$(id -u)"
# Agents shown at a time, under the title row.
shown=2
# shellcheck source=agents.sh
. "$scripts_dir/agents.sh"

# US$ with cents under 10, whole above; "+" when a model had no price.
dollars() {
  local amount="$1" unpriced="${2:-0}" sign=""
  [ "$unpriced" = 1 ] && sign="+"
  awk -v amount="$amount" -v sign="$sign" 'BEGIN {
    if (amount < 10) printf "$%.2f%s", amount, sign; else printf "$%.0f%s", amount, sign }'
}


# <% used or "-">: its bar and per cent, as wide as the title's heading.
limit() {
  local used="$1"
  if [ -z "$used" ] || [ "$used" = - ]; then
    printf '#[dim]░░░░░░░░░░    —#[nodim]'
    return
  fi
  [ "$used" -gt 100 ] && used=100
  printf '%s %3s%%' "$("$scripts_dir/header_bar.sh" "$used")" "$used"
}

cost_of() {
  [ -r "$costs_file" ] && awk -v agent="$1" '$1 == agent { print $2, $3, $4 }' "$costs_file"
}

# 980, 12k, 1.2M.
tokens() {
  awk -v count="$1" 'BEGIN {
    if (count < 1000) printf "%d tok", count
    else if (count < 1000000) printf "%dk tok", count / 1000
    else printf "%.1fM tok", int(count / 100000) / 10 }'
}

# Run from the wheel, by tmux: the registry and colors the ticker gives it.
if [ "${1:-}" = --scroll ]; then
  set -a
  # shellcheck source=/dev/null
  [ -r "${DEVWS_ENV_FILE:-$scripts_dir/.env}" ] && . "${DEVWS_ENV_FILE:-$scripts_dir/.env}"
  set +a
  export DEVWS_AGENTS="${DEVWS_AGENTS-claude codex}"
  DEVWS_COLOR_OK="$(tmux show-option -gqv @devws_color_ok 2>/dev/null)"
  DEVWS_COLOR_WARN="$(tmux show-option -gqv @devws_color_warn 2>/dev/null)"
  DEVWS_COLOR_STRESS="$(tmux show-option -gqv @devws_color_stress 2>/dev/null)"
  DEVWS_COLOR_DIM="$(tmux show-option -gqv @devws_color_dim 2>/dev/null)"
  export DEVWS_COLOR_OK DEVWS_COLOR_WARN DEVWS_COLOR_STRESS DEVWS_COLOR_DIM
fi

# Only an unset list means the default agents: an empty one has none.
agents=()
while IFS= read -r agent; do
  agent_kind "$agent" >/dev/null && [ -d "$(agent_home "$agent")" ] && agents+=("$agent")
done < <(agent_names)

# The first agent shown, kept within the list.
offset="$(tmux show-option -gqv @devws_agents_offset 2>/dev/null)"
[[ "$offset" =~ ^[0-9]+$ ]] || offset=0
last=$((${#agents[@]} - shown))
[ "$last" -ge 0 ] || last=0

if [ "${1:-}" = --scroll ]; then
  case "${2:-}" in
    up) offset=$((offset - 1)) ;;
    down) offset=$((offset + 1)) ;;
    *) printf 'Usage: header_agents.sh --scroll <up|down>\n' >&2; exit 2 ;;
  esac
  [ "$offset" -ge 0 ] || offset=0
  [ "$offset" -le "$last" ] || offset="$last"
  tmux set-option -g @devws_agents_offset "$offset" 2>/dev/null
  # Drawn again now: the ticker draws it only once a minute.
  mkdir -p "$state_dir" &&
    "$0" "$state_dir/costs" > "$state_dir/agents.new" && mv "$state_dir/agents.new" "$state_dir/agents"
  exit 0
fi
[ "$offset" -le "$last" ] || offset="$last"

if [ "${#agents[@]}" -eq 0 ]; then
  printf '#[dim]%s#[nodim]\n' 'no agents registered' '☰ Menu › Agent › Register'
  exit 0
fi
# The names in one column, as wide as the longest: the same while scrolling.
width=0
for agent in ${agents[@]+"${agents[@]}"}; do
  [ "${#agent}" -le "$width" ] || width="${#agent}"
done

# The month's total covers every agent, shown or not.
total=0
total_unpriced=0
for agent in ${agents[@]+"${agents[@]}"}; do
  read -r cost unpriced used_tokens <<< "$(cost_of "$agent")"
  [ -z "${used_tokens:-}" ] && [ -n "${cost:-}" ] || continue
  total="$(awk -v a="$total" -v b="$cost" 'BEGIN { print a + b }')"
  [ "$unpriced" = 1 ] && total_unpriced=1
done

title="$(printf '%-*s %-15s  %-15s' "$width" "" 5h wk)"
[ -r "$costs_file" ] && title+="  month $(dollars "$total" "$total_unpriced")"
if [ "${#agents[@]}" -gt "$shown" ]; then
  title+="  ↕ $((offset + 1))–$((offset + shown))/${#agents[@]}"
fi
title="${title%"${title##*[! ]}"}"
lines=("#[dim]$title#[nodim]")
for agent in "${agents[@]:offset:shown}"; do
  read -r session_used week_used _ <<< "$(agent_limits "$agent")"
  read -r cost unpriced used_tokens <<< "$(cost_of "$agent")"
  spent=""
  if [ -n "${used_tokens:-}" ]; then
    spent="$(tokens "$used_tokens")"
  elif [ -n "${cost:-}" ]; then
    spent="$(dollars "$cost" "$unpriced")"
  fi
  lines+=("$(printf '#[bold]%-*s#[nobold] %s  %s' "$width" "$agent" "$(limit "${session_used:-}")" "$(limit "${week_used:-}")")${spent:+  $spent}")
done

# With ai-memory installed, the last row ends with its service's state, a
# green dot while it answers, red when it does not — sessions are then not
# being recorded (memory.sh).
memory="$(dirname "$0")/memory.sh"
if [ -x "$memory" ] && "$memory" available; then
  if "$memory" up; then
    lines[${#lines[@]} - 1]+=" #[fg=${DEVWS_COLOR_OK:-green}]●#[fg=default]"
  else
    lines[${#lines[@]} - 1]+=" #[fg=${DEVWS_COLOR_STRESS:-red}]●#[fg=default]"
  fi
fi
# The wheel over these rows scrolls them (the devws-agents range); the rest
# of the header keeps its own (devws-header: a click hides it).
for line in "${lines[@]}"; do
  printf '#[range=user|devws-agents]%s#[norange]#[range=user|devws-header]\n' "$line"
done
