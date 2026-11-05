#!/usr/bin/env bash
# `devws agentic`: the workspace of an agent team (agentic/). The user talks
# to the leader; the leader opens the agents' panels as it needs them
# (agent_cli.sh spawn --place team). The repo is the one devws already has: a
# clone under DEVWS_HOME and a worktree for the task.
#
#   agentic.sh [--editor]                         the start screen: choose the
#                                                 repo and the branch
#   agentic.sh [--editor] <folder>                a folder as it is
#   agentic.sh [--editor] <repo or folder> <branch>
#                                                 the branch's worktree
#                                                 (devws worktree ensure)
#
# --editor adds a Neovim panel under the board. The leader, its arguments and
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
usage: devws agentic [--editor] [<repo or folder> [<branch>]]"
}

with_editor=0
places=()
for argument in "$@"; do
  case "$argument" in
    --editor) with_editor=1 ;;
    --*) usage_fail "unknown option: $argument" ;;
    *) places+=("$argument") ;;
  esac
done

# Before anything is made: the start screen clones and makes a worktree.
command -v "$agentic_command" >/dev/null 2>&1 ||
  fail "$agentic_command is not installed: an agent team needs it built from agentic/ (needs Rust: run ./install.sh in your devws checkout)"

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

started="$("$agentic_command" task start "$folder")" || fail "$agentic_command task start failed"
task="$(sed -n 1p <<<"$started")"
leader="$(sed -n 2p <<<"$started")"
leader_arguments="$(sed -n 3p <<<"$started")"
[ -n "$task" ] && [ -n "$leader" ] && [ -n "$leader_arguments" ] ||
  fail "$agentic_command task start printed '$started', not a task id, the leader's agent and its arguments file"

session="$("$devws" open --no-attach --template agentic --task "$task" \
  --agent-args "$leader_arguments" --set "agentic_command=$agentic_command" "$folder" "$leader")" || exit 1

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
