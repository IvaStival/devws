#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-link.XXXXXX")"
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Link scripts test failed: %s\n' "$*" >&2
  exit 1
}

# A devws checkout's config/tmuxinator, with the real link_scripts.sh, and a
# ~/.tmuxinator holding devws links, another project's file and a link of its
# own.
SOURCE="$TEST_ROOT/devws/config/tmuxinator"
mkdir -p "$SOURCE/project-template" "$TEST_ROOT/home/.tmuxinator"
cp "$ROOT/config/tmuxinator/link_scripts.sh" "$SOURCE/"
printf 'x\n' > "$SOURCE/job.sh"
printf 'x\n' > "$SOURCE/job_panel.sh"
printf 'DEVWS_AGENTS=claude\n' > "$SOURCE/.env"
DEST="$TEST_ROOT/home/.tmuxinator"
ln -s "$SOURCE/link_scripts.sh" "$DEST/link_scripts.sh"
ln -s "$SOURCE/job.sh" "$DEST/job.sh"
ln -s "$SOURCE/gone.sh" "$DEST/gone.sh"
printf 'name: other\n' > "$DEST/other-project.yml"
ln -s "$TEST_ROOT/elsewhere.sh" "$DEST/elsewhere.sh"
export HOME="$TEST_ROOT/home"

test_missing_scripts_are_linked_and_stale_ones_removed() {
  local output
  # Run through its own link, as devws does.
  output="$("$DEST/link_scripts.sh")"
  [ "$(readlink "$DEST/job_panel.sh")" = "$SOURCE/job_panel.sh" ] || fail "a missing script was not linked"
  [ "$(readlink "$DEST/.env")" = "$SOURCE/.env" ] || fail ".env was not linked"
  [ "$(readlink "$DEST/project-template")" = "$SOURCE/project-template" ] || fail "a folder was not linked"
  [ ! -L "$DEST/gone.sh" ] || fail "a link to a removed script was kept"
  grep -q 'job_panel.sh' <<<"$output" || fail "the new link was not reported: $output"
}

test_other_files_are_left_alone() {
  [ "$(cat "$DEST/other-project.yml")" = 'name: other' ] || fail "another project's file was touched"
  [ "$(readlink "$DEST/elsewhere.sh")" = "$TEST_ROOT/elsewhere.sh" ] || fail "a link pointing elsewhere was touched"
  # A real file where a devws script would go is kept, not replaced.
  rm "$DEST/job_panel.sh"
  printf 'mine\n' > "$DEST/job_panel.sh"
  "$DEST/link_scripts.sh" >/dev/null
  [ "$(cat "$DEST/job_panel.sh")" = mine ] || fail "a real file was replaced"
}

test_a_second_run_changes_nothing_and_says_nothing() {
  [ -z "$("$DEST/link_scripts.sh")" ] || fail "a second run reported changes"
}

test_missing_scripts_are_linked_and_stale_ones_removed
test_other_files_are_left_alone
test_a_second_run_changes_nothing_and_says_nothing

printf 'Link scripts tests passed.\n'
