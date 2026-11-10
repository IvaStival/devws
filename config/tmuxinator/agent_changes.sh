#!/usr/bin/env bash
# Finds what an agent's tool call changed in a git project, whatever the tool
# (an edit, a shell command, a formatter): the project's files are snapshot
# before the call and compared after it. The snapshots go into an index of
# devws's own, a copy of the project's, so the project's index, staging and
# history are never touched; ignored files are left out. Outside a git project
# nothing is found.
#
#   agent_changes.sh snapshot <dir> <name>   before the call
#   agent_changes.sh changes <dir> <name>    after it: one line per changed
#                                            range, "<file>\t<first>\t<last>"
#                                            (absolute path, line numbers of
#                                            the file now); a deletion is the
#                                            line where it was
#
# <name> keeps one caller's snapshot apart from another's (an agent pane).
# `discard` removes an obsolete snapshot without inspecting the repository.
# It always exits 0: the agents' hooks call it.

set -u

action="${1:-}"
dir="${2:-}"
name="$(printf '%s' "${3:-default}" | tr -c 'A-Za-z0-9_.-' '_')"
state_dir="${TMPDIR:-/tmp}"
state_dir="${state_dir%/}/devws-changes-$(id -u)"
before="$state_dir/$name.before"
index="$state_dir/$name.index"

if [ "$action" = discard ]; then
  rm -f "$before" "$index"
  exit 0
fi
[ "$action" != changes ] || [ -f "$before" ] || exit 0

top="$(git -C "$dir" rev-parse --show-toplevel 2>/dev/null)" || exit 0
git_dir="$(git -C "$top" rev-parse --absolute-git-dir 2>/dev/null)" || exit 0

# The tree of the project's files as they are now. The index copy starts from
# the project's, so git only rehashes the files that changed since.
tree_now() {
  if [ -f "$git_dir/index" ]; then
    cp "$git_dir/index" "$index" || return 1
  else
    rm -f "$index"
  fi
  GIT_INDEX_FILE="$index" git -C "$top" add -A >/dev/null 2>&1 &&
    GIT_INDEX_FILE="$index" git -C "$top" write-tree 2>/dev/null
}

case "$action" in
  snapshot)
    mkdir -p "$state_dir"
    rm -f "$before"
    tree="$(tree_now)" && [ -n "$tree" ] && printf '%s %s\n' "$top" "$tree" > "$before"
    ;;
  changes)
    [ -f "$before" ] || exit 0
    read -r snapshot_top snapshot_tree < "$before"
    rm -f "$before"
    [ "$snapshot_top" = "$top" ] || exit 0
    tree="$(tree_now)" || exit 0
    rm -f "$index"
    [ "$tree" != "$snapshot_tree" ] || exit 0
    git -C "$top" -c core.quotePath=false diff -U0 --no-color --no-renames --no-ext-diff \
      "$snapshot_tree" "$tree" 2>/dev/null |
      awk -v top="$top" '
        # The file as it is now; a deleted file has none.
        /^\+\+\+ / {
          file = substr($0, 5)
          sub(/\t$/, "", file)
          file = (file == "/dev/null") ? "" : top "/" substr(file, 3)
          next
        }
        /^@@ / && file != "" {
          split($3, now, ",")
          first = substr(now[1], 2) + 0
          count = (2 in now) ? now[2] + 0 : 1
          if (count == 0) { if (first < 1) first = 1; last = first }
          else last = first + count - 1
          printf "%s\t%d\t%d\n", file, first, last
        }'
    ;;
esac
exit 0
