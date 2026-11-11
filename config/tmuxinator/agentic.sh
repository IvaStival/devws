#!/usr/bin/env bash
# `devws agentic`: the workspace of an agent team (agentic/). The user talks
# to the leader; the leader opens the agents' panels as it needs them
# (agent_cli.sh spawn --place team). The repo is the one devws already has: a
# clone under DEVWS_HOME and a worktree for the task.
#
#   agentic.sh [options]                          the start screen: choose the
#                                                 repo and the branch
#   agentic.sh [options] <folder>                 a folder as it is
#   agentic.sh [options] <repo or folder> <branch>
#                                                 the branch's worktree
#                                                 (devws worktree ensure)
#
# --editor adds a Neovim panel under the board. --leader <agent> names the
# registered agent that leads; without it, on a terminal, the registered
# agents are offered, the one leading the folder's team now first
# (`devagentics task leader`), and elsewhere the task decides. The leader, its arguments and
# the board's `board` are the agentic program's: `devagentics task start <folder>` prints
# the task id, the leader's agent and the path of its arguments file
# (DEVWS_AGENTIC_COMMAND replaces the program, for tests).

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
devws="${DEVWS_BIN:-devws}"
agentic_command="${DEVWS_AGENTIC_COMMAND:-devagentics}"

fail() {
  printf 'devws agentic: %s\n' "$*" >&2
  exit "${code:-1}"
}

usage_fail() {
  code=2 fail "$*
usage: devws agentic [--editor] [--leader <agent>] [<repo or folder> [<branch>]]"
}

with_editor=0
leader=""
places=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --editor) with_editor=1 ;;
    --leader)
      [ "$#" -ge 2 ] && [ -n "$2" ] || usage_fail "--leader needs an agent"
      leader="$2"
      shift
      ;;
    --leader=*) leader="${1#--leader=}" ;;
    --*) usage_fail "unknown option: $1" ;;
    *) places+=("$1") ;;
  esac
  shift
done

# shellcheck source=/dev/null
[ -r "$script_dir/.env" ] && . "$script_dir/.env"

# <folder>: the registered agent the user picks to lead, the one leading now
# offered first. Fails when the question is left (Esc).
choose_leader() {
  local now agent
  now="$("$agentic_command" task leader "$1")" || fail "$agentic_command task leader failed"
  {
    printf '%s\n' "$now"
    for agent in ${DEVWS_AGENTS:-claude codex}; do
      [ "$agent" = "$now" ] || printf '%s\n' "$agent"
    done
  } | "$script_dir/picker.sh" Leader 'The agent that leads the team'
}

# Before anything is made: the start screen clones and makes a worktree.
# Missing, it is offered: rust_tools.sh asks, then installs Rust or
# updates it as needed and builds it (rustup's cargo is in ~/.cargo/bin).
# A program the tests name instead is never installed.
PATH="$HOME/.cargo/bin:$PATH"
if ! command -v "$agentic_command" >/dev/null 2>&1; then
  if [ -n "${DEVWS_AGENTIC_COMMAND:-}" ] || ! "$script_dir/rust_tools.sh" install; then
    fail "$agentic_command is not installed: an agent team needs it built from agentic/ (devws agentic offers to install it, or run ./install.sh in your devws checkout)"
  fi
  hash -r
  command -v "$agentic_command" >/dev/null 2>&1 || fail "$agentic_command is still not on PATH after installing it"
fi

case "${#places[@]}" in
  0)
    # 1 is a cancelled question; anything else failed and said why.
    folder="$("$script_dir/github_workspace.sh")" || exit $(($? == 1 ? 0 : $?))
    ;;
  1)
    [ -d "${places[0]}" ] || usage_fail "${places[0]} is not a folder (a GitHub repo needs a branch too)"
    folder="$(cd "${places[0]}" && pwd -P)"
    ;;
  2) folder="$("$devws" worktree ensure "${places[0]}" "${places[1]}")" || exit 1 ;;
  *) usage_fail "too many arguments" ;;
esac

# One agent registered leaves nothing to choose.
if [ -z "$leader" ] && [ -t 0 ] && [ "$(printf '%s\n' ${DEVWS_AGENTS:-claude codex} | wc -l)" -gt 1 ]; then
  leader="$(choose_leader "$folder")" || exit 0
  [ -n "$leader" ] || exit 0
fi

started="$("$agentic_command" task start "$folder" ${leader:+--leader "$leader"})" ||
  fail "$agentic_command task start failed"
task="$(sed -n 1p <<<"$started")"
leader="$(sed -n 2p <<<"$started")"
leader_arguments="$(sed -n 3p <<<"$started")"
[ -n "$task" ] && [ -n "$leader" ] && [ -n "$leader_arguments" ] ||
  fail "$agentic_command task start printed '$started', not a task id, the leader's agent and its arguments file"

session="$("$devws" open --no-attach --template agentic --task "$task" \
  --agent-args "$leader_arguments" --set "agentic_command=$agentic_command" "$folder" "$leader")" || exit 1

# The size of the terminal the workspace will show in: the tmux client's, the
# terminal's, or COLUMNS and LINES; nothing when there is none (a script).
terminal_size() {
  if [ -n "${TMUX:-}" ] && tmux display-message -p '#{client_width} #{client_height}' 2>/dev/null | grep -q '^[1-9]'; then
    tmux display-message -p '#{client_width} #{client_height}'
  elif [ -t 1 ]; then
    printf '%s %s\n' "$(tput cols)" "$(tput lines)"
  elif [ -n "${COLUMNS:-}" ] && [ -n "${LINES:-}" ]; then
    printf '%s %s\n' "$COLUMNS" "$LINES"
  fi
}

# Made detached, the workspace has tmux's default size and only takes the
# terminal's when shown, scaling every pane: the board came out narrow. So it
# takes the terminal's size now and the board half of the room beside the
# sidebar, as wide as the leader; then it follows the terminal again.
fit_to_terminal() {
  local width height sidebar board
  read -r width height <<<"$(terminal_size)"
  [ "${width:-0}" -gt 0 ] && [ "${height:-0}" -gt 0 ] || return 0
  # The status line takes a row of the terminal.
  tmux resize-window -t "=$session:" -x "$width" -y "$((height - 1))" || return 0
  sidebar="$(tmux list-panes -t "=$session:" -F '#{pane_title} #{pane_width}' | awk '$1 == "devws-picker" { print $2; exit }')"
  board="$(tmux list-panes -t "=$session:" -F '#{pane_id} #{@devws_role}' | awk '$2 == "team-log" { print $1; exit }')"
  # Two borders: the sidebar's and the leader's.
  [ -z "$board" ] || tmux resize-pane -t "$board" -x "$(((width - ${sidebar:-0} - 2) / 2))"
  tmux set-option -wu -t "=$session:" window-size
}
fit_to_terminal

if [ "$with_editor" = 1 ]; then
  log="$(tmux list-panes -s -t "=$session" -F '#{pane_id} #{@devws_role}' | awk '$2 == "team-log" { print $1; exit }')"
  "$script_dir/new_panel.sh" "$session" below nvim --beside "$log" --no-focus >/dev/null ||
    fail "could not add the editor"
fi

# Where the user is: inside tmux, the workspace is switched to; on a terminal,
# attached to; otherwise (a script) only named.
if [ -n "${TMUX:-}" ]; then
  tmux switch-client -t "=$session"
elif [ -t 0 ] && [ -t 1 ]; then
  exec tmux attach-session -t "=$session"
else
  printf '%s\n' "$session"
fi
