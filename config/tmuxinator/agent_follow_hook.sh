#!/usr/bin/env bash
# The agents' hook: shows the file the agent just changed in the editor, at
# the change, or a file it read (shown only in the "follow edits and reads"
# mode). Claude Code passes the tool call's JSON (file_path, new_string or
# content, a Read's offset); Codex passes its apply_patch call, whose patch
# names the files. Codex reads through shell commands, so its reads are not seen.
#
#   agent_follow_hook.sh <claude|codex>          after a tool call
#   agent_follow_hook.sh <claude|codex> pre      before one: the project is
#                                                snapshot (agent_changes.sh)
#   agent_follow_hook.sh <claude|codex> prompt   the user's new prompt: the
#                                                marks of the last turn go
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

first_line() {
  awk 'NF { print; exit }'
}

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
  latest="$(cut -f1 <<<"$ranges" | sort -u | while IFS= read -r changed_file; do
    [ -e "$changed_file" ] && printf '%s\t%s\n' "$(stat -f %Fm "$changed_file")" "$changed_file"
  done | sort -rn | head -n 1 | cut -f2-)"
  if [ -n "$latest" ]; then
    first="$(awk -F '\t' -v file="$latest" '$1 == file { print $2; exit }' <<<"$ranges")"
    printf '%s\n' "$ranges" | "$open" --follow-changes "$latest:$first"
    exit 0
  fi
fi

file=""
text=""
event=--follow
case "$agent" in
  claude)
    file="$(json '.tool_input.file_path // .tool_input.notebook_path // empty')"
    if [ "$(json '.tool_name // empty')" = "Read" ]; then
      event=--follow-read
      offset="$(json '.tool_input.offset // empty')"
      [ -z "$file" ] || [ -z "$offset" ] || file="$file:$offset"
    else
      # The patch Claude Code reports says where the change is; searching the
      # file for the new text would stop at an earlier line that reads the
      # same (an "end", a "}"), so the text is only the fallback.
      line="$(json '.tool_response.structuredPatch[0] // empty
        | .newStart + (.lines | map(startswith("+") or startswith("-")) | index(true) // 0)')"
      if [ -n "$file" ] && [ -n "$line" ]; then
        file="$file:$line"
      else
        text="$(json '.tool_input.new_string // .tool_input.edits[0].new_string // .tool_input.content // empty' | first_line)"
      fi
    fi
    ;;
  codex)
    # The patch is a string somewhere in the tool input; the last file it
    # changes is shown, at the first line it adds there.
    patch="$(json '[.. | strings | select(contains("*** Begin Patch"))][0] // empty')"
    file="$(printf '%s\n' "$patch" |
      sed -En -e 's/^\*\*\* (Update|Add) File: //p' -e 's/^\*\*\* Move to: //p' | tail -n 1)"
    text="$(printf '%s\n' "$patch" | awk -v file="$file" '
      /^\*\*\* (Update|Add) File: / || /^\*\*\* Move to: / { current = $0; sub(/^\*\*\* [^:]*: /, "", current) }
      current == file && /^\+/ && length($0) > 1 { print substr($0, 2); exit }')"
    cwd="$(json '.cwd // empty')"
    [ -n "$cwd" ] && cd "$cwd" 2>/dev/null
    ;;
esac

[ -n "$file" ] && "$open" "$event" "$file" "$text"
exit 0
