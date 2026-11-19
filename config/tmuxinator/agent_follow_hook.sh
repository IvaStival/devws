#!/usr/bin/env bash
# The agents' hook: shows the file the agent just changed in the editor, at
# the change, or a file it read (shown only in the "follow edits and reads"
# mode). What a tool call changed or read is in its program's payload, which
# the program's module reads (follow, agents/*/agent.sh).
#
#   agent_follow_hook.sh <kind>          after a tool call
#   agent_follow_hook.sh <kind> pre      before one: the project is snapshot
#                                        (agent_changes.sh)
#   agent_follow_hook.sh <kind> prompt   the user's new prompt: the marks of
#                                        the last turn go
#   (hook JSON on stdin)
#
# In a git project, what a tool call changed is found from the project itself,
# so a shell command's changes are shown too: every changed line is marked,
# and the file changed last is shown at its first change. Elsewhere, and for
# reads, the tool call's own payload says what to show.
#
# It always exits 0: a hook that fails would interrupt the agent.

set -u

agent="${1:-}"
phase="${2:-post}"
payload="$(cat)"
scripts="$(dirname "$0")"
open="$scripts/editor_open.sh"

# Extra agents and workspaces without a linked editor have no consumer for
# repository snapshots. Check before parsing the payload or scanning Git.
[ -n "${TMUX_PANE:-}" ] || exit 0
snapshot="pane-$TMUX_PANE"
eligibility="$(tmux display-message -p -t "$TMUX_PANE" '#{@devws_editor_socket}|#{@devws_follow}' 2>/dev/null)"
socket="${eligibility%|*}"
mode="${eligibility##*|}"
if [ ! -S "$socket" ] || [ "$mode" = off ]; then
  "$scripts/agent_changes.sh" discard "" "$snapshot"
  exit 0
fi

json() {
  printf '%s' "$payload" | jq -r "$1" 2>/dev/null
}

# The project the agent works in, and this agent's own snapshot name: a
# workspace's extra agents keep theirs apart.
project="$(json '.cwd // empty')"
project="${project:-$PWD}"

case "$phase" in
  pre)
    "$scripts/agent_changes.sh" snapshot "$project" "$snapshot"
    exit 0
    ;;
  prompt)
    "$open" --clear-marks
    exit 0
    ;;
esac

# Changes found by snapshot: the file changed last is shown, at its first
# changed range; all of them are marked.
ranges="$("$scripts/agent_changes.sh" changes "$project" "$snapshot")"
if [ -n "$ranges" ]; then
  # Each file's modification time with its fraction: GNU stat's -c first, as
  # GNU's -f is about the filesystem; BSD's (macOS) has no -c and prints
  # nothing. C locale: a "." in both the times and the sort.
  latest="$(cut -f1 <<<"$ranges" | sort -u | while IFS= read -r changed_file; do
    [ -e "$changed_file" ] && printf '%s\t%s\n' \
      "$(LC_ALL=C stat -c %.9Y "$changed_file" 2>/dev/null || LC_ALL=C stat -f %Fm "$changed_file")" "$changed_file"
  done | LC_ALL=C sort -rn | head -n 1 | cut -f2-)"
  if [ -n "$latest" ]; then
    first="$(awk -F '\t' -v file="$latest" '$1 == file { print $2; exit }' <<<"$ranges")"
    printf '%s\n' "$ranges" | "$open" --follow-changes "$latest:$first"
    exit 0
  fi
fi

# Otherwise the tool call itself says what to show: the program's module
# reads its payload (follow): "<event>\n<file>\n<text>".
# shellcheck source=agents.sh
. "$scripts/agents.sh"
event="" file="" text=""
{
  IFS= read -r event
  IFS= read -r file
  IFS= read -r text
} < <(printf '%s' "$payload" | agent_hook "$agent" follow 2>/dev/null)
event="${event:---follow}"
[ -n "$file" ] && "$open" "$event" "$file" "$text"
exit 0
