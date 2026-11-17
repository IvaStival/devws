#!/usr/bin/env bash
# An opencode Zen account's credit. Zen has no API that tells the balance, so
# the user enters it (the devws menu's OpenCode screen, after a top-up or a
# look at opencode.ai) and what the account's responses cost since counts
# against it.
#
#   opencode_credit.sh set <agent> <US$>   the balance now
#   opencode_credit.sh show <agent>        "<balance> <used since>", US$;
#                                          nothing when no balance was entered
#
# Kept in ~/.devws-state/opencode-credit.tsv: "<agent> <balance> <epoch>".
# DEVWS_TOKENS_NOW (epoch seconds) replaces the current time, for tests.

set -u
export LC_ALL=C

scripts_dir="$(cd "$(dirname "$0")" && pwd)"
env_file="${DEVWS_ENV_FILE:-$scripts_dir/.env}"
# shellcheck source=/dev/null
[ -r "$env_file" ] && . "$env_file"
# shellcheck source=agents.sh
. "$scripts_dir/agents.sh"
credit_file="$HOME/.devws-state/opencode-credit.tsv"
tab="$(printf '\t')"

fail() {
  printf 'opencode_credit.sh: %s\n' "$*" >&2
  exit "${code:-1}"
}

set_balance() {
  local agent="$1" balance="$2" others=""
  opencode_db "$agent" >/dev/null || fail "$agent is not an opencode agent"
  [[ "$balance" =~ ^[0-9]+(\.[0-9]+)?$ ]] || fail "the balance is US\$ in numbers, as 12.50: $balance"
  mkdir -p "$(dirname "$credit_file")"
  [ -f "$credit_file" ] && others="$(awk -F "$tab" -v agent="$agent" '$1 != agent' "$credit_file")"
  {
    [ -z "$others" ] || printf '%s\n' "$others"
    printf '%s\t%s\t%s\n' "$agent" "$balance" "${DEVWS_TOKENS_NOW:-$(date +%s)}"
  } > "$credit_file.$$" && mv "$credit_file.$$" "$credit_file"
  printf 'Balance of %s: US$ %.2f\n' "$agent" "$balance"
}

show() {
  local agent="$1" db balance since
  db="$(opencode_db "$agent")" || return 0
  [ -f "$credit_file" ] || return 0
  IFS="$tab" read -r balance since < <(awk -F "$tab" -v OFS="$tab" -v agent="$agent" '$1 == agent { print $2, $3 }' "$credit_file")
  [ -n "${balance:-}" ] || return 0
  printf '%.2f %s\n' "$balance" "$("$scripts_dir/opencode_usage.sh" spent "$db" "$since")"
}

case "${1:-}" in
  set) [ "$#" -eq 3 ] || code=2 fail "usage: opencode_credit.sh set <agent> <US\$>"; set_balance "$2" "$3" ;;
  show) [ "$#" -eq 2 ] || code=2 fail "usage: opencode_credit.sh show <agent>"; show "$2" ;;
  *) code=2 fail "usage: opencode_credit.sh set <agent> <US\$> | show <agent>" ;;
esac
