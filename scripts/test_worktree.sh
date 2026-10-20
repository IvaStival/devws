#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-worktree.XXXXXX")"
# Resolved, as git reports folders: /var is a link to /private/var on macOS.
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
WORKTREE="$ROOT/config/tmuxinator/worktree.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Worktree test failed: %s\n' "$*" >&2
  exit 1
}

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  GIT_AUTHOR_NAME=devws GIT_AUTHOR_EMAIL=devws@example.com \
  GIT_COMMITTER_NAME=devws GIT_COMMITTER_EMAIL=devws@example.com

# A repo with a commit, a local branch, and a remote with a branch of its own.
REPO="$TEST_ROOT/projects/app"
mkdir -p "$TEST_ROOT/projects"
git init -q -b main "$TEST_ROOT/remote.git" --bare
git init -q -b main "$REPO"
printf 'one\n' > "$REPO/file.txt"
git -C "$REPO" add file.txt
git -C "$REPO" commit -q -m first
git -C "$REPO" branch existing
git -C "$REPO" remote add origin "$TEST_ROOT/remote.git"
git -C "$REPO" push -q origin main main:refs/heads/from-remote
git -C "$REPO" fetch -q origin

worktree() {
  "$WORKTREE" "$@"
}

test_the_folder_sits_next_to_the_repo() {
  [ "$(worktree path "$REPO" feat/login)" = "$TEST_ROOT/projects/app-feat-login" ] ||
    fail "unexpected folder: $(worktree path "$REPO" feat/login)"
}

test_a_new_branch_comes_off_head() {
  local folder
  folder="$(worktree ensure "$REPO" feat/new 2>/dev/null)"
  [ "$folder" = "$TEST_ROOT/projects/app-feat-new" ] || fail "new branch went to $folder"
  [ "$(worktree branch "$folder")" = feat/new ] || fail "the new worktree is not on feat/new"
  [ "$(git -C "$folder" rev-parse HEAD)" = "$(git -C "$REPO" rev-parse HEAD)" ] ||
    fail "the new branch does not start at HEAD"
  [ "$(worktree branch "$REPO")" = main ] || fail "the repo's own checkout changed"
}

test_an_existing_branch_and_worktree() {
  local folder again
  folder="$(worktree ensure "$REPO" existing 2>/dev/null)"
  [ "$(worktree branch "$folder")" = existing ] || fail "the local branch was not checked out"
  again="$(worktree ensure "$REPO" existing 2>/dev/null)"
  [ "$again" = "$folder" ] || fail "an existing worktree was not reused: $again"
  # From inside another worktree, the same one is found.
  again="$(worktree ensure "$TEST_ROOT/projects/app-feat-new" existing 2>/dev/null)"
  [ "$again" = "$folder" ] || fail "from another worktree it went to $again"
}

test_a_remote_branch_is_tracked() {
  local folder
  folder="$(worktree ensure "$REPO" from-remote 2>/dev/null)"
  [ "$(git -C "$folder" rev-parse --abbrev-ref '@{upstream}')" = origin/from-remote ] ||
    fail "the remote branch is not tracked"
}

test_a_foreign_folder_or_bad_name_is_refused() {
  mkdir -p "$TEST_ROOT/projects/app-taken"
  ! worktree ensure "$REPO" taken 2>/dev/null || fail "a folder that is not its worktree was used"
  ! git -C "$REPO" show-ref --verify --quiet refs/heads/taken || fail "a refused worktree left a branch"
  ! worktree ensure "$REPO" 'bad..name' 2>/dev/null || fail "a bad branch name was accepted"
}

test_linked_tells_a_worktree_from_the_repo() {
  worktree linked "$TEST_ROOT/projects/app-feat-new" || fail "a worktree is not linked"
  ! worktree linked "$REPO" || fail "the repo itself counts as a worktree"
  ! worktree linked "$TEST_ROOT" || fail "a folder outside git counts as a worktree"
}

test_status_and_remove_keep_changes() {
  local folder="$TEST_ROOT/projects/app-feat-new"
  [ "$(worktree status "$folder")" = clean ] || fail "a fresh worktree is not clean"
  printf 'draft\n' > "$folder/notes.txt"
  worktree status "$folder" | grep -qx '?? notes.txt' || fail "an untracked file is not listed"
  ! worktree remove "$folder" 2>/dev/null || fail "a worktree with changes was removed"
  [ -f "$folder/notes.txt" ] || fail "the changes were lost"

  rm "$folder/notes.txt"
  worktree remove "$folder" || fail "a clean worktree was not removed"
  [ ! -e "$folder" ] || fail "the folder is still there"
  git -C "$REPO" show-ref --verify --quiet refs/heads/feat/new || fail "removing the worktree deleted the branch"

  ! worktree remove "$REPO" 2>/dev/null || fail "the repo's own folder was removed"
  [ -f "$REPO/file.txt" ] || fail "the repo lost its files"
}

test_the_folder_sits_next_to_the_repo
test_a_new_branch_comes_off_head
test_an_existing_branch_and_worktree
test_a_remote_branch_is_tracked
test_a_foreign_folder_or_bad_name_is_refused
test_linked_tells_a_worktree_from_the_repo
test_status_and_remove_keep_changes

printf 'Worktree tests passed.\n'
