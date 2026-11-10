#!/usr/bin/env bash
# Registers devws agents in .env (the format agents.sh reads), for the devws
# menu's Register agent / Remove agent:
#
#   register_agent.sh add <name> <claude|codex> <folder>
#   register_agent.sh remove <name>
#   register_agent.sh folder <name> <folder>
#
# add lists the name in DEVWS_AGENTS and writes DEVWS_AGENT_<name>="<kind>
# <folder>", the folder under ~ when it is in the home folder. remove drops
# both, keeps at least one agent, and moves DEVWS_DEFAULT_AGENT off a removed
# default. folder writes a registered name's entry with another folder (a bare
# name too, so it runs with that folder set, agents.sh). The header is
# restarted so its rows follow at once.
# DEVWS_ENV_FILE replaces ~/.tmuxinator/.env, for tests.

set -u

scripts_dir="$(cd "$(dirname "$0")" && pwd)"
env_file="${DEVWS_ENV_FILE:-$HOME/.tmuxinator/.env}"

die() {
  printf 'devws: %s\n' "$1" >&2
  exit 2
}

# The file a symlink chain ends at: written in place, so ~/.tmuxinator/.env
# keeps pointing at the checkout's .env.
resolve() {
  local path="$1" link
  while [ -L "$path" ]; do
    link="$(readlink "$path")"
    case "$link" in
      /*) path="$link" ;;
      *) path="$(dirname "$path")/$link" ;;
    esac
  done
  printf '%s' "$path"
}

[ -r "$env_file" ] || die "no devws settings at $env_file"
target="$(resolve "$env_file")"

# The registry as it stands (agents.sh's view of it).
# shellcheck source=/dev/null
. "$env_file"
# shellcheck source=agents.sh
. "$scripts_dir/agents.sh"
agents=" ${DEVWS_AGENTS:-claude codex} "

listed() {
  case "$agents" in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
  esac
}

# Rewrites .env through awk with the given program's variables, then swaps it in.
rewrite() {
  local temporary
  temporary="$(mktemp "$target.XXXXXX")" || die "cannot write $target"
  if awk "$@" "$target" > "$temporary"; then
    cat "$temporary" > "$target"
    rm -f "$temporary"
  else
    rm -f "$temporary"
    die "cannot update $target"
  fi
}

# The header reads the registry when its ticker starts.
restart_header() {
  [ -n "$(tmux show-option -gqv @devws_header_ticker 2>/dev/null)" ] || return 0
  "$scripts_dir/header_ticker.sh" --start >/dev/null 2>&1 || true
}

# <folder>: the existing folder, absolute; written under ~ when in the home folder.
written_folder() {
  local folder="$1" home
  case "$folder" in
    "~") home="$HOME" ;;
    "~/"*) home="$HOME/${folder#\~/}" ;;
    *) home="$folder" ;;
  esac
  [ -d "$home" ] || die "folder does not exist: $folder"
  home="$(cd "$home" && pwd)"
  case "$home" in
    "$HOME") printf '~' ;;
    "$HOME"/*) printf '~/%s' "${home#"$HOME"/}" ;;
    *) printf '%s' "$home" ;;
  esac
}

# <name> <list> <entry>: the list replaces DEVWS_AGENTS (added at the end if
# missing); the entry replaces the name's own and goes after the last agent
# setting.
write_entry() {
  rewrite -v name="$1" -v list="$2" -v entry="$3" '
    { lines[NR] = $0 }
    /^DEVWS_(AGENTS|DEFAULT_AGENT|AGENT_[A-Za-z0-9_]+)=/ { last = NR }
    /^DEVWS_AGENTS=/ { lines[NR] = "DEVWS_AGENTS=\"" list "\""; listed = 1 }
    index($0, "DEVWS_AGENT_" name "=") == 1 { lines[NR] = "" ; skip[NR] = 1 }
    END {
      for (n = 1; n <= NR; n++) {
        if (!skip[n]) print lines[n]
        if (n == last) print entry
      }
      if (!listed) print "DEVWS_AGENTS=\"" list "\""
      if (!last) print entry
    }'
}

# With ai-memory installed, the account in its folder gets memory too (memory.sh).
wire_memory() {
  local name="$1" memory="$scripts_dir/memory.sh"
  [ -x "$memory" ] && "$memory" available || return 0
  if "$memory" wire "$name" >/dev/null 2>&1; then
    printf 'Wired %s to ai-memory\n' "$name"
  else
    printf 'Could not wire %s to ai-memory; run devws memory wire\n' "$name"
  fi
}

add() {
  local name="$1" kind="$2" folder="$3" written
  agent_valid_name "$name" || die "an agent name uses only letters, digits and _: $name"
  listed "$name" && die "$name is already registered"
  agent_default_home "$kind" >/dev/null || die "unknown agent kind: $kind (claude or codex)"
  written="$(written_folder "$folder")" || exit
  write_entry "$name" "$(printf '%s' "$agents $name" | xargs)" "DEVWS_AGENT_$name=\"$kind $written\""
  restart_header
  printf 'Registered %s (%s, %s)\n' "$name" "$kind" "$written"
  wire_memory "$name"
}

# The agents already running keep their folder until restarted.
folder() {
  local name="$1" kind written
  listed "$name" || die "$name is not registered"
  kind="$(agent_kind "$name")" || die "$name is not registered"
  written="$(written_folder "$2")" || exit
  write_entry "$name" "$(printf '%s' "$agents" | xargs)" "DEVWS_AGENT_$name=\"$kind $written\""
  restart_header
  printf '%s now runs in %s (restart its agents to use it)\n' "$name" "$written"
  wire_memory "$name"
}

remove() {
  local name="$1" list default
  listed "$name" || die "$name is not registered"
  list="$(printf '%s' "$agents" | tr ' ' '\n' | awk -v name="$name" 'NF && $0 != name' | xargs)"
  [ -n "$list" ] || die "$name is the last agent; register another one first"
  default="${DEVWS_DEFAULT_AGENT:-}"
  [ "$default" != "$name" ] || default="${list%% *}"

  rewrite -v name="$name" -v list="$list" -v default="$default" '
    index($0, "DEVWS_AGENT_" name "=") == 1 { next }
    /^DEVWS_AGENTS=/ { print "DEVWS_AGENTS=\"" list "\""; next }
    /^DEVWS_DEFAULT_AGENT=/ && default != "" { print "DEVWS_DEFAULT_AGENT=\"" default "\""; next }
    { print }'
  restart_header
  printf 'Removed %s\n' "$name"
}

case "${1:-}" in
  add)
    [ "$#" -eq 4 ] || die "usage: register_agent.sh add <name> <claude|codex> <folder>"
    add "$2" "$3" "$4"
    ;;
  remove)
    [ "$#" -eq 2 ] || die "usage: register_agent.sh remove <name>"
    remove "$2"
    ;;
  folder)
    [ "$#" -eq 3 ] || die "usage: register_agent.sh folder <name> <folder>"
    folder "$2" "$3"
    ;;
  *) die "usage: register_agent.sh add <name> <claude|codex> <folder> | remove <name> | folder <name> <folder>" ;;
esac
