#!/usr/bin/env bash
# The devws menu's ◆ OpenCode screen: team agents on opencode, switched on and
# off by hand (devagentics opencode), the models they run, the account they
# run on, and what it has used and has left.
#
#   menu.sh   (in the menu's popup; Esc leaves)
#
# The global switch is mandatory: while it is off, the floor's and the
# seniors' switches are kept but ignored, and no agent runs on opencode.
# DEVWS_AGENTIC_COMMAND names devagentics, DEVWS_PICKER the picker, for tests.

set -u

module_dir="$(cd "$(dirname "$0")" && pwd)"
scripts_dir="$(cd "$module_dir/../.." && pwd)"
agentic="${DEVWS_AGENTIC_COMMAND:-devagentics}"
picker="${DEVWS_PICKER:-$scripts_dir/picker.sh}"
env_file="${DEVWS_ENV_FILE:-$scripts_dir/.env}"
# The folder an opencode account gets when registered here: opencode's own.
default_folder="~/.local/share"

pause() {
  read -r -p 'Press Enter to return…' || :
}

# .env again: registering an account here changes it.
load_env() {
  # shellcheck source=/dev/null
  [ -r "$env_file" ] && . "$env_file"
  # shellcheck source=agents.sh
  . "$scripts_dir/agents.sh"
}

# Whether the account is one of devws's agents: listed, and running opencode.
# (A bare name that is a kind counts as one in agents.sh, listed or not.)
registered() {
  [[ " ${DEVWS_AGENTS:-claude codex} " == *" $account "* ]] &&
    [ "$(agent_kind "$account" 2>/dev/null)" = opencode ]
}

# <field>: one of `devagentics opencode show`'s lines, after its name.
field() {
  awk -F '\t' -v name="$1" '$1 == name { $1 = ""; sub(/^\t/, ""); print; exit }' OFS='\t' <<<"$settings"
}

# <account> <command…>: an opencode command on the account's own data.
on_account() {
  local account="$1"
  shift
  XDG_DATA_HOME="$(agent_home "$account")" "$@"
}

credit_line() {
  local credit balance used
  credit="$("$scripts_dir/agent_cli.sh" credit "$account" 2>/dev/null)"
  if [ -z "$credit" ]; then
    printf 'credit   Credit: no balance entered (Zen has no API for it) · set balance…'
    return
  fi
  read -r balance used <<<"$credit"
  LC_ALL=C awk -v balance="$balance" -v used="$used" 'BEGIN {
    printf "credit   Credit: US$ %.2f left of US$ %.2f (US$ %.2f used, estimated) · set balance…", balance - used, balance, used }'
}

rows() {
  local global floor senior floor_model senior_model registered
  global="$(field global)"
  IFS=$'\t' read -r floor floor_model <<<"$(field floor)"
  IFS=$'\t' read -r senior senior_model <<<"$(field senior)"
  registered="not registered: register and log in"
  ! registered || registered="$(agent_home "$account")"
  printf 'global   OpenCode (mandatory): %s\n' "$global"
  printf 'floor    Factory floor: %s\n' "$floor"
  printf 'floor-model    Floor model: %s\n' "$floor_model"
  printf 'senior   Seniors: %s\n' "$senior"
  printf 'senior-model   Senior model: %s\n' "$senior_model"
  printf 'account  Account: %s (%s)\n' "$account" "$registered"
  [ "$registered" = "${registered#not}" ] && printf 'login    Log in again (opencode auth login)\n'
  [ "$registered" = "${registered#not}" ] && printf '%s\n' "$(credit_line)"
  [ "$registered" = "${registered#not}" ] && printf 'stats    Usage statistics\n'
}

# <on|off|ignored>: the other one, for a switch shown so. A group "ignored"
# is on, held off by the global switch: choosing it turns it off.
flipped() {
  case "$1" in on | ignored) printf off ;; *) printf on ;; esac
}

register() {
  if ! registered; then
    mkdir -p "$HOME/.local/share"
    "$scripts_dir/register_agent.sh" add "$account" opencode "$default_folder" || return
    load_env
  fi
  login
}

login() {
  command -v opencode >/dev/null 2>&1 || {
    printf 'opencode is not installed: npm i -g opencode-ai (see opencode.ai)\n'
    return 1
  }
  printf 'Pick "OpenCode Zen" and paste its API key (opencode.ai, Zen).\n'
  on_account "$account" opencode auth login
}

# <group>: a model from those the account has, or one typed.
choose_model() {
  local group="$1" model
  model="$(
    {
      command -v opencode >/dev/null 2>&1 && on_account "$account" opencode models 2>/dev/null
      printf '%s\n' 'Other…' 'None'
    } | "$picker" Model "The model the $group runs on opencode"
  )"
  case "$model" in
    '') return ;;
    None) model="" ;;
    'Other…')
      read -e -r -p 'Model (provider/model): ' model
      [ -n "$model" ] || return
      ;;
  esac
  "$agentic" opencode model "$group" "$model" >/dev/null || pause
}

set_balance() {
  local balance
  printf 'The balance on opencode.ai now (Zen, Billing); what agents spend from now counts against it.\n'
  read -e -r -p 'US$: ' balance
  [ -n "$balance" ] || return
  "$module_dir/credit.sh" set "$account" "$balance" || pause
}

stats() {
  printf '\033c'
  printf '%s\n\n' "$(credit_line | sed 's/^credit   //; s/ · set balance…$//')"
  on_account "$account" opencode stats --models
  pause
}

load_env
while :; do
  settings="$("$agentic" opencode show)" || { pause; exit 1; }
  account="$(field account)"
  choice="$(rows | "$picker" OpenCode 'Team agents on opencode: Enter changes the line, Esc leaves')" || exit 0
  case "${choice%% *}" in
    '') exit 0 ;;
    global) "$agentic" opencode switch global "$(flipped "$(field global)")" >/dev/null ;;
    floor | senior)
      state="$(field "${choice%% *}")"
      "$agentic" opencode switch "${choice%% *}" "$(flipped "${state%%$'\t'*}")" >/dev/null
      ;;
    floor-model) choose_model floor ;;
    senior-model) choose_model senior ;;
    account) register || pause ;;
    login) login || pause ;;
    credit) set_balance ;;
    stats) stats ;;
  esac
done
