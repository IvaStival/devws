#!/usr/bin/env bash
# ai-memory (github.com/akitaonrails/ai-memory) for devws's agents: its hooks
# record each session and its MCP server lets an agent search what was done
# before, in one store a per-user service keeps. `ai-memory setup` wires the
# default folders (~/.claude, ~/.codex); this wires every agent devws runs, in
# its own folder (CLAUDE_CONFIG_DIR / CODEX_HOME, as agent_runner.sh sets).
# devws keeps starting the agents itself.
#
#   memory.sh available        ai-memory is installed
#   memory.sh up               its service answers (/healthz)
#   memory.sh wire <agent>     MCP and hooks for that agent's account
#   memory.sh wire-all [--force]   every agent in DEVWS_AGENTS not wired yet
#                              (--force: every one)
#   memory.sh ask <session> <question>   asks the workspace's agent to search
#                              its memory (ai-memory has no search command:
#                              agents search through its MCP tool)
#   memory.sh recent [folder]  the newest memory pages of the folder's project
#   memory.sh web              opens the web UI; with no login yet (ai-memory
#                              has no user until one is added), first offers
#                              an admin (root) account named after you
#   memory.sh doctor           ai-memory's own checks
#
# AI_MEMORY_SERVER_URL replaces http://127.0.0.1:49374. DEVWS_ENV_FILE
# replaces .env, for tests.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
env_file="${DEVWS_ENV_FILE:-$script_dir/.env}"
[ -r "$env_file" ] && . "$env_file"
# shellcheck source=agents.sh
. "$script_dir/agents.sh"

server="${AI_MEMORY_SERVER_URL:-http://127.0.0.1:49374}"
# The ai-memory command; DEVWS_AI_MEMORY replaces it (the tests: a stub, or
# none at all whatever the machine has installed).
ai_memory="${DEVWS_AI_MEMORY:-ai-memory}"
# The agents wired, so an install or a registration does not wire them again
# (each wiring leaves ai-memory's backups of the files it touched).
wired_file="$HOME/.devws-state/memory-wired"

die() {
  printf 'devws memory: %s\n' "$1" >&2
  exit 2
}

available() {
  command -v "$ai_memory" >/dev/null 2>&1
}

up() {
  curl -fsS --max-time 1 "$server/healthz" >/dev/null 2>&1
}

# The agent's account, as agent_runner.sh starts it: none for a default folder.
with_account() {
  local agent="$1" kind
  shift
  kind="$(agent_kind "$agent")" || die "not a registered agent: $agent"
  if agent_is_default_home "$agent"; then
    env -u CLAUDE_CONFIG_DIR -u CODEX_HOME "$@"
  elif [ "$kind" = claude ]; then
    env -u CODEX_HOME CLAUDE_CONFIG_DIR="$(agent_home "$agent")" "$@"
  else
    env -u CLAUDE_CONFIG_DIR CODEX_HOME="$(agent_home "$agent")" "$@"
  fi
}

wire() {
  local agent="$1" client
  available || die "ai-memory is not installed"
  case "$(agent_kind "$agent")" in
    claude) client=claude-code ;;
    codex) client=codex ;;
    *) die "not a registered agent: $agent" ;;
  esac
  with_account "$agent" "$ai_memory" install-mcp --client "$client" --apply || return 1
  with_account "$agent" "$ai_memory" install-hooks --agent "$client" --apply || return 1
  mkdir -p "$(dirname "$wired_file")"
  grep -qxF "$agent" "$wired_file" 2>/dev/null || printf '%s\n' "$agent" >> "$wired_file"
}

# [--force]: wires the agents already wired again too.
wire_all() {
  local agent
  available || return 0
  for agent in ${DEVWS_AGENTS:-claude codex}; do
    [ "${1:-}" != --force ] && grep -qxF "$agent" "$wired_file" 2>/dev/null && continue
    wire "$agent" || printf 'devws memory: could not wire %s\n' "$agent" >&2
  done
}

ask() {
  local session="$1" question="$2" pane
  pane="$(tmux list-panes -s -t "=$session:" -F '#{pane_id}|#{@devws_role}' 2>/dev/null |
    awk -F '|' '$2 == "agent" { print $1; exit }')"
  [ -n "$pane" ] || die "no agent in $session"
  tmux send-keys -t "$pane" -l "Search your ai-memory for: $question"
  tmux send-keys -t "$pane" Enter
}

# The web UI needs a human login, which ai-memory does not create: with no
# user yet, the page stops at a login nobody has. A failing `user list`
# (an older ai-memory) is taken as users present, so the page still opens.
has_login() {
  local users
  users="$("$ai_memory" user list 2>/dev/null)" || return 0
  ! grep -q '(no registered users)' <<<"$users"
}

offer_login() {
  local username answer
  username="$(id -un)"
  printf 'ai-memory has no web login yet.\n'
  read -r -p "Create an admin (root) account \"$username\"? [Y/n] " answer || return 0
  case "$answer" in n | N | no) return 0 ;; esac
  "$ai_memory" user add-human --username "$username" --role root ||
    die "could not create the account; try: ai-memory user add-human --username $username --role root"
  printf '\nLog in with that temporary password: the page then asks for your own.\n'
  read -r -p 'Press Enter to open the web page… ' _ || true
}

web() {
  available || die "ai-memory is not installed"
  has_login || offer_login
  open "$server/web"
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  available) available ;;
  up) up ;;
  wire) [ -n "${1:-}" ] || die "no agent given"; wire "$1" ;;
  wire-all) wire_all "${1:-}" ;;
  ask) [ "$#" -eq 2 ] || die "usage: memory.sh ask <session> <question>"; ask "$1" "$2" ;;
  recent) available || die "ai-memory is not installed"; (cd "${1:-$PWD}" && "$ai_memory" recent) ;;
  web) web ;;
  doctor) available || die "ai-memory is not installed"; "$ai_memory" doctor ;;
  *) die "usage: memory.sh <available|up|wire|wire-all|ask|recent|web|doctor> …" ;;
esac
