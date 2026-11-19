#!/usr/bin/env bash
# The agents' usage, for the header's agent column and tmux's own line.
#
#   header_tokens.sh costs                  this month's cost per agent devws
#                                           runs (agent_names): "work 212.43 0"
#                                           (US$, then 1 if a model had no price);
#                                           an agent on a plan without API
#                                           prices has its tokens after them:
#                                           "gemini 0 0 1250000"
#   header_tokens.sh limits <agent>         the plan limits its program logs,
#                                           % used: "<5-hour session> <week>"
#   header_tokens.sh session <transcript>   one conversation: "conv 230k · ctx 41%"
#   header_tokens.sh model <transcript>     latest observed model identifier
#   header_tokens.sh usage <transcript>     what one conversation used:
#                                           "<input> <output> <cache> <US$> <unpriced>"
#                                           (tokens; cache is read + written)
#   header_tokens.sh messages <transcript>  the same, a line per response:
#                                           "<id> <YYYY-MM-DD> <input> <output>
#                                           <cache> <US$> <unpriced>" (id "-"
#                                           when a program's logs do not repeat)
#
# Read from the agents' own logs, in each agent's folder (agents.sh). Each
# program's module knows its logs (agents/*/agent.sh: owns_transcript,
# session, usage, messages, model, month, log_limits); these hooks run inside
# this script and use what it defines below: human, epoch and price (jq),
# recent_logs, token_records, prices_file and now. Costs price each response
# at agent_prices.tsv's API rates; the month starts on the 1st, 00:00 local.
# DEVWS_TOKENS_NOW (epoch seconds) replaces the current time, for tests.

set -u

mode="${1:-}"
now="${DEVWS_TOKENS_NOW:-$(date +%s)}"
scripts_dir="$(cd "$(dirname "$0")" && pwd)"
prices_file="$scripts_dir/agent_prices.tsv"
# shellcheck source=agents.sh
. "$scripts_dir/agents.sh"

# jq's number formatting: 980, 12k, 1.2M.
human='def human: if . < 1000 then tostring
  elif . < 1000000 then "\(. / 1000 | floor)k"
  else "\(. / 100000 | floor | . / 10 | tostring | if test("\\.") then . else . + ".0" end)M" end;'
# Timestamps are UTC ISO 8601 with milliseconds, which fromdateiso8601 rejects.
epoch='def epoch: sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601;'
# The price row of a model ($prices, from agent_prices.tsv): exact, else the
# longest listed prefix, else null.
price='def price($model):
  ($prices | split("\n") | map(select(length > 0 and (startswith("#") | not)) | split("\t"))
    | map({model: .[0], rates: (.[1:] | map(tonumber))})) as $rows
  | ($rows | map(select(.model == $model)) | first)
    // ($rows | map(select(. as $row | $model | startswith($row.model))) | sort_by(.model | length) | last)
  | if . then .rates else null end;'

# <folder> <days> [<name>]: the logs (*.jsonl, or named <name>) of the last
# <days> days, NUL-separated.
recent_logs() {
  [ -d "$1" ] && find "$1" -name "${3:-*.jsonl}" -mtime "-$2" -print0
}

# Cache normalized records rather than totals: reset times, pricing and
# message deduplication still use the current query's rules.
token_records() {
  perl "$scripts_dir/token_cache.pl"
}

costs() {
  local month agent kind home cost
  # The 1st of this month at midnight: BSD date (macOS), else GNU's (Linux).
  month="$(date -r "$now" -v1d -v0H -v0M -v0S +%s 2>/dev/null ||
    date -d "$(date -d "@$now" +%Y-%m-01)" +%s)"
  # The list on fd 3: the loop's commands read their own stdin.
  while IFS= read -r agent <&3; do
    kind="$(agent_kind "$agent")" || continue
    home="$(agent_home "$agent")"
    cost="$(agent_hook "$kind" month "$month" "$home")"
    printf '%s %s\n' "$agent" "${cost:-0 0}"
  done 3< <(agent_names)
}

# <hook> <transcript> [args…]: the module whose logs the transcript is
# answers; nothing when none claims it.
transcript_hook() {
  local hook="$1" transcript="$2" records="" kind
  shift 2
  [ -r "$transcript" ] && records="$(printf '%s\0' "$transcript" | token_records)"
  while IFS= read -r kind; do
    if agent_hook "$kind" owns_transcript "$transcript" "$records"; then
      agent_hook "$kind" "$hook" "$transcript" "$records" "$@"
      return 0
    fi
  done < <(agent_kinds)
  return 0
}

case "$mode" in
  costs) costs ;;
  model) transcript_hook model "${2:-}" ;;
  model-info) transcript_hook model "${2:-}" timed ;;
  usage) transcript_hook usage "${2:-}" ;;
  messages) transcript_hook messages "${2:-}" ;;
  session) transcript_hook session "${2:-}" ;;
  limits)
    kind="$(agent_kind "${2:-}")" || { printf 'Not a registered agent: %s\n' "${2:-}" >&2; exit 2; }
    if ! agent_defines "$kind" log_limits; then
      printf '%s logs no limits: its module reads them elsewhere (agent_statusline.sh)\n' "$kind" >&2
      exit 2
    fi
    agent_hook "$kind" log_limits "$(agent_home "$2")"
    ;;
  *)
    printf 'Usage: header_tokens.sh costs | limits <agent> | session <transcript> | usage <transcript> | model <transcript>\n' >&2
    exit 2
    ;;
esac
