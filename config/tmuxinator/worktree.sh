#!/usr/bin/env bash
# Git worktrees for devws workspaces: a branch checked out in a folder of its
# own next to the repo, so two agents can work on one repo without editing
# each other's files.
#
#   worktree.sh path <dir> <branch>     the folder for <branch>:
#                                       <parent of the repo>/<repo>-<branch>,
#                                       "/" in the branch turned into "-"; for
#                                       a devws-managed repo (a bare one in
#                                       <owner>/<repo>/.bare, github_repo.sh)
#                                       <owner>/<repo>/<repo>-<branch>
#   worktree.sh ensure <dir> <branch> [base]
#                                       the worktree for <branch>, created when
#                                       there is none; prints its folder
#   worktree.sh linked <dir>            succeeds when <dir> is a worktree of
#                                       its own (not the repo's main folder)
#   worktree.sh branch <dir>            the branch checked out in <dir>
#   worktree.sh status <dir>            "clean", or git status --short's lines
#   worktree.sh remove <dir>            removes the worktree's folder
#
# <dir> is any folder in the repo or one of its worktrees. ensure checks out
# an existing local branch, else a remote one (tracking it), else branches a
# new one off [base] (default: <dir>'s HEAD) without tracking it, so a plain
# push never goes to the base. Nothing here deletes a branch or a commit, and
# remove never forces: git refuses a worktree with changes.

set -u

die() {
  printf 'devws worktree: %s\n' "$1" >&2
  exit 2
}

in_repo() {
  git -C "$1" rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not in a git repo: $1"
}

# The repo's common git dir, whichever of its worktrees <dir> is in.
common_dir() {
  git -C "$1" rev-parse --path-format=absolute --git-common-dir
}

folder_for() {
  local common main
  common="$(common_dir "$1")" || return 1
  main="$(dirname "$common")"
  # A managed repo has no main folder: its worktrees go inside the folder
  # that holds the bare repo, not next to it.
  if [ "$(basename "$common")" = .bare ]; then
    printf '%s/%s-%s' "$main" "$(basename "$main")" "${2//\//-}"
  else
    printf '%s/%s-%s' "$(dirname "$main")" "$(basename "$main")" "${2//\//-}"
  fi
}

# The folder of the worktree that has <branch> checked out, if any.
worktree_of() {
  git -C "$1" worktree list --porcelain | awk -v ref="refs/heads/$2" '
    /^worktree / { folder = substr($0, 10) }
    $0 == "branch " ref { print folder; exit }'
}

ensure() {
  local dir="$1" branch="$2" base="${3:-HEAD}" folder existing
  git check-ref-format --branch "$branch" >/dev/null 2>&1 || die "not a valid branch name: $branch"
  existing="$(worktree_of "$dir" "$branch")"
  if [ -n "$existing" ]; then
    printf '%s\n' "$existing"
    return 0
  fi
  folder="$(folder_for "$dir" "$branch")" || die "cannot find the repo of $dir"
  [ ! -e "$folder" ] || die "$folder already exists and is not the worktree of $branch"
  if git -C "$dir" show-ref --verify --quiet "refs/heads/$branch"; then
    git -C "$dir" worktree add "$folder" "$branch" >&2 || die "git could not check out $branch"
  elif git -C "$dir" for-each-ref --format='%(refname:lstrip=3)' refs/remotes | grep -qxF "$branch"; then
    # git creates the local branch tracking the remote one.
    git -C "$dir" worktree add "$folder" "$branch" >&2 || die "git could not check out $branch"
  else
    git -C "$dir" worktree add --no-track -b "$branch" "$folder" "$base" >&2 || die "git could not create $branch"
  fi
  printf '%s\n' "$folder"
}

# A worktree of its own has a git folder apart from the repo's common one.
linked() {
  local git_dir common
  git_dir="$(git -C "$1" rev-parse --path-format=absolute --git-dir 2>/dev/null)" || return 1
  common="$(git -C "$1" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || return 1
  [ "$git_dir" != "$common" ]
}

status() {
  local changes
  changes="$(git -C "$1" status --short --untracked-files=all)" || die "cannot read the status of $1"
  if [ -z "$changes" ]; then
    printf 'clean\n'
  else
    printf '%s\n' "$changes"
  fi
}

remove() {
  linked "$1" || die "$1 is not a worktree of its own; it is kept"
  # From the common git dir: a managed repo has no main folder to run in.
  git -C "$(common_dir "$1")" worktree remove "$1" || die "git kept $1"
}

command="${1:-}"
dir="${2:-}"
[ -n "$dir" ] || die "usage: worktree.sh <path|ensure|linked|branch|status|remove> <dir> [branch]"
case "$command" in
  path) in_repo "$dir"; [ -n "${3:-}" ] || die "no branch given"; folder_for "$dir" "$3"; printf '\n' ;;
  ensure) in_repo "$dir"; [ -n "${3:-}" ] || die "no branch given"; ensure "$dir" "$3" "${4:-}" ;;
  linked) linked "$dir" ;;
  branch) in_repo "$dir"; git -C "$dir" symbolic-ref --quiet --short HEAD ;;
  status) in_repo "$dir"; status "$dir" ;;
  remove) in_repo "$dir"; remove "$dir" ;;
  *) die "unknown command: $command" ;;
esac
