#!/usr/bin/env bash
# Adds a panel to a workspace's agent group, beside the terminal (New panel in
# the devws menu, C-b N), so it minimizes, moves with the layout and is
# restored with the group. Prints the new pane's id.
#
#   new_panel.sh <session> agent <name>   an extra agent: started as the main
#                                         one is (agent_runner.sh --extra), with
#                                         its own state in its header
#   new_panel.sh <session> terminal       a shell
#   new_panel.sh <session> nvim           your Neovim (LazyVim) in the project,
#                                         not linked to the agent
#
# A minimized group is expanded first. The workspace is saved afterwards, so
# a restore brings the panel back.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
session="${1:-}"
kind="${2:-}"
name="${3:-}"

fail() {
  tmux display-message "New panel: $*" 2>/dev/null
  printf 'New panel: %s\n' "$*" >&2
  exit 1
}

# shellcheck source=/dev/null
[ -r "$script_dir/.env" ] && . "$script_dir/.env"
# shellcheck source=agents.sh
. "$script_dir/agents.sh"
# shellcheck source=workspace_panes.sh
. "$script_dir/workspace_panes.sh"

[ -n "$session" ] || fail "no workspace"
case "$kind" in
  agent)
    agent_kind "$name" >/dev/null 2>&1 || fail "unknown agent: $name (register it in the devws menu)"
    command="$(printf '%q %q --extra' "$script_dir/agent_runner.sh" "$name")"
    role=extra-agent
    ;;
  terminal)
    command=""
    role=extra
    ;;
  nvim)
    command -v nvim >/dev/null 2>&1 || fail "Neovim is not installed"
    command="nvim ."
    role=extra-editor
    ;;
  *) fail "unknown kind: $kind (agent, terminal, nvim)" ;;
esac

# While minimized the group's panes live in a hidden window.
"$script_dir/minimize_panes.sh" "$session" restore agents >/dev/null ||
  fail "could not expand the agent group"
load_workspace_panes "$session" || fail "workspace $session not found"
# The panel takes half of the widest of the terminal and the panels beside
# it, so adding several keeps their widths even instead of halving the
# terminal each time.
beside=""
widest=0
for candidate in $(pane_with_role terminal) $(extra_panes); do
  width="$(pane_field "$candidate" 6)"
  if [ "${width:-0}" -gt "$widest" ]; then
    beside="$candidate"
    widest="$width"
  fi
done
[ -n "$beside" ] || beside="$(agent_slot_pane)"
[ -n "$beside" ] || fail "no terminal or agent pane to open it beside"
root="$(tmux show-options -qv -t "=$session:" @devws_root 2>/dev/null)"

# The role is set here as well as by agent_runner.sh, so the save below
# already sees it.
pane="$(tmux split-window -h -t "$beside" -c "${root:-$PWD}" -P -F '#{pane_id}' ${command:+"$command"})" ||
  fail "could not open the panel"
tmux set-option -p -t "$pane" @devws_role "$role"
[ "$kind" = agent ] && tmux set-option -p -t "$pane" @devws_agent "$name"

"$script_dir/save_state.sh"
printf '%s\n' "$pane"
