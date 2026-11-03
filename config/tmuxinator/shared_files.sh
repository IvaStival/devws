#!/usr/bin/env bash
# The files git ignores that a branch's folder still needs to run (.env,
# deploy/.docker-env, a SQLite database), kept once per repo in
# <owner>/<repo>/.shared with the repo's own layout, and put into each of its
# worktrees (github_repo.sh, github_workspace.sh).
#
#   shared_files.sh candidates <owner/repo>      checkouts of the repo in
#                                                DEVWS_PROJECTS (default
#                                                ~/Projects), to import from
#   shared_files.sh ignored <folder>             the folder's git-ignored
#                                                files, minus packages,
#                                                caches, builds and logs
#   shared_files.sh seed <repo dir> <folder> <file>...
#                                                copies the files into .shared
#   shared_files.sh link <repo dir> <worktree>   puts .shared into the
#                                                worktree: a link per file, so
#                                                one edit reaches every branch,
#                                                except databases (*.sqlite,
#                                                *.sqlite3, *.db), copied so
#                                                each branch has its own data
#
# Nothing here replaces a file that is already there.

set -u

die() {
  printf 'devws shared files: %s\n' "$1" >&2
  exit 2
}

script_dir="$(cd "$(dirname "$0")" && pwd)"
# The environment wins over .env, as in github_repo.sh.
from_environment="${DEVWS_PROJECTS:-}"
# shellcheck source=/dev/null
[ -r "$script_dir/.env" ] && . "$script_dir/.env"
DEVWS_PROJECTS="${from_environment:-${DEVWS_PROJECTS:-}}"

projects_folder() {
  local folder="${DEVWS_PROJECTS:-~/Projects}"
  case "$folder" in
    "~") folder="$HOME" ;;
    "~/"*) folder="$HOME/${folder#\~/}" ;;
  esac
  printf '%s' "${folder%/}"
}

candidates() {
  local wanted folder origin
  wanted="$("$script_dir/github_repo.sh" parse "$1")" || exit 2
  for folder in "$(projects_folder)"/*/; do
    folder="${folder%/}"
    origin="$(git -C "$folder" remote get-url origin 2>/dev/null)" || continue
    [ "$("$script_dir/github_repo.sh" parse "$origin" 2>/dev/null)" = "$wanted" ] &&
      printf '%s\n' "$folder"
  done
  return 0
}

# Directories git ignores whole are listed once (--directory), so vendor/ is
# one line rather than thousands.
ignored() {
  git -C "$1" ls-files --others --ignored --exclude-standard --directory 2>/dev/null |
    grep -Ev '(^|/)(vendor|node_modules|\.idea|\.vscode|\.next|dist|build|target|coverage|\.cache|__pycache__|\.venv|storage|bootstrap/cache)(/|$)' |
    grep -Ev '(^|/)(\.DS_Store|next-env\.d\.ts)$|\.(log|cache|tsbuildinfo)$|/$'
}

# A path inside the folder: no absolute path, no "..".
inside() {
  case "/$1/" in
    //* | */../* | */./*) return 1 ;;
  esac
}

seed() {
  local repo_dir="$1" folder="$2" file
  shift 2
  for file in "$@"; do
    inside "$file" || die "not a path inside $folder: $file"
    [ -e "$folder/$file" ] || die "not found: $folder/$file"
    mkdir -p "$repo_dir/.shared/$(dirname "$file")" || die "cannot create $repo_dir/.shared"
    cp -p "$folder/$file" "$repo_dir/.shared/$file" || die "cannot copy $file"
  done
}

link() {
  local shared="$1/.shared" worktree="$2" file target
  [ -d "$shared" ] || return 0
  (cd "$shared" && find . -type f ! -name .DS_Store) | while IFS= read -r file; do
    file="${file#./}"
    target="$worktree/$file"
    [ -e "$target" ] || [ -L "$target" ] && continue
    mkdir -p "$(dirname "$target")" || die "cannot create $(dirname "$target")"
    case "$file" in
      *.sqlite | *.sqlite3 | *.db) cp -p "$shared/$file" "$target" ;;
      *) ln -s "$shared/$file" "$target" ;;
    esac || die "cannot put $file in $worktree"
  done
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  candidates) [ -n "${1:-}" ] || die "no repo given"; candidates "$1" ;;
  ignored) [ -d "${1:-}" ] || die "no folder given"; ignored "$1" ;;
  seed) [ "$#" -ge 2 ] || die "usage: shared_files.sh seed <repo dir> <folder> <file>..."; seed "$@" ;;
  link) [ "$#" -eq 2 ] || die "usage: shared_files.sh link <repo dir> <worktree>"; link "$1" "$2" ;;
  *) die "usage: shared_files.sh <candidates|ignored|seed|link> …" ;;
esac
