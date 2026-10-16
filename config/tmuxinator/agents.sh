#!/usr/bin/env bash
# The agents registered in devws (.env): DEVWS_AGENTS lists their names, and
# each name's DEVWS_AGENT_<name>="<kind> <folder>" says which program it runs
# (claude or codex) and the config folder it runs with (CLAUDE_CONFIG_DIR or
# CODEX_HOME). A name without an entry that is itself a kind, as in .env files
# from before the registry, is that kind in its default folder.
#
# Source this file; it reads the variables already set (it does not load .env).

# The kind's own folder, where the program looks when no folder is given.
agent_default_home() {
  case "$1" in
    claude) printf '%s' "$HOME/.claude" ;;
    codex) printf '%s' "$HOME/.codex" ;;
    *) return 1 ;;
  esac
}

# A registered name is a valid variable name: letters, digits and _.
agent_valid_name() {
  [[ "${1:-}" =~ ^[A-Za-z0-9_]+$ ]]
}

# <name>: "<kind> <folder>" as registered, or the kind itself for an old name.
agent_entry() {
  local name="$1" entry
  agent_valid_name "$name" || return 1
  entry="$(eval "printf '%s' \"\${DEVWS_AGENT_$name:-}\"")"
  if [ -z "$entry" ]; then
    agent_default_home "$name" >/dev/null || return 1
    entry="$name"
  fi
  printf '%s' "$entry"
}

# <name>: claude or codex; fails for a name that is not registered.
agent_kind() {
  local entry kind
  entry="$(agent_entry "$1")" || return 1
  kind="${entry%% *}"
  agent_default_home "$kind" >/dev/null || return 1
  printf '%s' "$kind"
}

# <name>: the config folder, ~ expanded; the kind's default when none is given.
agent_home() {
  local entry kind home
  entry="$(agent_entry "$1")" || return 1
  kind="$(agent_kind "$1")" || return 1
  home=""
  [ "$entry" = "${entry#* }" ] || home="${entry#* }"
  case "$home" in
    "") home="$(agent_default_home "$kind")" ;;
    "~") home="$HOME" ;;
    "~/"*) home="$HOME/${home#\~/}" ;;
  esac
  printf '%s' "${home%/}"
}

# <name>: whether it runs in its kind's default folder, where the program needs
# no folder set (and setting one would change where Claude Code keeps its login).
agent_is_default_home() {
  local kind
  kind="$(agent_kind "$1")" || return 1
  [ "$(agent_home "$1")" = "$(agent_default_home "$kind")" ]
}
