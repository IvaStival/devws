#!/usr/bin/env bash
# opencode in devws (agents.sh loads it; docs/architecture/agent-modules.md
# lists the hooks). An account is an XDG data folder: opencode keeps its
# login and conversations in <folder>/opencode. Its devws plugin
# (devws_plugin.js) stands in for hooks; usage.sh reads what it spent from
# its database, credit.sh keeps a prepaid balance, and menu.sh is the devws
# menu's ◆ OpenCode screen.

opencode_program() { printf 'opencode'; }
opencode_default_home() { printf '%s' "$HOME/.local/share"; }
opencode_account_variables() { printf 'DEVWS_OPENCODE_DB\n'; }
# opencode has no folder variable of its own: XDG_DATA_HOME moves its login
# and conversations (and those of the agent's own commands).
opencode_account_env() { export XDG_DATA_HOME="$1"; }

# <agent>: its database, where its conversations and their cost are.
opencode_db() {
  local home
  [ "$(agent_kind "$1")" = opencode ] || return 1
  home="$(agent_home "$1")" || return 1
  printf '%s' "$home/opencode/opencode.db"
}

opencode_launch() {
  local resume="$1" prompt_file="$2" config module
  shift 2
  module="$(agent_module_dir opencode)"
  # Where its usage is read (usage.sh), told to the plugin.
  export DEVWS_OPENCODE_DB="${XDG_DATA_HOME:-$HOME/.local/share}/opencode/opencode.db"
  # Passed as config content, so the user's own opencode config stays: the
  # plugin that reports to devws, the editor prompt, and the two agents
  # devAgentics starts (agent_args.rs): the floor's edits its worktree, a
  # senior's has no edit tools (its orders forbid every file).
  config="$(jq -n --arg plugin "file://$module/devws_plugin.js" \
    --arg prompt "$prompt_file" '{
      plugin: [$plugin],
      instructions: [$prompt],
      agent: {
        "devws-floor": {mode: "primary", description: "devws factory floor: works in its worktree",
          permission: {edit: "allow", bash: "allow", webfetch: "allow"}},
        "devws-senior": {mode: "primary", description: "devws senior: reads and plans, never edits",
          permission: {edit: "deny", bash: "allow", webfetch: "allow"}}
      }
    }')"
  OPENCODE_CONFIG_CONTENT="$config" opencode ${resume:+--continue} "$@"
}

# Ctrl-C quits it; Escape stops the turn once pressed again ("esc again to
# interrupt").
opencode_interrupt_keys() { printf 'Escape Escape'; }

# The models its logins give it, one provider/model a line.
opencode_models() {
  command -v opencode >/dev/null 2>&1 || { printf 'opencode is not installed: npm i -g opencode-ai (see opencode.ai)\n' >&2; return 1; }
  XDG_DATA_HOME="$(agent_home "$1")" opencode models
}

# A prepaid (Zen) account's balance and what was spent since (credit.sh).
opencode_credit() { "$(agent_module_dir opencode)/credit.sh" show "$1"; }

# The plugin passes the edited file.
opencode_follow() {
  local file
  file="$(jq -r '.tool_input.file_path // empty' 2>/dev/null)"
  printf '%s\n%s\n%s\n' --follow "$file" ""
}

# ---- token usage (run inside header_tokens.sh)

# A conversation is <db>#<session> (usage.sh).
opencode_owns_transcript() {
  case "$1" in *.db\#*) return 0 ;; esac
  return 1
}
opencode_usage() { "$(agent_module_dir opencode)/usage.sh" usage "$1"; }

# Billed per response by the provider: every cost is known.
opencode_month() {
  printf '%s 0\n' "$("$(agent_module_dir opencode)/usage.sh" spent "$2/opencode/opencode.db" "$1")"
}

# Every response is in one database, read in one query: the history's rows.
opencode_usage_rows() { "$(agent_module_dir opencode)/usage.sh" rows "$1/opencode/opencode.db"; }
opencode_usage_database() { printf '%s' "$1/opencode/opencode.db"; }

# ---- team and menu

# The team reaches it through its own switches (devagentics opencode): an
# account only when registered.
opencode_team_account() { return 1; }
# It works in a team, but does not run it (devagentics opencode).
opencode_can_lead() { return 1; }
opencode_menu_entry() { printf 'agent|opencode|◆ OpenCode||agent_screen opencode\n'; }
opencode_screen() { "$(agent_module_dir opencode)/menu.sh"; }
