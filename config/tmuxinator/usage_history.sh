#!/usr/bin/env bash
# The usage history (the devws menu's ◷ Usage history, `devws usage`): what
# every conversation of every agent in DEVWS_AGENTS used, from their own logs
# (header_tokens.sh), by workspace folder, by agent team and by account, for
# today, the last 7 and 30 days and all time, at API prices.
#
#   usage_history.sh show   the table
#   usage_history.sh page   the table in less (the menu's popup), saying
#                           first that the logs are being read
#   usage_history.sh rows   every response: "<account> <folder> <id> <day>
#                           <input> <output> <cache> <US$> <unpriced>",
#                           tab-separated, a message repeated in other logs
#                           left in (show counts it once)
#
# A log's lines are kept in $TMPDIR/devws-usage-<uid>/ with its size and
# modification time; only a new or changed log is read again.
# DEVWS_TOKENS_NOW (epoch seconds) replaces the current time, for tests.

set -u

scripts_dir="$(cd "$(dirname "$0")" && pwd)"
# The agents and their folders (DEVWS_AGENTS, DEVWS_AGENT_<name>) are in .env,
# exported for agents.sh. DEVWS_ENV_FILE replaces .env, for tests.
env_file="${DEVWS_ENV_FILE:-$scripts_dir/.env}"
set -a
# shellcheck source=/dev/null
[ -r "$env_file" ] && . "$env_file"
set +a
# shellcheck source=agents.sh
. "$scripts_dir/agents.sh"
now="${DEVWS_TOKENS_NOW:-$(date +%s)}"
tasks_dir="${DEVAGENTICS_HOME:-$HOME/.devagentics}/tasks"
cache_dir="${TMPDIR:-/tmp}"
cache_dir="${cache_dir%/}/devws-usage-$(id -u)"
tab="$(printf '\t')"

# Every log under <folder>, with its size and modification time (it changed
# when they did): "<size>:<mtime><TAB><path>". One stat for them all.
fingerprints() {
  find "$1" -name '*.jsonl' -exec stat -f "%z:%m$tab%N" {} + 2>/dev/null ||
    find "$1" -name '*.jsonl' -exec stat -c "%s:%Y$tab%n" {} +
}

# <log> <fingerprint> <entry>: the log's cache entry, its fingerprint, its
# folder, then its responses (header_tokens.sh messages).
read_log() {
  local log="$1" print="$2" entry="$3" folder
  # Both programs name the folder a conversation ran in near its start.
  folder="$(grep -m 1 -o '"cwd":"[^"]*"' "$log" | sed 's/^"cwd":"//; s/"$//')"
  {
    printf '%s\n%s\n' "$print" "${folder:--}"
    "$scripts_dir/header_tokens.sh" messages "$log"
  } > "$entry.$$" && mv "$entry.$$" "$entry"
}

# Only a new or changed log starts a process: the others are checked with
# the shell's own read, and every entry is printed by one awk.
rows() {
  local agent kind home root seen="" print log entry stored
  mkdir -p "$cache_dir" && chmod 700 "$cache_dir"
  for agent in ${DEVWS_AGENTS-claude codex}; do
    kind="$(agent_kind "$agent")" || continue
    home="$(agent_home "$agent")" || continue
    case "$kind" in
      claude) root="$home/projects" ;;
      codex) root="$home/sessions" ;;
      *) continue ;;
    esac
    # Two names for one folder are one account's logs, counted once.
    case "$seen" in *"$tab$root$tab"*) continue ;; esac
    seen="$seen$tab$root$tab"
    [ -d "$root" ] || continue
    local entries=()
    while IFS="$tab" read -r print log; do
      [ -n "$log" ] || continue
      entry="$cache_dir/${log//\//%}"
      stored=""
      [ -f "$entry" ] && IFS= read -r stored < "$entry"
      [ "$stored" = "$print" ] || read_log "$log" "$print" "$entry"
      [ -f "$entry" ] && entries+=("$entry")
    done < <(fingerprints "$root")
    [ "${#entries[@]}" -gt 0 ] || continue
    awk -v account="$agent" -v OFS="$tab" '
      FNR == 1 { next }
      FNR == 2 { folder = $0; next }
      { print account, folder, $1, $2, $3, $4, $5, $6, $7 }' "${entries[@]}"
  done
}

# The day <days> days before today, local: 2026-11-07.
days_ago() {
  date -r "$now" -v-"$1"d +%F 2>/dev/null || date -d "@$((now - $1 * 86400))" +%F
}

# Each task's own folders (its lanes, its agents') and its repos' worktrees,
# where its leader and seniors work: "<task> <folder>" per line. A task that
# is gone still has its folders named in the logs (tasks/<task>/…).
team_folders() {
  local task_file task
  for task_file in "$tasks_dir"/*/task.toml; do
    [ -f "$task_file" ] || continue
    task="$(basename "$(dirname "$task_file")")"
    sed -n 's/^worktree = "\(.*\)"$/\1/p' "$task_file" | while IFS= read -r folder; do
      case "$folder" in "$tasks_dir"/*) ;; *) printf '%s\t%s\n' "$task" "$folder" ;; esac
    done
  done
}

show() {
  local today week month
  today="$(days_ago 0)"
  week="$(days_ago 6)"
  month="$(days_ago 29)"
  rows | awk -F "$tab" -v today="$today" -v week="$week" -v month="$month" \
    -v tasks="$tasks_dir/" -v home="$HOME" -v teams="$(team_folders)" '
    BEGIN {
      count = split(teams, lines, "\n")
      for (line = 1; line <= count; line++) {
        split(lines[line], fields, "\t")
        if (fields[2] != "") leader_team[fields[2]] = fields[1]
      }
      period[1] = "today"; period[2] = "7 days"; period[3] = "30 days"; period[4] = "all time"
    }
    # A Claude message repeated in a resumed conversation counts once.
    $3 != "-" { if (($3) in seen) next; seen[$3] = 1 }
    {
      folder = $2; day = $4
      tokens = $5 + $6 + $7
      team = ""
      if (index(folder, tasks) == 1) {
        rest = substr(folder, length(tasks) + 1)
        team = substr(rest, 1, index(rest "/", "/") - 1)
      } else if (folder in leader_team) {
        team = leader_team[folder]
      }
      for (p = 1; p <= 4; p++) {
        if ((p == 1 && day != today) || (p == 2 && day < week) || (p == 3 && day < month)) continue
        add("account", $1, p, tokens, $8, $9)
        add("total", "all agents", p, tokens, $8, $9)
        if (team != "") add("team", team, p, tokens, $8, $9)
        if (index(folder, tasks) != 1) add("workspace", workspace_name(folder), p, tokens, $8, $9)
      }
    }
    function workspace_name(folder,    parts, count) {
      if (folder == "-" || folder == "") return "(unknown folder)"
      if (folder == home) return "~"
      count = split(folder, parts, "/")
      return parts[count]
    }
    function add(section, name, p, tokens, cost, unpriced,    key) {
      key = section SUBSEP name
      names[key] = 1
      used[key, p] += tokens
      spent[key, p] += cost
      if (unpriced) partial[key, p] = 1
    }
    function human(count) {
      if (count < 1000) return sprintf("%d", count)
      if (count < 1000000) return sprintf("%dk", count / 1000)
      if (count < 1000000000) return sprintf("%.1fM", count / 1000000)
      return sprintf("%.1fB", count / 1000000000)
    }
    function cell(key, p) {
      if (!((key, p) in used)) return sprintf("%-17s", "·")
      return sprintf("%-17s", sprintf("$%.2f%s %s", spent[key, p], ((key, p) in partial) ? "+" : "", human(used[key, p])))
    }
    function section(title, kind,    key, split_key, order, total, i, j, swap) {
      total = 0
      for (key in names) {
        split(key, split_key, SUBSEP)
        if (split_key[1] == kind) order[++total] = key
      }
      if (total == 0) return
      # Most spent all time first.
      for (i = 2; i <= total; i++)
        for (j = i; j > 1 && spent[order[j], 4] > spent[order[j - 1], 4]; j--) {
          swap = order[j]; order[j] = order[j - 1]; order[j - 1] = swap
        }
      printf "\n%s\n", title
      for (i = 1; i <= total; i++) {
        split(order[i], split_key, SUBSEP)
        printf "  %-34s %s %s %s %s\n", substr(split_key[2], 1, 34), cell(order[i], 1), cell(order[i], 2), cell(order[i], 3), cell(order[i], 4)
      }
    }
    END {
      printf "USAGE HISTORY · cost at API prices (+: a model without a price) and tokens\n"
      printf "  %-34s %-17s %-17s %-17s %-17s\n", "", period[1], period[2], period[3], period[4]
      section("WORKSPACES", "workspace")
      section("AGENT TEAMS", "team")
      section("ACCOUNTS", "account")
      section("TOTAL", "total")
    }'
}

# Reading every log the first time takes a while: say so, then page it.
page() {
  local output status
  output="$(mktemp "${TMPDIR:-/tmp}/devws-usage-page.XXXXXX")" || return
  printf "Reading the agents' logs…\n"
  show > "$output"
  status=$?
  if [ -t 1 ] && command -v less >/dev/null 2>&1; then
    less -+F -S "$output"
  else
    cat "$output"
  fi
  rm -f "$output"
  return "$status"
}

case "${1:-show}" in
  show) show ;;
  page) page ;;
  rows) rows ;;
  *)
    printf 'usage: usage_history.sh [show|page|rows]\n' >&2
    exit 2
    ;;
esac
