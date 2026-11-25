#!/usr/bin/env bash
# The devws menu's ◆ Antigravity screen: the Antigravity (agy) account devws
# runs, its sign-in, what its Gemini quota has used and its month's tokens,
# the models it can run, and devws's plugin in agy's config folder
# (plugin.sh).
#
#   menu.sh   (in the menu's popup; Esc leaves)
#
# agy keeps one account, in its own folder: the screen registers that one.
# DEVWS_PICKER names the picker and DEVWS_ENV_FILE the .env, for tests.

set -u

module_dir="$(cd "$(dirname "$0")" && pwd)"
scripts_dir="$(cd "$module_dir/../.." && pwd)"
picker="${DEVWS_PICKER:-$scripts_dir/picker.sh}"
env_file="${DEVWS_ENV_FILE:-$scripts_dir/.env}"
plugin="${DEVWS_AGY_CONFIG:-$HOME/.gemini/config}/plugins/devws"
# The name it gets when registered here, and its folder: agy's own.
default_name=antigravity
default_folder="~/.gemini/antigravity-cli"

pause() {
  read -r -p 'Press Enter to return…' || :
}

# .env again: registering the account here changes it.
load_env() {
  # shellcheck source=/dev/null
  [ -r "$env_file" ] && . "$env_file"
  # shellcheck source=agents.sh
  . "$scripts_dir/agents.sh"
}

# The registered agent that runs Antigravity, if any: the first one listed.
account() {
  local agent
  for agent in ${DEVWS_AGENTS:-claude codex}; do
    [ "$(agent_kind "$agent" 2>/dev/null)" = antigravity ] && { printf '%s' "$agent"; return 0; }
  done
  return 1
}

installed() {
  command -v agy >/dev/null 2>&1
}

# "5h 3% · wk 12%" from the status line's last reading, or why there is none.
quota_line() {
  local session week
  read -r session week _ <<<"$(agent_limits "$1")"
  if [ -z "${session:-}" ]; then
    printf 'Quota: not read yet (an agy pane in devws reads it)'
    return
  fi
  printf 'Quota: 5h %s%% · week %s%% used' "$session" "${week:--}"
}

tokens_line() {
  local tokens
  tokens="$(DEVWS_AGENTS="$1" "$scripts_dir/header_tokens.sh" costs 2>/dev/null | awk '{ print $4 }')"
  printf 'This month: %s tokens' "${tokens:-0}"
}

rows() {
  local name
  if ! installed; then
    printf 'install  agy is not installed: see antigravity.google\n'
  fi
  if ! name="$(account)"; then
    printf 'account  Account: not registered: register as %s\n' "$default_name"
  else
    printf 'account  Account: %s (%s)\n' "$name" "$(agent_home "$name")"
    printf 'login    Sign in again (opens agy)\n'
    printf 'quota    %s\n' "$(quota_line "$name")"
    printf 'tokens   %s\n' "$(tokens_line "$name")"
    printf 'models   Models it can run\n'
  fi
  if [ -f "$plugin/plugin.json" ]; then
    printf 'plugin   devws plugin: installed (%s) · remove\n' "$plugin"
  else
    printf 'plugin   devws plugin: not installed (each devws agy start installs it) · install\n'
  fi
}

register() {
  account >/dev/null && return 0
  "$scripts_dir/register_agent.sh" add "$default_name" antigravity "$default_folder" || return
  load_env
}

# agy signs in on its first start, in the browser.
login() {
  installed || { printf 'agy is not installed: see antigravity.google\n'; return 1; }
  printf 'agy starts: sign in if it asks, then leave it with Ctrl-C.\n'
  agy
}

models() {
  printf '\033c'
  "$scripts_dir/agent_cli.sh" models "$(account)"
  pause
}

toggle_plugin() {
  if [ -f "$plugin/plugin.json" ]; then
    "$module_dir/plugin.sh" remove
  else
    "$module_dir/plugin.sh" install
  fi
}

load_env
while :; do
  choice="$(rows | "$picker" Antigravity 'Antigravity in devws: Enter changes the line, Esc leaves')" || exit 0
  case "${choice%% *}" in
    '') exit 0 ;;
    account) register || pause ;;
    login) login || pause ;;
    models) models ;;
    plugin) toggle_plugin || pause ;;
  esac
done
