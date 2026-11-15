#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-bench.XXXXXX")"
BENCH="$ROOT/scripts/bench_team.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Bench test failed: %s\n' "$*" >&2
  exit 1
}

# No identity of the machine's: the run's repo must get one of its own.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 HOME="$TEST_ROOT/home"
mkdir -p "$HOME"

test_a_run_starts_from_a_fresh_committed_copy_of_the_fixture() {
  local folder="$TEST_ROOT/notes-reviewed" output
  output="$("$BENCH" notes reviewed "$folder")" || fail "preparing the run failed"
  [[ -f "$folder/GOAL.md" ]] || fail "the goal was not copied"
  [[ -z "$(git -C "$folder" status --porcelain)" ]] || fail "the copy is not committed"
  [[ "$(git -C "$folder" branch --show-current)" == bench/reviewed-* ]] ||
    fail "the run is not on a branch of its own"
  git -C "$folder" config user.name >/dev/null || fail "the repo has no git identity"
  grep -q "devws agentic \"$folder\"" <<<"$output" || fail "no start command: $output"
  ! grep -q -- '--legacy' <<<"$output" || fail "a reviewed run was told to start legacy"
  grep -q 'devagentics bench report' <<<"$output" || fail "no report command: $output"
}

test_a_legacy_run_creates_its_legacy_task_first() {
  local output
  output="$("$BENCH" notes legacy "$TEST_ROOT/notes-legacy")" || fail "preparing failed"
  grep -q 'devagentics task start .* --legacy' <<<"$output" ||
    fail "no legacy task command: $output"
}

test_the_fixture_acceptance_fails_until_the_team_builds_it() {
  local folder="$TEST_ROOT/notes-reviewed"
  if (cd "$folder" && python3 -m unittest discover -s tests -t . >/dev/null 2>&1); then
    fail "the skeleton already passes its acceptance tests"
  fi
}

test_bad_arguments_and_reused_folders_are_refused() {
  ! "$BENCH" notes >/dev/null 2>&1 || fail "a missing mode was accepted"
  ! "$BENCH" nope reviewed "$TEST_ROOT/x" >/dev/null 2>&1 || fail "an unknown fixture was accepted"
  ! "$BENCH" notes fast "$TEST_ROOT/y" >/dev/null 2>&1 || fail "an unknown mode was accepted"
  ! "$BENCH" notes reviewed "$TEST_ROOT/notes-reviewed" >/dev/null 2>&1 ||
    fail "an existing folder was reused"
}

test_a_run_starts_from_a_fresh_committed_copy_of_the_fixture
test_a_legacy_run_creates_its_legacy_task_first
test_the_fixture_acceptance_fails_until_the_team_builds_it
test_bad_arguments_and_reused_folders_are_refused

printf 'Bench tests passed.\n'
