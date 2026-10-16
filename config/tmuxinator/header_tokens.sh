#!/usr/bin/env bash
# The agents' usage, for the header's agent column and tmux's own line.
#
#   header_tokens.sh costs                  this month's cost per agent in
#                                           DEVWS_AGENTS: "work 212.43 0"
#                                           (US$, then 1 if a model had no price)
#   header_tokens.sh limits <codex agent>   a Codex agent's plan limits, % used:
#                                           "<5-hour session> <week>"
#   header_tokens.sh session <transcript>   one conversation: "conv 230k · ctx 41%"
#
# Read from the agents' own logs, in each agent's folder (agents.sh): Claude
# Code's <folder>/projects/*/*.jsonl and Codex's
# <folder>/sessions/YYYY/MM/DD/*.jsonl. Costs price each response
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

# Claude Code writes a message's usage on several lines: one per id.
claude_messages='[inputs | fromjson? | select(.type == "assistant" and .message.usage and .message.id)]
  | map({id: .message.id, at: (.timestamp | epoch), model: .message.model, usage: .message.usage,
         total: (.message.usage | (.input_tokens + .output_tokens
           + (.cache_creation_input_tokens // 0) + (.cache_read_input_tokens // 0)))})
  | unique_by(.id)'

# The logs of the last <days> days.
recent_logs() {
  [ -d "$1" ] && find "$1" -name '*.jsonl' -mtime "-$2" -print0
}

# Rates: input, cache write 5 min, cache write 1 h, cache read, output. A
# message without the 5 min / 1 h split counts its writes as 5 min ones.
#   claude_cost <since> <folder>
claude_cost() {
  recent_logs "$2/projects" 32 | xargs -0 grep -h '"output_tokens"' 2>/dev/null |
    jq -nRr --argjson since "$1" --rawfile prices "$prices_file" "$epoch $price $claude_messages
      | map(select(.at >= \$since and .model != \"<synthetic>\") | . as \$message | price(.model) as \$rates
        | if \$rates == null then {cost: 0, unpriced: 1}
          else (.usage.cache_creation // {}) as \$writes
            | ((\$writes.ephemeral_5m_input_tokens // null) == null and (\$writes.ephemeral_1h_input_tokens // null) == null) as \$unsplit
            | {cost: ((.usage.input_tokens * \$rates[0]
                + (if \$unsplit then (.usage.cache_creation_input_tokens // 0) else (\$writes.ephemeral_5m_input_tokens // 0) end) * \$rates[1]
                + (if \$unsplit then 0 else (\$writes.ephemeral_1h_input_tokens // 0) end) * \$rates[2]
                + (.usage.cache_read_input_tokens // 0) * \$rates[3]
                + .usage.output_tokens * \$rates[4]) / 1000000),
               unpriced: 0}
          end)
      | \"\(map(.cost) | add // 0 | . * 100 | round / 100) \(map(.unpriced) | max // 0)\""
}

# Each Codex response logs its usage (token_usage_record) under its turn,
# whose turn_context names the model. Input counts cached and written tokens.
#   codex_cost <since> <folder>
codex_cost() {
  local logs=()
  while IFS= read -r -d '' log; do
    logs+=("$log")
  done < <(recent_logs "$2/sessions" 32)
  if [ "${#logs[@]}" -eq 0 ]; then
    printf '0 0\n'
    return
  fi
  grep -h '"turn_context"\|"token_usage_record"' "${logs[@]}" 2>/dev/null |
    jq -nRr --argjson since "$1" --rawfile prices "$prices_file" "$epoch $price
      [inputs | fromjson?] as \$lines
      | (\$lines | map(select(.type == \"turn_context\") | {key: .payload.turn_id, value: .payload.model}) | from_entries) as \$models
      | \$lines | map(select(.type == \"token_usage_record\" and (.timestamp | epoch) >= \$since)
        | price(\$models[.payload.turn_id] // \"\") as \$rates | .payload.usage as \$usage
        | if \$rates == null then {cost: 0, unpriced: 1}
          else {cost: (((\$usage.input_tokens - (\$usage.cached_input_tokens // 0) - (\$usage.cache_write_input_tokens // 0)) * \$rates[0]
                + (\$usage.cache_write_input_tokens // 0) * \$rates[1]
                + (\$usage.cached_input_tokens // 0) * \$rates[3]
                + \$usage.output_tokens * \$rates[4]) / 1000000),
                unpriced: 0}
          end)
      | \"\(map(.cost) | add // 0 | . * 100 | round / 100) \(map(.unpriced) | max // 0)\""
}

costs() {
  local month agent kind home cost
  month="$(date -r "$now" -v1d -v0H -v0M -v0S +%s)"
  for agent in ${DEVWS_AGENTS-claude codex}; do
    kind="$(agent_kind "$agent")" || continue
    home="$(agent_home "$agent")"
    case "$kind" in
      claude) cost="$(claude_cost "$month" "$home")" ;;
      codex) cost="$(codex_cost "$month" "$home")" ;;
    esac
    printf '%s %s\n' "$agent" "${cost:-0 0}"
  done
}

# Codex logs its plan limits with each token count: the latest one counts, and
# a window that has reset since is back to 0. Claude Code's limits come from
# its status line instead (agent_statusline.sh).
#   codex_limits <folder>
codex_limits() {
  local logs=()
  while IFS= read -r -d '' log; do
    logs+=("$log")
  done < <(recent_logs "$1/sessions" 8)
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
  costs) costs ;;
  session) session "${2:-}" ;;
  limits)
    if [ "$(agent_kind "${2:-}")" != codex ]; then
      printf 'Only Codex logs its limits; Claude Code reports them to its status line\n' >&2
      exit 2
    fi
    codex_limits "$(agent_home "$2")"
    ;;
  *)
    printf 'Usage: header_tokens.sh costs | limits <codex agent> | session <transcript>\n' >&2
    exit 2
    ;;
esac
