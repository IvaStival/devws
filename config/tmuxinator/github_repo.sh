#!/usr/bin/env bash
# GitHub repos kept by devws: one hidden bare clone per repo under DEVWS_HOME
# (.env, default ~/.devws), whose worktrees sit beside it (worktree.sh):
#
#   ~/.devws/<owner>/<repo>/.bare               the clone, fetched on every start
#   ~/.devws/<owner>/<repo>/<repo>-<branch>     a worktree per task
#
#   github_repo.sh home                  the root folder
#   github_repo.sh parse <repo>          owner/repo, from owner/repo or a GitHub
#                                        https or ssh URL
#   github_repo.sh list [--refresh]      the user's repos and their orgs', the
#                                        cloned ones first; gh's answer is kept
#                                        for an hour
#   github_repo.sh ensure <repo>         clones the repo, or fetches it; prints
#                                        its bare repo's folder
#   github_repo.sh default-branch <dir>  origin's default branch
#   github_repo.sh worktrees <dir>       "<branch><TAB><folder>" per worktree
#   github_repo.sh pr <dir>              the branch's PR state as last checked:
#                                        none, draft, open, merged or closed;
#                                        empty when never checked
#   github_repo.sh pr-query <dir>        asks GitHub now, keeps and prints it
#   github_repo.sh pr-refresh <dir>...   asks again for the states older than
#                                        five minutes; succeeds when one changed
#
# Clones come from https://github.com/ or git@github.com: as gh's git_protocol
# says; DEVWS_CLONE_BASE replaces that (tests, GitHub Enterprise). The PR state
# is kept in the worktree's own git folder, so it goes with the worktree.

set -u

die() {
  printf 'devws github: %s\n' "$1" >&2
  exit 2
}

# The environment wins over .env, so one shell (or a test) can use another root.
from_environment="${DEVWS_HOME:-}"
config="$(cd "$(dirname "$0")" && pwd)/.env"
# shellcheck source=/dev/null
[ -r "$config" ] && . "$config"
DEVWS_HOME="${from_environment:-${DEVWS_HOME:-}}"

list_max_age=3600
pr_max_age=300

home() {
  local folder="${DEVWS_HOME:-~/.devws}"
  case "$folder" in
    "~") folder="$HOME" ;;
    "~/"*) folder="$HOME/${folder#\~/}" ;;
  esac
  printf '%s' "${folder%/}"
}

parse() {
  local name="$1"
  name="${name#https://github.com/}"
  name="${name#ssh://git@github.com/}"
  name="${name#git@github.com:}"
  name="${name%/}"
  name="${name%.git}"
  [[ "$name" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || die "not a GitHub repo: $1"
  case "/$name/" in */./* | */../*) die "not a GitHub repo: $1" ;; esac
  printf '%s\n' "$name"
}

clone_url() {
  local base="${DEVWS_CLONE_BASE:-}"
  if [ -z "$base" ]; then
    if [ "$(gh config get git_protocol 2>/dev/null)" = ssh ]; then
      base="git@github.com:"
    else
      base="https://github.com/"
    fi
  fi
  printf '%s%s.git' "$base" "$1"
}

now() {
  date +%s
}

cloned() {
  local root bare
  root="$(home)"
  for bare in "$root"/*/*/.bare; do
    [ -d "$bare" ] || continue
    bare="${bare%/.bare}"
    printf '%s\n' "${bare#"$root"/}"
  done
}

ask_github_for_repos() {
  local org
  gh repo list --limit 1000 --json nameWithOwner -q '.[].nameWithOwner' || return 1
  for org in $(gh api user/orgs -q '.[].login'); do
    gh repo list "$org" --limit 1000 --json nameWithOwner -q '.[].nameWithOwner' || return 1
  done
}

list() {
  local cache fetched_at="" temporary
  cache="$(home)/.cache/repos"
  [ -r "$cache" ] && fetched_at="$(head -n 1 "$cache")"
  case "$fetched_at" in ''|*[!0-9]*) fetched_at=0 ;; esac

  if [ "${1:-}" = --refresh ] || [ $(($(now) - fetched_at)) -ge "$list_max_age" ]; then
    mkdir -p "$(dirname "$cache")" || die "cannot create $(dirname "$cache")"
    temporary="$(mktemp "$cache.XXXXXX")" || die "cannot write $cache"
    if { now; ask_github_for_repos | LC_ALL=C sort -u; } > "$temporary"; then
      mv "$temporary" "$cache"
    else
      rm -f "$temporary"
      # An old list beats none while GitHub cannot be reached.
      [ -r "$cache" ] || die "gh could not list your repos (is it logged in? gh auth login)"
      printf 'devws github: gh could not list your repos; showing the last list\n' >&2
    fi
  fi

  { cloned | LC_ALL=C sort; tail -n +2 "$cache"; } | awk 'NF && !seen[$0]++'
}

ensure() {
  local name bare created=""
  name="$(parse "$1")" || exit 2
  bare="$(home)/$name/.bare"
  if [ ! -d "$bare" ]; then
    created=1
    git init -q --bare "$bare" || die "cannot create $bare"
    git -C "$bare" remote add origin "$(clone_url "$name")"
    # A bare clone has no remote-tracking branches unless told to keep them,
    # and worktrees branch off origin/<default>.
    git -C "$bare" config remote.origin.fetch '+refs/heads/*:refs/remotes/origin/*'
  fi
  printf 'Fetching %s…\n' "$name" >&2
  if ! git -C "$bare" fetch --prune origin >&2; then
    # Only a folder this call made is removed: never an existing clone.
    if [ -n "$created" ]; then
      rm -rf "$bare"
      rmdir "$(dirname "$bare")" "$(dirname "$(dirname "$bare")")" 2>/dev/null
    fi
    die "git could not fetch $name"
  fi
  git -C "$bare" remote set-head origin --auto >/dev/null 2>&1
  printf '%s\n' "$bare"
}

default_branch() {
  local head
  if head="$(git -C "$1" symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null)"; then
    printf '%s\n' "${head#origin/}"
  elif git -C "$1" show-ref --verify --quiet refs/remotes/origin/main; then
    printf 'main\n'
  else
    printf 'master\n'
  fi
}

worktrees() {
  git -C "$1" worktree list --porcelain | awk '
    /^worktree / { folder = substr($0, 10) }
    /^branch refs\/heads\// { printf "%s\t%s\n", substr($0, 19), folder }'
}

pr_file() {
  local git_dir
  git_dir="$(git -C "$1" rev-parse --path-format=absolute --git-dir 2>/dev/null)" || return 1
  printf '%s/devws-pr' "$git_dir"
}

pr() {
  local file checked state
  file="$(pr_file "$1")" || return 0
  [ -r "$file" ] || return 0
  read -r checked state < "$file"
  printf '%s\n' "${state:-}"
}

pr_query() {
  local file state errors
  file="$(pr_file "$1")" || die "not a git folder: $1"
  errors="$(mktemp "${TMPDIR:-/tmp}/devws-pr.XXXXXX")" || exit 2
  if state="$(cd "$1" && gh pr view --json state,isDraft \
      -q 'if .isDraft then "draft" else (.state | ascii_downcase) end' 2>"$errors")"; then
    :
  elif grep -q 'no pull requests found' "$errors"; then
    state=none
  else
    # Offline or logged out: the last known state stays.
    cat "$errors" >&2
    rm -f "$errors"
    return 1
  fi
  rm -f "$errors"
  printf '%s %s\n' "$(now)" "$state" > "$file"
  printf '%s\n' "$state"
}

pr_refresh() {
  local folder file checked state changed=1
  command -v gh >/dev/null 2>&1 || return 1
  for folder in "$@"; do
    file="$(pr_file "$folder")" || continue
    checked=0 state=""
    [ -r "$file" ] && read -r checked state < "$file"
    case "$checked" in ''|*[!0-9]*) checked=0 ;; esac
    [ $(($(now) - checked)) -ge "$pr_max_age" ] || continue
    # Only GitHub repos (or the clones' own base) have PRs to ask about;
    # asking about others would fail on every redraw, as nothing is kept.
    case "$(git -C "$folder" remote get-url origin 2>/dev/null)" in
      *github.com*) ;;
      "${DEVWS_CLONE_BASE:-}"?*) [ -n "${DEVWS_CLONE_BASE:-}" ] || continue ;;
      *) continue ;;
    esac
    [ "$(pr_query "$folder" 2>/dev/null)" = "$state" ] || changed=0
  done
  return "$changed"
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  home) home; printf '\n' ;;
  parse) [ -n "${1:-}" ] || die "no repo given"; parse "$1" ;;
  list) list "${1:-}" ;;
  ensure) [ -n "${1:-}" ] || die "no repo given"; ensure "$1" ;;
  default-branch) [ -n "${1:-}" ] || die "no folder given"; default_branch "$1" ;;
  worktrees) [ -n "${1:-}" ] || die "no folder given"; worktrees "$1" ;;
  pr) [ -n "${1:-}" ] || die "no folder given"; pr "$1" ;;
  pr-query) [ -n "${1:-}" ] || die "no folder given"; pr_query "$1" ;;
  pr-refresh) pr_refresh "$@" ;;
  *) die "usage: github_repo.sh <home|parse|list|ensure|default-branch|worktrees|pr|pr-query|pr-refresh> …" ;;
esac
