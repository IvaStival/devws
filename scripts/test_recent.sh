#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-recent.XXXXXX")"
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
RECENT="$ROOT/config/tmuxinator/recent.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Recent test failed: %s\n' "$*" >&2
  exit 1
}

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  GIT_AUTHOR_NAME=devws GIT_AUTHOR_EMAIL=devws@example.com \
  GIT_COMMITTER_NAME=devws GIT_COMMITTER_EMAIL=devws@example.com
export HOME="$TEST_ROOT/home" DEVWS_HOME="$TEST_ROOT/devws"
mkdir -p "$HOME"

recent() {
  "$RECENT" "$@"
}

# A managed repo with two worktrees, and a local folder.
REMOTE="$TEST_ROOT/remote.git"
git init -q --bare -b main "$REMOTE"
SEED="$TEST_ROOT/seed"
git init -q -b main "$SEED"
git -C "$SEED" commit -q --allow-empty -m first
git -C "$SEED" remote add origin "$REMOTE"
git -C "$SEED" push -q origin main
BARE="$DEVWS_HOME/acme/app/.bare"
git init -q --bare "$BARE"
git -C "$BARE" remote add origin "$REMOTE"
git -C "$BARE" config remote.origin.fetch '+refs/heads/*:refs/remotes/origin/*'
git -C "$BARE" fetch -q origin
LOGIN="$DEVWS_HOME/acme/app/app-feat-login"
CRASH="$DEVWS_HOME/acme/app/app-fix-crash"
git -C "$BARE" worktree add -q --no-track -b feat/login "$LOGIN" origin/main
git -C "$BARE" worktree add -q --no-track -b fix/crash "$CRASH" origin/main
LOCAL="$TEST_ROOT/projects/tool"
git init -q -b main "$LOCAL"
git -C "$LOCAL" commit -q --allow-empty -m first

test_remembers_the_agent_and_editor() {
  [ -z "$(recent get "$LOGIN")" ] || fail "a folder never opened has a remembered pair"
  recent add "$LOGIN" codex vim
  [ "$(recent get "$LOGIN")" = "codex vim" ] || fail "get: $(recent get "$LOGIN")"
  recent add "$LOGIN" claude nvim
  [ "$(recent get "$LOGIN")" = "claude nvim" ] || fail "the newer pair did not replace the old"
  [ "$(grep -c "$LOGIN" "$HOME/.devws-state/recent.tsv")" = 1 ] || fail "a folder is recorded twice"
}

test_rows_put_the_newest_first_then_the_unopened_worktrees() {
  local rows
  recent add "$LOCAL" claude nvim
  rows="$(recent rows | cut -f2)"
  [ "$rows" = "$LOCAL"$'\n'"$LOGIN"$'\n'"$CRASH" ] || fail "order: $rows"
  # Each row names the repo and branch, and whether it has changes.
  printf 'x\n' > "$LOGIN/notes.txt"
  rows="$(recent rows)"
  grep -q $'^app  *feat/login  *● changes.*\t'"$LOGIN"'$' <<<"$rows" || fail "login row: $rows"
  grep -q $'^tool  *main  *✓ clean.*\t'"$LOCAL"'$' <<<"$rows" || fail "local row: $rows"
  grep -q $'never opened\t'"$CRASH"'$' <<<"$rows" || fail "an unopened worktree is not marked: $rows"
}

test_folders_that_are_gone_are_dropped() {
  rm -rf "$LOCAL"
  ! grep -qx "$LOCAL" <<<"$(recent rows | cut -f2)" || fail "a removed folder is still listed"
}

test_remembers_the_agent_and_editor
test_rows_put_the_newest_first_then_the_unopened_worktrees
test_folders_that_are_gone_are_dropped

printf 'Recent tests passed.\n'
