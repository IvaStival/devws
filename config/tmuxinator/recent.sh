#!/usr/bin/env bash
# The folders devws opened, newest first, with the agent and editor each had:
# `devws` with no folder lists them to reopen (github_workspace.sh), and
# reopening one asks nothing. Kept in ~/.devws-state/recent.tsv as
# "<folder><TAB><when><TAB><agent><TAB><editor>".
#
#   recent.sh add <folder> <agent> <editor>   records an opening
#   recent.sh get <folder>                    "<agent> <editor>", or nothing
#   recent.sh rows                            "<row><TAB><folder>" to choose
#                                             from: repo, branch, clean or
#                                             changed, PR state, when; then
#                                             the managed worktrees (under
#                                             DEVWS_HOME) never opened
#
# Folders that no longer exist are left out, and forgotten at the next add.

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
recent_file="$HOME/.devws-state/recent.tsv"

die() {
  printf 'devws recent: %s\n' "$1" >&2
  exit 2
}

add() {
  local folder="$1" agent="$2" editor="$3" temporary
  mkdir -p "$(dirname "$recent_file")" || die "cannot create $(dirname "$recent_file")"
  temporary="$(mktemp "$recent_file.XXXXXX")" || die "cannot write $recent_file"
  {
    printf '%s\t%s\t%s\t%s\n' "$folder" "$(date +%s)" "$agent" "$editor"
    [ -r "$recent_file" ] && awk -F '\t' -v folder="$folder" '$1 != folder' "$recent_file" |
      while IFS=$'\t' read -r path rest; do
        [ -d "$path" ] && printf '%s\t%s\n' "$path" "$rest"
      done
  } > "$temporary"
  mv "$temporary" "$recent_file"
}

get() {
  [ -r "$recent_file" ] || return 0
  awk -F '\t' -v folder="$1" '$1 == folder && $3 != "" { print $3 " " $4; exit }' "$recent_file"
}

ago() {
  local seconds=$(($(date +%s) - $1))
  if [ "$seconds" -lt 3600 ]; then
    printf 'just now'
  elif [ "$seconds" -lt 86400 ]; then
    printf '%dh ago' $((seconds / 3600))
  elif [ "$seconds" -lt 172800 ]; then
    printf 'yesterday'
  elif [ "$seconds" -lt 1209600 ]; then
    printf '%dd ago' $((seconds / 86400))
  else
    printf '%dw ago' $((seconds / 604800))
  fi
}

# repo, branch, clean or changed (and the PR's state), when.
row() {
  local folder="$1" when="$2" repo branch state pr common
  common="$(git -C "$folder" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"
  if [ "$(basename "$common")" = .bare ]; then
    repo="$(basename "$(dirname "$common")")"
  else
    repo="$(basename "$(git -C "$folder" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$folder")")"
  fi
  branch="$(git -C "$folder" branch --show-current 2>/dev/null)"
  # Only a look: it takes no index lock another git command could meet.
  if [ -z "$(git --no-optional-locks -C "$folder" status --porcelain 2>/dev/null)" ]; then
    state="✓ clean"
  else
    state="● changes"
  fi
  pr="$("$script_dir/github_repo.sh" pr "$folder" 2>/dev/null)"
  case "$pr" in '' | none) ;; *) state="$state · PR $pr" ;; esac
  printf '%-18s %-28s %-24s %s\t%s\n' "$repo" "${branch:-—}" "$state" "$when" "$folder"
}

managed_worktrees() {
  local home bare
  home="$("$script_dir/github_repo.sh" home)"
  for bare in "$home"/*/*/.bare; do
    [ -d "$bare" ] || continue
    "$script_dir/github_repo.sh" worktrees "$bare" | cut -f2
  done
}

rows() {
  local folder when agent editor listed=$'\n'
  if [ -r "$recent_file" ]; then
    while IFS=$'\t' read -r folder when agent editor; do
      [ -d "$folder" ] || continue
      listed+="$folder"$'\n'
      row "$folder" "$(ago "$when")"
    done < "$recent_file"
  fi
  while IFS= read -r folder; do
    [ -n "$folder" ] && [ -d "$folder" ] || continue
    case "$listed" in *$'\n'"$folder"$'\n'*) continue ;; esac
    row "$folder" "never opened"
  done < <(managed_worktrees)
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  add) [ "$#" -eq 3 ] || die "usage: recent.sh add <folder> <agent> <editor>"; add "$@" ;;
  get) [ -n "${1:-}" ] || die "no folder given"; get "$1" ;;
  rows) rows ;;
  *) die "usage: recent.sh <add|get|rows> …" ;;
esac
