#!/usr/bin/env bash
# Tidy up the branch folders under DEVWS_HOME that are done with: their PR is
# merged or closed, they have no uncommitted changes, and no workspace is open
# on them (Tidy up in the menu, `devws tidy`). Only folders go: the branches
# and their commits stay, and git refuses a folder with changes.
#
#   tidy.sh                     lists them, all ticked (Tab unticks one), and
#                               removes the ticked ones as a background job
#   tidy.sh candidates          "<repo  branch  PR state><TAB><folder>" per
#                               folder to remove; the PR states are asked anew
#   tidy.sh remove <folder>...  removes them; fails when one is kept

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"

open_roots() {
  tmux list-sessions -F '#{@devws_root}' 2>/dev/null
}

candidates() {
  local home bare folder branch state open
  home="$("$script_dir/github_repo.sh" home)"
  open="$(open_roots)"
  for bare in "$home"/*/*/.bare; do
    [ -d "$bare" ] || continue
    while IFS=$'\t' read -r branch folder; do
      [ -n "$folder" ] || continue
      grep -qxF "$folder" <<<"$open" && continue
      [ -z "$(git --no-optional-locks -C "$folder" status --porcelain 2>/dev/null)" ] || continue
      state="$("$script_dir/github_repo.sh" pr-query "$folder" 2>/dev/null)" || continue
      case "$state" in
        merged | closed)
          printf '%-18s %-28s PR %s\t%s\n' "$(basename "$(dirname "$bare")")" "$branch" "$state" "$folder"
          ;;
      esac
    done < <("$script_dir/github_repo.sh" worktrees "$bare")
  done
}

remove() {
  local folder kept=0
  for folder in "$@"; do
    if "$script_dir/worktree.sh" remove "$folder"; then
      printf 'Removed %s\n' "$folder"
    else
      kept=1
    fi
  done
  return "$kept"
}

ask_and_remove() {
  local rows choice
  local -a folders=()
  printf 'Looking for branch folders that are done with…\n'
  rows="$(candidates)"
  if [ -z "$rows" ]; then
    printf 'Nothing to tidy: no branch folder has a merged or closed PR and no changes.\n'
    return 0
  fi
  while IFS= read -r choice; do
    [ -n "$choice" ] && folders+=("${choice##*$'\t'}")
  done < <(
    "$script_dir/picker.sh" Tidy 'Branch folders to remove (PR merged or closed, no changes; branches are kept)' \
      --multi --bind 'load:select-all' --delimiter=$'\t' --with-nth=1 <<<"$rows"
  )
  [ "${#folders[@]}" -gt 0 ] || return 0
  "$script_dir/job.sh" start "Tidy up ${#folders[@]} folder$([ "${#folders[@]}" -eq 1 ] || printf s)" -- \
    "$script_dir/tidy.sh" remove "${folders[@]}" >/dev/null
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  '') ask_and_remove ;;
  candidates) candidates ;;
  remove) [ "$#" -gt 0 ] || { printf 'usage: tidy.sh remove <folder>...\n' >&2; exit 2; }; remove "$@" ;;
  *) printf 'usage: tidy.sh <candidates|remove> …\n' >&2; exit 2 ;;
esac
