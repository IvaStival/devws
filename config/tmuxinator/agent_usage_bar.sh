#!/usr/bin/env bash
# devws's default Claude Code status line (agent_statusline.sh, when the
# account has none of its own): the model, the branch, and how much of the
# plan's 5-hour session, its week and the context window is used, each as a
# bar — green under 60%, yellow under 85%, red from there. A limit the plan
# does not have is left out.
#
#   agent_usage_bar.sh   (status line JSON on stdin)

set -u

payload="$(cat)"

json() {
  printf '%s' "$payload" | jq -r "$1" 2>/dev/null
}

reset=$'\033[0m'
dim=$'\033[2m'

#   build_bar <label> <percent used>
build_bar() {
  local label="$1" percent="$2" width=10 filled color bar=""
  filled=$((percent * width / 100))
  [ "$filled" -le "$width" ] || filled=$width
  [ "$filled" -ge 0 ] || filled=0
  for ((cell = 0; cell < width; cell++)); do
    if [ "$cell" -lt "$filled" ]; then bar+="▓"; else bar+="░"; fi
  done
  if [ "$percent" -lt 60 ]; then
    color=$'\033[32m'
  elif [ "$percent" -lt 85 ]; then
    color=$'\033[33m'
  else
    color=$'\033[31m'
  fi
  printf '  %s%s%s %s%s %s%%%s' "$dim" "$label" "$reset" "$color" "$bar" "$percent" "$reset"
}

model="$(json '.model.display_name // "?"')"
session_used="$(json '.rate_limits.five_hour.used_percentage // empty | floor')"
week_used="$(json '.rate_limits.seven_day.used_percentage // empty | floor')"
context_used="$(json '.context_window.used_percentage // empty | floor')"
folder="$(json '.workspace.current_dir // .cwd // empty')"
branch="$(git -C "${folder:-.}" branch --show-current 2>/dev/null)"

line="$dim[$model]$reset"
[ -z "$branch" ] || line+=" $dim$branch$reset"
[ -z "$session_used" ] || line+="$(build_bar 5h "$session_used")"
[ -z "$week_used" ] || line+="$(build_bar 7d "$week_used")"
[ -z "$context_used" ] || line+="$(build_bar ctx "$context_used")"
printf '%s' "$line"
