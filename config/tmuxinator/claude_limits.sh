#!/usr/bin/env bash
# A Claude account's plan limits as Anthropic has them now: "<5-hour session>
# <week>", in per cent used, a "-" for a window the account does not have.
# The status line's reading (agent_statusline.sh) is only as new as the last
# Claude session that drew it, and a reset bought before the window's end is
# never in it, so the agent team asks here before it starts an agent.
#
# It asks the endpoint Claude Code's /usage asks, with the account's own login:
# the macOS keychain entry Claude Code keeps for its config folder, else the
# folder's .credentials.json (Linux). The token goes to curl on stdin, never on
# a command line, and is not printed. Exit 1 when the login or the answer is
# missing; the caller then falls back on the status line's reading.
#
#   claude_limits.sh <agent>

set -u

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=agents.sh
. "$script_dir/agents.sh"

usage_url="${DEVWS_CLAUDE_USAGE_URL:-https://api.anthropic.com/api/oauth/usage}"

# Claude Code names its keychain entry after the CLAUDE_CONFIG_DIR it runs
# with: the plain name without one, else the name and the first 8 hex digits
# of the folder's SHA-256. agent_runner.sh sets the folder as agent_home gives
# it, so that is the text hashed here.
keychain_service() {
  local agent="$1" digest
  if ! agent_has_folder "$agent"; then
    printf 'Claude Code-credentials'
    return
  fi
  digest="$(printf '%s' "$(agent_home "$agent")" | shasum -a 256)" || return 1
  printf 'Claude Code-credentials-%s' "${digest:0:8}"
}

credentials() {
  local agent="$1" service file
  if command -v security >/dev/null 2>&1; then
    service="$(keychain_service "$agent")" || return 1
    security find-generic-password -s "$service" -w 2>/dev/null && return
  fi
  file="$(agent_home "$agent")/.credentials.json"
  [ -r "$file" ] && cat "$file"
}

main() {
  local agent="${1:-}" token answer
  [ "$#" -eq 1 ] || { echo "usage: claude_limits.sh <agent>" >&2; return 2; }
  [ "$(agent_kind "$agent" 2>/dev/null)" = claude ] || { echo "not a Claude agent: $agent" >&2; return 1; }

  token="$(credentials "$agent" | jq -r '.claudeAiOauth.accessToken // empty' 2>/dev/null)"
  [ -n "$token" ] || return 1
  answer="$(printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --silent --fail --max-time 5 --config - \
      --header 'anthropic-beta: oauth-2025-04-20' "$usage_url" 2>/dev/null)" || return 1
  printf '%s' "$answer" | jq -er '
    def used: if type == "object" and (.utilization | type) == "number"
      then .utilization | floor | tostring else "-" end;
    select(type == "object" and (has("five_hour") or has("seven_day")))
    | "\(.five_hour | used) \(.seven_day | used)"' 2>/dev/null
}

main "$@"
