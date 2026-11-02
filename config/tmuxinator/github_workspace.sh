#!/usr/bin/env bash
# The folder for a new GitHub workspace: asks for the repo, clones or fetches
# it (github_repo.sh), then for one of its worktrees or a new branch off a
# fresh origin/<default branch> (worktree.sh). The questions use the terminal;
# only the folder is printed. Run by `devws` with no folder and by the menu's
# "New GitHub workspace".
#
#   github_workspace.sh [repo]                 the folder; asks for the repo
#                                              unless one is given
#   github_workspace.sh branch-name <prefix> <description>
#                                              <prefix>/<description as a slug>

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
github_repo="$script_dir/github_repo.sh"
worktree="$script_dir/worktree.sh"
shared_files="$script_dir/shared_files.sh"
prefixes='feat fix chore refactor docs'
custom_name='Custom name…'

# "Login page: OAuth & SSO!" → login-page-oauth-sso
branch_name() {
  local slug
  slug="$(printf '%s' "$2" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-')"
  slug="${slug#-}"
  slug="${slug%-}"
  [ -n "$slug" ] || return 1
  printf '%s/%s\n' "$1" "$slug"
}

pick_repo() {
  local refresh_row='↻ Refresh the list' other_row='＋ Clone URL / owner/repo…'
  local refresh="" choice
  while :; do
    choice="$(
      { printf '%s\n' "$other_row" "$refresh_row"; "$github_repo" list ${refresh:+--refresh}; } |
        fzf --height=~50% --layout=reverse --prompt='Repo > ' \
          --header='GitHub repo for the workspace (Esc: cancel)'
    )" || return 1
    case "$choice" in
      "$refresh_row") refresh=1 ;;
      "$other_row")
        read -e -r -p 'Repo (owner/repo or URL): ' choice || return 1
        [ -n "$choice" ] || return 1
        printf '%s\n' "$choice"
        return 0
        ;;
      *)
        printf '%s\n' "$choice"
        return 0
        ;;
    esac
  done
}

# An existing worktree's folder, or nothing for a new branch.
pick_worktree() {
  local bare="$1" new_row='+ New branch' rows branch folder changes state choice
  rows="$(
    "$github_repo" worktrees "$bare" | while IFS=$'\t' read -r branch folder; do
      changes="✓ clean"
      [ "$("$worktree" status "$folder" 2>/dev/null)" = clean ] || changes="● changes"
      state="$("$github_repo" pr "$folder")"
      printf '%s\t%s%s\t%s\n' "$branch" "$changes" "${state:+ · PR $state}" "$folder"
    done
  )"
  [ -n "$rows" ] || return 0
  choice="$(
    { printf '%s\t\t\n' "$new_row"; printf '%s\n' "$rows"; } |
      fzf --height=~50% --layout=reverse --prompt='Worktree > ' \
        --delimiter=$'\t' --with-nth=1,2 --tabstop=32 \
        --header='Continue a worktree, or start a new branch (Esc: cancel)'
  )" || return 1
  printf '%s\n' "${choice##*$'\t'}"
}

# The first time: which folder's git-ignored files (.env…) the repo's branches
# get (shared_files.sh). "Skip" settles it with an empty .shared; Esc asks
# again next time.
seed_shared() {
  local repo_dir="$1" name="$2" other_row='Type a path…' skip_row='Skip' source file
  local -a files=()
  [ -d "$repo_dir/.shared" ] && return 0
  source="$(
    { "$shared_files" candidates "$name"; printf '%s\n' "$other_row" "$skip_row"; } |
      fzf --height=~40% --layout=reverse --prompt='Import from > ' \
        --header="Copy $name's untracked files (.env, .docker-env…) from a checkout you have"
  )" || return 0
  case "$source" in
    "$skip_row")
      mkdir -p "$repo_dir/.shared"
      return 0
      ;;
    "$other_row")
      read -e -r -p 'Folder: ' source || return 0
      case "$source" in "~/"*) source="$HOME/${source#\~/}" ;; esac
      ;;
  esac
  [ -d "$source" ] || { printf 'Not a folder: %s\n' "$source" >&2; return 0; }

  while IFS= read -r file; do
    [ -n "$file" ] && files+=("$file")
  done < <(
    "$shared_files" ignored "$source" |
      fzf --multi --height=~60% --layout=reverse --bind 'load:select-all' --prompt='Files > ' \
        --header='Files every branch gets (Tab unticks one, Enter copies)'
  )
  mkdir -p "$repo_dir/.shared"
  [ "${#files[@]}" -eq 0 ] || "$shared_files" seed "$repo_dir" "$source" "${files[@]}"
}

ask_branch() {
  local prefix description branch
  prefix="$(
    printf '%s\n' $prefixes "$custom_name" |
      fzf --height=~10 --layout=reverse --prompt='Kind > ' \
        --header='What the branch is for (Esc: cancel)'
  )" || return 1
  while :; do
    if [ "$prefix" = "$custom_name" ]; then
      read -e -r -p 'Branch name: ' branch || return 1
    else
      read -e -r -p "$prefix/ (a few words): " description || return 1
      branch="$(branch_name "$prefix" "$description")" || {
        printf 'Type a few words to name the branch.\n' >&2
        continue
      }
    fi
    [ -n "$branch" ] || return 1
    git check-ref-format --branch "$branch" >/dev/null 2>&1 && break
    printf 'Not a valid branch name: %s\n' "$branch" >&2
  done
  printf '%s\n' "$branch"
}

main() {
  local repo="${1:-}" bare folder branch base
  if [ -z "$repo" ]; then
    repo="$(pick_repo)" || return 1
  fi
  # A failure (exit 2) is told apart from a cancelled question (1).
  bare="$("$github_repo" ensure "$repo")" || return
  seed_shared "$(dirname "$bare")" "$("$github_repo" parse "$repo")"

  folder="$(pick_worktree "$bare")" || return 1
  if [ -z "$folder" ]; then
    branch="$(ask_branch)" || return 1
    base="origin/$("$github_repo" default-branch "$bare")"
    folder="$("$worktree" ensure "$bare" "$branch" "$base")" || return
  fi
  # Also for an existing worktree: it gets files added to .shared since.
  "$shared_files" link "$(dirname "$bare")" "$folder" || return
  printf '%s\n' "$folder"
}

case "${1:-}" in
  branch-name)
    [ "$#" -eq 3 ] || { printf 'usage: github_workspace.sh branch-name <prefix> <description>\n' >&2; exit 2; }
    branch_name "$2" "$3"
    ;;
  *) main "$@" ;;
esac
