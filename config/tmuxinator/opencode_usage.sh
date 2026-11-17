#!/usr/bin/env bash
# What opencode agents used, from opencode's own database: each response it
# records carries its tokens and what the provider billed for it (for Zen,
# the real charge, not an estimate at API prices).
#
#   opencode_usage.sh usage <db>#<session>   one conversation: "<input>
#                                            <output> <cache> <US$> 0" (cache
#                                            is read + written)
#   opencode_usage.sh rows <db>              every response: "<folder> <id>
#                                            <YYYY-MM-DD> <input> <output>
#                                            <cache> <US$> 0", tab-separated
#                                            (usage_history.sh)
#   opencode_usage.sh spent <db> <epoch>     what responses since then cost,
#                                            US$ (opencode_credit.sh)
#
# The database is opened read-only, beside the opencode running on it. A
# database that is not there yet (an account never used) has used nothing.

set -u
# Numbers are read and printed with a decimal point, whatever the user's locale.
export LC_ALL=C

fail() {
  printf 'opencode_usage.sh: %s\n' "$*" >&2
  exit 2
}

command -v sqlite3 >/dev/null 2>&1 || fail "sqlite3 is needed to read opencode's usage"

# <db> <sql>: tab-separated rows; nothing for a database not made yet.
query() {
  [ -r "$1" ] || return 0
  sqlite3 -readonly -separator "$(printf '\t')" "$1" "$2"
}

# The responses, as columns: only the assistant's messages are billed.
responses="SELECT id, time_created,
    json_extract(data, '\$.path.cwd') AS folder,
    coalesce(json_extract(data, '\$.tokens.input'), 0) AS input,
    coalesce(json_extract(data, '\$.tokens.output'), 0) + coalesce(json_extract(data, '\$.tokens.reasoning'), 0) AS output,
    coalesce(json_extract(data, '\$.tokens.cache.read'), 0) + coalesce(json_extract(data, '\$.tokens.cache.write'), 0) AS cache,
    coalesce(json_extract(data, '\$.cost'), 0) AS cost,
    session_id
  FROM message WHERE json_extract(data, '\$.role') = 'assistant'"

# A session id is opencode's own (ses_…): letters, digits and _.
usage() {
  local db="${1%#*}" session="${1##*#}"
  [[ "$session" =~ ^[A-Za-z0-9_]+$ ]] || fail "not an opencode conversation: $1"
  query "$db" "SELECT coalesce(sum(input), 0), coalesce(sum(output), 0), coalesce(sum(cache), 0),
      round(coalesce(sum(cost), 0), 2), 0
    FROM ($responses) WHERE session_id = '$session'" | awk -F '\t' '{ $4 += 0; print }'
  [ -r "$db" ] || printf '0 0 0 0 0\n'
}

rows() {
  query "$1" "SELECT coalesce(folder, '-'), id, date(time_created / 1000, 'unixepoch', 'localtime'),
      input, output, cache, round(cost, 6), 0
    FROM ($responses) ORDER BY time_created" | awk -F '\t' -v OFS='\t' '{ $7 += 0; print }'
}

spent() {
  [[ "$2" =~ ^[0-9]+$ ]] || fail "since: epoch seconds, not $2"
  local total
  total="$(query "$1" "SELECT round(coalesce(sum(cost), 0), 2) FROM ($responses) WHERE time_created >= $2 * 1000")"
  awk -v total="${total:-0}" 'BEGIN { printf "%.2f\n", total }'
}

case "${1:-}" in
  usage) [ "$#" -eq 2 ] || fail "usage: opencode_usage.sh usage <db>#<session>"; usage "$2" ;;
  rows) [ "$#" -eq 2 ] || fail "usage: opencode_usage.sh rows <db>"; rows "$2" ;;
  spent) [ "$#" -eq 3 ] || fail "usage: opencode_usage.sh spent <db> <epoch>"; spent "$2" "$3" ;;
  *) fail "usage: opencode_usage.sh usage <db>#<session> | rows <db> | spent <db> <epoch>" ;;
esac
