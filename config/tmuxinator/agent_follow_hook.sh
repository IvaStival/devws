#!/usr/bin/env bash
# The agents' edit hook: shows the file the agent just changed in the editor,
# at the change. Claude Code passes the edit's JSON (file_path, new_string or
# content); Codex passes its apply_patch call, whose patch names the files.
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
case "$agent" in
  claude)
    file="$(json '.tool_input.file_path // .tool_input.notebook_path // empty')"
    text="$(json '.tool_input.new_string // .tool_input.edits[0].new_string // .tool_input.content // empty' | first_line)"
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

[ -n "$file" ] && "$open" --follow "$file" "$text"
exit 0
