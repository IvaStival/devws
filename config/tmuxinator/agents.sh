#!/usr/bin/env bash
# The AI programs devws runs, and the agents registered with them.
#
# Each program is a module: a folder in agents/ (agents/10-claude,
# agents/40-antigravity…), its number the order programs are offered in, with
# an agent.sh that defines the program's hooks as <kind>_<hook> functions
# (docs/architecture/agent-modules.md). Installing a program is adding its
# folder: every script asks the modules (agent_hook) instead of naming
# programs itself.
#
# The agents registered in devws (.env): DEVWS_AGENTS lists their names, and
# each name's DEVWS_AGENT_<name>="<kind> <folder>" says which program it runs
# and the config folder it runs with (the module's account_env says how). A
# name without an entry that is itself a kind, as in .env files from before
# the registry, is that kind in its default folder.
#
# Source this file; it reads the variables already set (it does not load .env).

# The folder of the modules: beside this file in the checkout, through the
# link ~/.tmuxinator/agents.sh is. Plain bash here: scripts source this with
# little on their PATH.
agents_source="${BASH_SOURCE[0]}"
case "$agents_source" in */*) ;; *) agents_source="./$agents_source" ;; esac
if [ -L "$agents_source" ]; then
  agents_target="$(readlink "$agents_source")"
  case "$agents_target" in
    /*) agents_source="$agents_target" ;;
    *) agents_source="${agents_source%/*}/$agents_target" ;;
  esac
fi
agent_modules="$(cd "${agents_source%/*}" && pwd)/agents"
# The folder devws's scripts are in, for the modules: ~/.tmuxinator when
# sourced from there.
agents_source="${BASH_SOURCE[0]}"
case "$agents_source" in */*) ;; *) agents_source="./$agents_source" ;; esac
agent_scripts="$(cd "${agents_source%/*}" && pwd)"
unset agents_source agents_target

for agent_module in "$agent_modules"/*/agent.sh; do
  # shellcheck source=/dev/null
  [ -r "$agent_module" ] && . "$agent_module"
done
unset agent_module

# The programs, in their modules' order: claude, codex, …
agent_kinds() {
  local module name
  for module in "$agent_modules"/*/agent.sh; do
    [ -r "$module" ] || continue
    name="${module%/agent.sh}"
    name="${name##*/}"
    printf '%s\n' "${name#*-}"
  done
}

# <kind>: its module's folder, for the files it keeps there.
agent_module_dir() {
  local module
  for module in "$agent_modules"/*-"$1"/agent.sh; do
    [ -r "$module" ] && { printf '%s\n' "${module%/agent.sh}"; return 0; }
  done
  return 1
}

# Looked up often (every agent name): the list is read once.
agent_kind_list=" "
while IFS= read -r agent_module; do
  agent_kind_list+="$agent_module "
done < <(agent_kinds)
unset agent_module
agent_is_kind() {
  [ -n "${1:-}" ] && [[ "$agent_kind_list" == *" $1 "* ]]
}

# <kind> <hook> [args…]: the module's <kind>_<hook>, else the default_<hook>
# below with the kind first; fails when neither is defined, or the kind is
# not a module.
agent_hook() {
  local kind="$1" hook="$2"
  shift 2
  agent_is_kind "$kind" || return 1
  if declare -F "${kind}_$hook" >/dev/null; then
    "${kind}_$hook" "$@"
  elif declare -F "default_$hook" >/dev/null; then
    "default_$hook" "$kind" "$@"
  else
    return 1
  fi
}

# Whether the module defines the hook itself.
agent_defines() {
  declare -F "${1}_$2" >/dev/null
}

# The hooks a module must define; the others have defaults below.
agent_required_hooks="program default_home launch interrupt_keys"

# ----------------------------------------------------------------- defaults

# Its accounts' folders: the default one, then ~/.<kind> and ~/.<kind>-*.
default_default_folders() {
  local kind="$1" folder
  for folder in "$(agent_hook "$kind" default_home)" "$HOME/.$kind" "$HOME/.$kind"-*; do
    [ -d "$folder" ] && printf '%s\n' "$folder"
  done | awk '!seen[$0]++'
}
# The variables that point the program at an account; unset before a launch.
default_account_variables() { :; }
# The pane options it records on its pane: cleared when the pane is reused.
default_pane_options() { :; }
# <home>: exports what runs the program on that account.
default_account_env() { :; }
# <agent>: before a launch in the current folder.
default_prepare() { :; }
default_limits() { :; }
# <agent>: what `devws agent limits` answers; the limits by default.
default_live_limits() { agent_hook "$1" limits "$2"; }
default_default_model() { :; }
default_models() { printf 'the %s module lists no models\n' "$1" >&2; return 2; }
default_credit() { :; }
# <pane>: a mark to compare after an interrupt; <pane> <mark>: whether the
# program said the turn stopped, for one that runs no hook then.
default_interrupt_mark() { printf '0'; }
default_interrupt_seen() { return 1; }
# <transcript> <pane>: whether the turn was interrupted (agent_interrupt.sh).
default_interrupted() { return 1; }
# <payload>: what a status line payload says (agent_statusline.sh).
default_status_line_limits() { :; }
default_status_line_model() { :; }
default_status_line_extra() { :; }
default_settings_file() { :; }
# The hook payload on stdin: "<event>\n<file>\n<text>" (agent_follow_hook.sh).
default_follow() { cat >/dev/null; }
# Token usage (header_tokens.sh, which these run inside).
default_owns_transcript() { return 1; }
default_session() { :; }
default_usage() { :; }
default_messages() { :; }
default_model() { :; }
default_month() { printf '0 0\n'; }
# <home>: "<folder>\t<log file name>" its conversations' logs are, or nothing.
default_usage_logs() { :; }
# <home>: a program that keeps every response in one database: that database,
# and its rows for the usage history (usage_history.sh).
default_usage_database() { :; }
default_init_project() { printf 'Unsupported devws agent: %s\n' "$1" >&2; return 2; }
default_memory_client() { :; }
# Whether the team may start agents on it unregistered (devws agent
# accounts); one the team reaches through its own switches says no.
default_team_account() { return 0; }
default_can_lead() { return 0; }
# A line of the devws menu's Agent tab (devws_menu.sh), or nothing.
default_menu_entry() { :; }
# <agent> <root>: the message that asks the agent to commit (git_actions.sh).
default_commit_command() { :; }

# ----------------------------------------------------- for the modules

# The current folder when it is one devAgentics made for a task (its lanes,
# its agents'), which a program may be told to trust.
agent_task_folder() {
  local folder tasks
  folder="$(pwd -P)"
  tasks="$(cd "${DEVAGENTICS_HOME:-$HOME/.devagentics}/tasks" 2>/dev/null && pwd -P)" || return 1
  case "$folder/" in "$tasks"/*) printf '%s' "$folder" ;; *) return 1 ;; esac
}

# <project> <template> <program>: AGENTS.md (devws's CLAUDE.md, for
# <program>) and .agents/skills, for a program that reads them; prints
# "created:<file>" or "skipped:<why>" (init_agent.sh).
agent_init_agents_md() {
  local project_root="$1" template_root="$2" program="$3"
  if [ -e "$project_root/AGENTS.md" ]; then
    printf 'skipped:AGENTS.md already exists\n'
  else
    sed "s/CLAUDE\\.md/AGENTS.md/g; s/Claude Code/$program/g" \
      "$template_root/CLAUDE.md" > "$project_root/AGENTS.md"
    printf 'created:AGENTS.md\n'
  fi
  mkdir -p "$project_root/.agents/skills"
  cp -Rn "$template_root/skills/." "$project_root/.agents/skills/"
}

# The variables every module points its program at an account with: cleared
# before a launch, so none comes from the environment.
agent_account_variables() {
  local kind
  while IFS= read -r kind; do
    agent_hook "$kind" account_variables
  done < <(agent_kinds)
}

# ----------------------------------------------------------------- registry

# The kind's own folder, where the program looks when no folder is given.
agent_default_home() {
  agent_hook "$1" default_home
}

# <kind>: the command that runs it.
agent_program() {
  agent_hook "$1" program
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
    agent_is_kind "$name" || return 1
    entry="$name"
  fi
  printf '%s' "$entry"
}

# <name>: the program it runs (a module's kind); fails for a name that is not
# registered.
agent_kind() {
  local entry kind
  entry="$(agent_entry "$1")" || return 1
  kind="${entry%% *}"
  agent_is_kind "$kind" || return 1
  printf '%s' "$kind"
}

# The agents devws can run: those in DEVWS_AGENTS (claude and codex when it
# is unset), then each program that is installed and that no listed name
# runs, by its bare name, so an installed program is offered without being
# registered. DEVWS_OFFERED_PROGRAMS replaces the programs looked for, for
# tests.
agent_names() {
  local name kind kinds=" " names=()
  for name in ${DEVWS_AGENTS-claude codex}; do
    kind="$(agent_kind "$name" 2>/dev/null)" || continue
    names+=("$name")
    kinds+="$kind "
  done
  for kind in ${DEVWS_OFFERED_PROGRAMS-$(agent_kinds)}; do
    case "$kinds" in *" $kind "*) continue ;; esac
    [[ " ${names[*]-} " == *" $kind "* ]] && continue
    command -v "$(agent_program "$kind")" >/dev/null 2>&1 && names+=("$kind")
  done
  [ "${#names[@]}" -eq 0 ] || printf '%s\n' "${names[@]}"
}

# <name>: the model its program starts with when none is named (the module's
# default_model).
agent_default_model() {
  local kind
  kind="$(agent_kind "$1")" || return 1
  agent_hook "$kind" default_model "$1"
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

# <name>: whether it runs with a config folder set: one is registered for it,
# even its kind's default. Claude Code keeps a login per folder set, and one
# for none, so `CLAUDE_CONFIG_DIR=~/.claude claude` is not plain `claude`; a
# bare name runs as the plain command does.
agent_has_folder() {
  local entry
  entry="$(agent_entry "$1")" || return 1
  [ "$entry" != "${entry#* }" ]
}

# <name>: the plan limits the agent has used, in per cent: "<5-hour session>
# <week>", a "-" for what is not known (the module's limits; a status line's
# reading is followed by the epoch it was read at).
agent_limits() {
  local kind
  kind="$(agent_kind "$1")" || return 0
  agent_hook "$kind" limits "$1"
}
