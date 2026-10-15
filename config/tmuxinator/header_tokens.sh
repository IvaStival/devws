#!/usr/bin/env bash
# Token usage of the agents, for the header's AGENTS group.
#
#   header_tokens.sh totals                 a line per agent in DEVWS_AGENTS:
#                                           "claude 1.2M 8.4M" (today, week)
#   header_tokens.sh limits codex           Codex's plan limits, % used:
#                                           "<5-hour session> <week>"
#   header_tokens.sh session <transcript>   one conversation: "conv 230k · ctx 41%"
#
# Read from the agents' own logs: Claude Code's ~/.claude/projects/*/*.jsonl
# and Codex's ~/.codex/sessions/YYYY/MM/DD/*.jsonl. A total counts input,
# output and cache tokens, as Codex's own total_tokens does. "Today" starts at
# local midnight and the week on Monday 00:00 local. DEVWS_TOKENS_NOW (epoch
# seconds) replaces the current time, for tests.

set -u

mode="${1:-}"
now="${DEVWS_TOKENS_NOW:-$(date +%s)}"

# jq's number formatting, shared by every output: 980, 12k, 1.2M.
human='def human: if . < 1000 then tostring
  elif . < 1000000 then "\(. / 1000 | floor)k"
  else "\(. / 100000 | floor | . / 10 | tostring | if test("\\.") then . else . + ".0" end)M" end;'
# Timestamps are UTC ISO 8601 with milliseconds, which fromdateiso8601 rejects.
epoch='def epoch: sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601;'

# Claude Code writes a message's usage on several lines: one count per id.
claude_messages='[inputs | fromjson? | select(.type == "assistant" and .message.usage and .message.id)]
  | map({id: .message.id, at: (.timestamp | epoch), usage: .message.usage,
         total: (.message.usage | (.input_tokens + .output_tokens
           + (.cache_creation_input_tokens // 0) + (.cache_read_input_tokens // 0)))})
  | unique_by(.id)'

# Codex logs the conversation's running total: each event adds the difference.
codex_events='[inputs as $line | input_filename as $file | $line | fromjson?
    | select(.type == "event_msg" and .payload.type == "token_count" and .payload.info)
    | {file: $file, at: (.timestamp | epoch), total: .payload.info.total_token_usage.total_tokens}]
  | reduce .[] as $event ({previous: {}, events: []};
      .events += [{at: $event.at, used: ($event.total - (.previous[$event.file] // 0))}]
      | .previous[$event.file] = $event.total)
  | .events | map(select(.used > 0))'

# The logs of the last 8 days, which cover the week; the rest never counts.
recent_logs() {
  [ -d "$1" ] && find "$1" -name '*.jsonl' -mtime -8 -print0
}

claude_usage() {
  recent_logs "$HOME/.claude/projects" | xargs -0 grep -h '"output_tokens"' 2>/dev/null |
    jq -nRr --argjson today "$1" --argjson week "$2" "$human $epoch $claude_messages
      | \"\(map(select(.at >= \$today) | .total) | add // 0 | human) \(map(select(.at >= \$week) | .total) | add // 0 | human)\""
}

codex_usage() {
  local logs=()
  while IFS= read -r -d '' log; do
    logs+=("$log")
  done < <(recent_logs "$HOME/.codex/sessions")
  # Every log is read whole, so jq knows which conversation each event is from.
  if [ "${#logs[@]}" -eq 0 ]; then
    printf '0 0\n'
    return
  fi
  jq -nRr --argjson today "$1" --argjson week "$2" "$human $epoch $codex_events
    | \"\(map(select(.at >= \$today) | .used) | add // 0 | human) \(map(select(.at >= \$week) | .used) | add // 0 | human)\"" \
    "${logs[@]}" 2>/dev/null
}

totals() {
  local today week agent usage today_used week_used
  today="$(date -r "$now" -v0H -v0M -v0S +%s)"
  week="$(date -r "$now" -v-mon -v0H -v0M -v0S +%s)"
  for agent in ${DEVWS_AGENTS:-claude codex}; do
    case "$agent" in
      claude) usage="$(claude_usage "$today" "$week")" ;;
      codex) usage="$(codex_usage "$today" "$week")" ;;
      *) continue ;;
    esac
    read -r today_used week_used <<< "${usage:-0 0}"
    printf '%s %s %s\n' "$agent" "${today_used:-0}" "${week_used:-0}"
  done
}

# Codex logs its plan limits with each token count: the latest one counts, and
# a window that has reset since is back to 0. Claude Code's limits come from
# its status line instead (agent_statusline.sh).
codex_limits() {
  local logs=()
  while IFS= read -r -d '' log; do
    logs+=("$log")
  done < <(recent_logs "$HOME/.codex/sessions")
  [ "${#logs[@]}" -gt 0 ] || return 0
  grep -h '"rate_limits"' "${logs[@]}" 2>/dev/null |
    jq -nRr --argjson now "$now" "$epoch [inputs | fromjson? | select(.payload.rate_limits.primary)
      | {at: (.timestamp | epoch), limits: .payload.rate_limits}] | sort_by(.at) | last // empty | .limits
      | [.primary, .secondary] | map(if . == null then \"-\" elif (.resets_at // 1e18) < \$now then 0 else .used_percent | floor end)
      | join(\" \")"
}

# Codex knows its context window; Claude Code's logs do not, so its context is
# shown in tokens.
session() {
  local transcript="$1"
  [ -r "$transcript" ] || return 0
  if grep -q '"token_count"' "$transcript"; then
    jq -nRr "$human [inputs | fromjson? | select(.type == \"event_msg\" and .payload.type == \"token_count\" and .payload.info) | .payload.info]
      | last // empty
      | \"conv \(.total_token_usage.total_tokens | human) · ctx \(.last_token_usage.input_tokens * 100 / .model_context_window | floor)%\"" \
      "$transcript" 2>/dev/null
  else
    grep -h '"output_tokens"' "$transcript" |
      jq -nRr "$human $epoch $claude_messages | select(length > 0)
        | \"conv \(map(.total) | add | human) · ctx \(sort_by(.at) | last | .usage
          | (.input_tokens + (.cache_creation_input_tokens // 0) + (.cache_read_input_tokens // 0)) | human)\"" 2>/dev/null
  fi
  return 0
}

case "$mode" in
  totals) totals ;;
  session) session "${2:-}" ;;
  limits)
    case "${2:-}" in
      codex) codex_limits ;;
      *)
        printf 'Only Codex logs its limits; Claude Code reports them to its status line\n' >&2
        exit 2
        ;;
    esac
    ;;
  *)
    printf 'Usage: header_tokens.sh totals | limits codex | session <transcript>\n' >&2
    exit 2
    ;;
esac
