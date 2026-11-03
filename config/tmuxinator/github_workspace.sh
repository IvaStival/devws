#!/usr/bin/env bash
# The folder for a workspace, chosen from the start screen: a folder opened
# before (recent.sh, newest first, and the worktrees never opened), a local
# folder (one in DEVWS_PROJECTS, or a typed path), or a branch of one of
# your GitHub repos, or of another repo cloned by URL: one already on GitHub,
# tracking it, or a new one off a fresh origin/<default branch>
# (github_repo.sh, worktree.sh). The questions
# use the terminal; only the folder is printed. Run by `devws` with no folder
# and by the menu's "New GitHub workspace".
#
#   github_workspace.sh                        the start screen
#   github_workspace.sh <repo>                 that repo: one of its worktrees,
#                                              or a branch
#   github_workspace.sh branch-name <prefix> <description>
#                                              <prefix>/<description as a slug>

set -u

script_dir="$(cd "$(dirname "$0")" && pwd)"
github_repo="$script_dir/github_repo.sh"
worktree="$script_dir/worktree.sh"
shared_files="$script_dir/shared_files.sh"
picker="$script_dir/picker.sh"
prefixes='feat fix chore refactor docs'
custom_name='Custom name…'
# The environment wins over .env, as in github_repo.sh. Read in a subshell:
# sourcing .env here would replace the DEVWS_HOME the scripts this runs get.
# shellcheck source=/dev/null
DEVWS_PROJECTS="${DEVWS_PROJECTS:-$([ -r "$script_dir/.env" ] && . "$script_dir/.env" >/dev/null 2>&1; printf '%s' "${DEVWS_PROJECTS:-}")}"
DEVWS_PROJECTS="${DEVWS_PROJECTS:-~/Projects}"

expand_home() {
  case "$1" in
    "~") printf '%s' "$HOME" ;;
    "~/"*) printf '%s' "$HOME/${1#\~/}" ;;
    *) printf '%s' "$1" ;;
  esac
}

# "Login page: OAuth & SSO!" → login-page-oauth-sso
branch_name() {
  local slug
  slug="$(printf '%s' "$2" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-')"
  slug="${slug#-}"
  slug="${slug%-}"
  [ -n "$slug" ] || return 1
  printf '%s/%s\n' "$1" "$slug"
}

# The owner first — your repos, then each organization's — then one of its
# repos, the cloned ones first and marked ●. Esc among the repos goes back to
# the owners.
pick_repo() {
  local refresh_row='↻ Refresh the list' other_row='↓ Clone another repo (URL / owner/repo)…'
  local refresh="" root choice owner repos
  root="$("$github_repo" home)"
  while :; do
    choice="$(
      {
        "$github_repo" owners ${refresh:+--refresh} | while IFS=$'\t' read -r owner repos mine; do
          if [ -n "$mine" ]; then
            printf '★ My repos (%s)\t%s\t%s\n' "$owner" "$repos" "$owner"
          else
            printf '  %s\t%s\t%s\n' "$owner" "$repos" "$owner"
          fi
        done
        printf '%s\t\t\n' "$other_row" "$refresh_row"
      } |
        "$picker" Owner 'Whose repo' --delimiter=$'\t' --with-nth=1,2 --tabstop=32
    )" || return 1
    refresh=""
    case "${choice%%$'\t'*}" in
      "$refresh_row")
        refresh=1
        continue
        ;;
      "$other_row")
        read -e -r -p 'Repo (owner/repo or URL): ' choice || return 1
        [ -n "$choice" ] || return 1
        printf '%s\n' "$choice"
        return 0
        ;;
    esac
    owner="${choice##*$'\t'}"

    choice="$(
      "$github_repo" list --owner "$owner" | while IFS= read -r repo; do
        if [ -d "$root/$repo/.bare" ]; then
          printf '● %s\t%s\n' "${repo#*/}" "$repo"
        else
          printf '  %s\t%s\n' "${repo#*/}" "$repo"
        fi
      done |
        "$picker" "$owner" "$owner's repos (● cloned)" --delimiter=$'\t' --with-nth=1
    )" || continue
    printf '%s\n' "${choice##*$'\t'}"
    return 0
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
      "$picker" Worktree 'Continue a worktree, or start a new branch' \
        --delimiter=$'\t' --with-nth=1,2 --tabstop=32
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
      "$picker" 'Import from' "Copy $name's untracked files (.env, .docker-env…) from a checkout you have"
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
      "$picker" Files 'Files every branch gets' --multi --bind 'load:select-all'
  )
  mkdir -p "$repo_dir/.shared"
  [ "${#files[@]}" -eq 0 ] || "$shared_files" seed "$repo_dir" "$source" "${files[@]}"
}

ask_branch() {
  local prefix description branch
  prefix="$(printf '%s\n' $prefixes "$custom_name" | "$picker" Kind 'What the branch is for')" || return 1
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

# A branch on GitHub (its folder, when it has one, is marked), or a new one
# named by ask_branch. Esc on the new one's kind comes back to the list.
pick_branch() {
  local bare="$1" new_row='+ New branch…' open choice branch age
  open="$("$github_repo" worktrees "$bare" | cut -f 1)"
  while :; do
    choice="$(
      {
        printf '%s\t\t\n' "$new_row"
        "$github_repo" branches "$bare" | while IFS=$'\t' read -r branch age; do
          if printf '%s\n' "$open" | grep -qxF -- "$branch"; then
            printf '%s\t%s  ● folder\t%s\n' "$branch" "$age" "$branch"
          else
            printf '%s\t%s\t%s\n' "$branch" "$age" "$branch"
          fi
        done
      } |
        "$picker" Branch 'Open a branch from GitHub, or start a new one (● has a folder)' \
          --delimiter=$'\t' --with-nth=1,2 --tabstop=32
    )" || return 1
    if [ "${choice%%$'\t'*}" = "$new_row" ]; then
      ask_branch && return 0
      continue
    fi
    printf '%s\n' "${choice##*$'\t'}"
    return 0
  done
}

# A folder on this machine: one in DEVWS_PROJECTS (a git repo shows its
# branch), or a typed path. Esc goes back to the start screen, and so does a
# path that does not exist, once said.
pick_local_folder() {
  local projects typed_row='✎ Type a path…' choice folder branch
  projects="$(expand_home "$DEVWS_PROJECTS")"
  choice="$(
    {
      printf '%s\t\n' "$typed_row"
      for folder in "$projects"/*/; do
        [ -d "$folder" ] || continue
        folder="${folder%/}"
        branch=""
        [ -e "$folder/.git" ] && branch="$(git -C "$folder" symbolic-ref --quiet --short HEAD 2>/dev/null)"
        printf '%-28s %s\t%s\n' "$(basename "$folder")" "${branch:+⎇ $branch}" "$folder"
      done
    } | "$picker" Folder "A folder in $DEVWS_PROJECTS, or type a path" --delimiter=$'\t' --with-nth=1
  )" || return 1
  [ -n "$choice" ] || return 1
  folder="${choice##*$'\t'}"
  if [ -z "$folder" ]; then
    read -e -r -p 'Folder (Tab completes): ' folder || return 1
    [ -n "$folder" ] || return 1
    folder="$(expand_home "$folder")"
  fi
  if ! (cd "$folder" 2>/dev/null && pwd -P); then
    printf 'Folder does not exist: %s\n' "$folder" >&2
    read -r -p 'Press Enter to go back…' _ || true
    return 1
  fi
}

# Reopen a folder or open a local one (printed as "open <folder>"), or open
# a branch of a repo ("new <repo>"), picked by owner or typed. Esc on the
# owners or the local folders comes back here.
start_screen() {
  local new_row='⎇ Branch from GitHub…' clone_row='↓ Clone another repo (URL / owner/repo)…'
  local local_row='⌂ Open a local folder…'
  local choice repo folder
  while :; do
    choice="$(
      { printf '%s\t\n' "$new_row" "$local_row" "$clone_row"; "$script_dir/recent.sh" rows; } |
        "$picker" devws 'Reopen a workspace, open a folder, or open a GitHub branch' --delimiter=$'\t' --with-nth=1
    )" || return 1
    case "${choice%%$'\t'*}" in
      "$new_row")
        repo="$(pick_repo)" || continue
        ;;
      "$local_row")
        folder="$(pick_local_folder)" || continue
        printf 'open %s\n' "$folder"
        return 0
        ;;
      "$clone_row")
        read -e -r -p 'Repo (owner/repo or URL): ' repo || continue
        [ -n "$repo" ] || continue
        ;;
      *)
        printf 'open %s\n' "${choice##*$'\t'}"
        return 0
        ;;
    esac
    printf 'new %s\n' "$repo"
    return 0
  done
}

main() {
  local repo="${1:-}" bare folder branch base choice
  # From the start screen, Esc on the branches goes back to it.
  while :; do
    if [ -z "${1:-}" ]; then
      choice="$(start_screen)" || return 1
      if [ "${choice%% *}" = open ]; then
        printf '%s\n' "${choice#open }"
        return 0
      fi
      repo="${choice#new }"
    fi
    # A failure (exit 2) is told apart from a cancelled question (1).
    bare="$("$github_repo" ensure "$repo")" || return
    seed_shared "$(dirname "$bare")" "$("$github_repo" parse "$repo")"

    # From the start screen it is a branch: its worktrees were listed there.
    folder=""
    if [ -n "${1:-}" ]; then
      folder="$(pick_worktree "$bare")" || return 1
    fi
    [ -z "$folder" ] || break
    if branch="$(pick_branch "$bare")"; then
      base="origin/$("$github_repo" default-branch "$bare")"
      folder="$("$worktree" ensure "$bare" "$branch" "$base")" || return
      break
    fi
    [ -z "${1:-}" ] || return 1
  done
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
