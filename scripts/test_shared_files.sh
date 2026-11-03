#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-shared.XXXXXX")"
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
SHARED="$ROOT/config/tmuxinator/shared_files.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Shared files test failed: %s\n' "$*" >&2
  exit 1
}

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  GIT_AUTHOR_NAME=devws GIT_AUTHOR_EMAIL=devws@example.com \
  GIT_COMMITTER_NAME=devws GIT_COMMITTER_EMAIL=devws@example.com
export DEVWS_PROJECTS="$TEST_ROOT/projects"

# An existing checkout of acme/app, with config git ignores and things that
# must never be shared.
CHECKOUT="$DEVWS_PROJECTS/app"
mkdir -p "$CHECKOUT/backend/deploy" "$CHECKOUT/backend/vendor/pkg" "$CHECKOUT/web/node_modules/x" \
  "$CHECKOUT/backend/storage/framework/views" "$CHECKOUT/.idea" "$CHECKOUT/backend/database"
git init -q -b main "$CHECKOUT"
printf '.env\n.docker-env\nvendor/\nnode_modules/\nstorage/framework/\n.idea/\n*.sqlite\n*.log\n' > "$CHECKOUT/.gitignore"
git -C "$CHECKOUT" add .gitignore
git -C "$CHECKOUT" commit -q -m first
git -C "$CHECKOUT" remote add origin git@github.com:acme/app.git
printf 'APP_KEY=x\n' > "$CHECKOUT/backend/.env"
printf 'APP_HTTP_PORT=8081\n' > "$CHECKOUT/backend/deploy/.docker-env"
printf 'data\n' > "$CHECKOUT/backend/database/database.sqlite"
printf 'x\n' > "$CHECKOUT/backend/vendor/pkg/a.php"
printf 'x\n' > "$CHECKOUT/web/node_modules/x/i.js"
printf 'x\n' > "$CHECKOUT/backend/storage/framework/views/v.php"
printf 'x\n' > "$CHECKOUT/.idea/w.xml"
printf 'x\n' > "$CHECKOUT/backend/laravel.log"
# Another repo in the same folder, which is not a candidate.
git init -q -b main "$DEVWS_PROJECTS/other"
git -C "$DEVWS_PROJECTS/other" remote add origin https://github.com/acme/other.git

REPO_DIR="$TEST_ROOT/devws/acme/app"
WORKTREE="$REPO_DIR/app-feat-x"
mkdir -p "$WORKTREE/backend"

shared() {
  "$SHARED" "$@"
}

test_candidates_are_checkouts_of_the_same_repo() {
  [ "$(shared candidates acme/app)" = "$CHECKOUT" ] || fail "candidates: $(shared candidates acme/app)"
  [ -z "$(shared candidates acme/none)" ] || fail "a repo with no checkout had candidates"
}

test_ignored_lists_config_but_not_packages_or_caches() {
  local listed
  listed="$(shared ignored "$CHECKOUT" | LC_ALL=C sort | tr '\n' ' ')"
  [ "$listed" = "backend/.env backend/database/database.sqlite backend/deploy/.docker-env " ] ||
    fail "ignored: $listed"
}

test_seed_copies_the_chosen_files() {
  shared seed "$REPO_DIR" "$CHECKOUT" backend/.env backend/database/database.sqlite backend/deploy/.docker-env
  [ "$(cat "$REPO_DIR/.shared/backend/.env")" = APP_KEY=x ] || fail ".env was not seeded"
  [ ! -L "$REPO_DIR/.shared/backend/.env" ] || fail "seed linked instead of copying"
  ! shared seed "$REPO_DIR" "$CHECKOUT" ../escape 2>/dev/null || fail "a path outside the folder was seeded"
}

test_link_symlinks_config_and_copies_databases() {
  printf 'mine\n' > "$WORKTREE/backend/deploy.txt"
  mkdir -p "$WORKTREE/backend/deploy"
  printf 'APP_HTTP_PORT=9999\n' > "$WORKTREE/backend/deploy/.docker-env"
  shared link "$REPO_DIR" "$WORKTREE"
  [ "$(readlink "$WORKTREE/backend/.env")" = "$REPO_DIR/.shared/backend/.env" ] ||
    fail ".env is not linked to .shared"
  [ -f "$WORKTREE/backend/database/database.sqlite" ] && [ ! -L "$WORKTREE/backend/database/database.sqlite" ] ||
    fail "the database was not copied"
  [ "$(cat "$WORKTREE/backend/deploy/.docker-env")" = APP_HTTP_PORT=9999 ] ||
    fail "an existing file was replaced"
  # Linking again changes nothing.
  shared link "$REPO_DIR" "$WORKTREE" || fail "linking twice failed"
}

test_candidates_are_checkouts_of_the_same_repo
test_ignored_lists_config_but_not_packages_or_caches
test_seed_copies_the_chosen_files
test_link_symlinks_config_and_copies_databases

printf 'Shared files tests passed.\n'
