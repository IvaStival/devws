#!/usr/bin/env bash
# The agents' hook: shows the file the agent just changed in the editor, at
# the change, or a file it read (shown only in the "follow edits and reads"
# mode). Claude Code passes the tool call's JSON (file_path, new_string or
# content, a Read's offset); Codex passes its apply_patch call, whose patch
# names the files. Codex reads through shell commands, so its reads are not seen.
#
#   agent_follow_hook.sh <claude|codex>   (hook JSON on stdin)
#
# It always exits 0: a hook that fails would interrupt the agent.

set -u

agent="${1:-}"
payload="$(cat)"
open="$(dirname "$0")/editor_open.sh"

first_line() {
  awk 'NF { print; exit }'
}

json() {
  printf '%s' "$payload" | jq -r "$1" 2>/dev/null
}

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
      text="$(json '.tool_input.new_string // .tool_input.edits[0].new_string // .tool_input.content // empty' | first_line)"
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
