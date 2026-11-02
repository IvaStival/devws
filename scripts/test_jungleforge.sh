#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-jungleforge.XXXXXX")"
TEST_ROOT="$(cd "$TEST_ROOT" && pwd -P)"
JUNGLEFORGE="$ROOT/config/tmuxinator/jungleforge.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'jungleforge test failed: %s\n' "$*" >&2
  exit 1
}

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  GIT_AUTHOR_NAME=devws GIT_AUTHOR_EMAIL=devws@example.com \
  GIT_COMMITTER_NAME=devws GIT_COMMITTER_EMAIL=devws@example.com
export DEVWS_JUNGLEFORGE="$TEST_ROOT/jungleforge"

# make logs its arguments; docker inspect answers from docker/<container>
# ("true|<mount source>|…"), or fails for a container that does not exist.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/docker"
cat > "$TEST_ROOT/bin/make" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$TEST_ROOT/make.log"
EOF
cat > "$TEST_ROOT/bin/docker" <<EOF
#!/usr/bin/env bash
[ "\$1" = inspect ] || exit 1
cat "$TEST_ROOT/docker/\${@: -1}" 2>/dev/null
EOF
chmod +x "$TEST_ROOT/bin/make" "$TEST_ROOT/bin/docker"
export PATH="$TEST_ROOT/bin:$PATH"

# PhiPlanner: a backend and a web app in one repo, both registered; a branch
# of it in a worktree; and another registered repo.
CHECKOUT="$TEST_ROOT/projects/PhiPlanner"
mkdir -p "$CHECKOUT/backend/deploy" "$CHECKOUT/web/deploy"
git init -q -b main "$CHECKOUT"
git -C "$CHECKOUT" remote add origin git@github.com:acme/PhiPlanner.git
printf 'PHP_CONTAINER=phi-php\n' > "$CHECKOUT/backend/deploy/.docker-env"
printf 'APP_CONTAINER=phi-web\n' > "$CHECKOUT/web/deploy/.docker-env"
printf '{"lock":1}\n' > "$CHECKOUT/backend/composer.lock"
git -C "$CHECKOUT" add -A
git -C "$CHECKOUT" commit -q -m first
WORKTREE="$TEST_ROOT/devws/acme/PhiPlanner/PhiPlanner-feat-x"
git -C "$CHECKOUT" worktree add -q -b feat/x "$WORKTREE"
OTHER="$TEST_ROOT/projects/other"
git init -q -b main "$OTHER"
git -C "$OTHER" remote add origin https://github.com/acme/other.git

mkdir -p "$DEVWS_JUNGLEFORGE"
cat > "$DEVWS_JUNGLEFORGE/.env" <<EOF
# registry
PHI_PLANNER_PATH=$CHECKOUT/backend
PHI_PLANNER_WEB_PATH="$CHECKOUT/web"
OTHER_PATH=$OTHER
REGISTRY=ghcr.io/acme
EOF

jungleforge() {
  "$JUNGLEFORGE" "$@"
}

test_apps_are_the_repos_registered_folders() {
  local expected
  expected="phi-planner"$'\t'"backend"$'\n'"phi-planner-web"$'\t'"web"
  [ "$(jungleforge apps "$WORKTREE" | cut -f1,2)" = "$expected" ] ||
    fail "apps from the worktree: $(jungleforge apps "$WORKTREE")"
  [ "$(jungleforge apps "$OTHER" | cut -f1,2)" = "other"$'\t' ] || fail "apps of a root-level repo"
  ! jungleforge apps "$TEST_ROOT" 2>/dev/null || fail "a folder outside git has apps"
}

test_targets_are_main_and_the_worktrees() {
  local expected
  expected="main"$'\t'"$CHECKOUT"$'\n'"feat/x"$'\t'"$WORKTREE"
  [ "$(jungleforge targets "$WORKTREE")" = "$expected" ] ||
    fail "targets: $(jungleforge targets "$WORKTREE")"
}

test_running_reads_the_mounted_folder() {
  printf 'true|%s|/somewhere/php.ini\n' "$WORKTREE/backend" > "$TEST_ROOT/docker/phi-php"
  printf 'false|%s\n' "$CHECKOUT/web" > "$TEST_ROOT/docker/phi-web"
  [ "$(jungleforge running "$CHECKOUT")" = "phi-planner"$'\t'"$WORKTREE"$'\n'"phi-planner-web"$'\t' ] ||
    fail "running: $(jungleforge running "$CHECKOUT")"
  # For the sidebar: the folders anything runs from, each repo asked once.
  [ "$(jungleforge running-folders "$CHECKOUT" "$WORKTREE" "$OTHER" "$TEST_ROOT")" = "$WORKTREE" ] ||
    fail "running-folders: $(jungleforge running-folders "$CHECKOUT" "$WORKTREE" "$OTHER" "$TEST_ROOT")"
  rm -f "$TEST_ROOT/docker/phi-php" "$TEST_ROOT/docker/phi-web"
}

test_run_starts_services_then_the_apps_on_the_folder() {
  : > "$TEST_ROOT/make.log"
  jungleforge run "$WORKTREE" phi-planner >/dev/null || fail "run failed"
  [ "$(sed -n 1p "$TEST_ROOT/make.log")" = "-C $DEVWS_JUNGLEFORGE services-start" ] ||
    fail "the services did not start first: $(cat "$TEST_ROOT/make.log")"
  [ "$(sed -n 2p "$TEST_ROOT/make.log")" = "-C $DEVWS_JUNGLEFORGE start phi-planner PHI_PLANNER_PATH=$WORKTREE/backend" ] ||
    fail "start: $(sed -n 2p "$TEST_ROOT/make.log")"
  grep -q '^-C .* start phi-planner-web' "$TEST_ROOT/make.log" && fail "an app that was not asked for started"
  # Never installed in this folder: it installs.
  [ "$(sed -n 3p "$TEST_ROOT/make.log")" = "-C $DEVWS_JUNGLEFORGE deps phi-planner PHI_PLANNER_PATH=$WORKTREE/backend" ] ||
    fail "deps did not run on a fresh folder: $(cat "$TEST_ROOT/make.log")"
}

test_deps_run_again_only_when_a_lock_file_changes() {
  : > "$TEST_ROOT/make.log"
  jungleforge run "$WORKTREE" phi-planner >/dev/null
  ! grep -q ' deps ' "$TEST_ROOT/make.log" || fail "deps ran with unchanged lock files"
  printf '{"lock":2}\n' > "$WORKTREE/backend/composer.lock"
  jungleforge run "$WORKTREE" phi-planner >/dev/null
  grep -q ' deps phi-planner ' "$TEST_ROOT/make.log" || fail "deps did not run after composer.lock changed"
}

test_run_without_names_moves_every_app() {
  : > "$TEST_ROOT/make.log"
  jungleforge run "$CHECKOUT" >/dev/null
  grep -q "start phi-planner PHI_PLANNER_PATH=$CHECKOUT/backend$" "$TEST_ROOT/make.log" &&
    grep -q "start phi-planner-web PHI_PLANNER_WEB_PATH=$CHECKOUT/web$" "$TEST_ROOT/make.log" ||
    fail "not every app moved: $(cat "$TEST_ROOT/make.log")"
}

test_stop_and_migrate_use_the_running_folder() {
  printf 'true|%s\n' "$WORKTREE/backend" > "$TEST_ROOT/docker/phi-php"
  : > "$TEST_ROOT/make.log"
  jungleforge stop "$CHECKOUT" >/dev/null
  grep -q "stop phi-planner$" "$TEST_ROOT/make.log" ||
    fail "stop: $(cat "$TEST_ROOT/make.log")"
  jungleforge migrate "$CHECKOUT" phi-planner >/dev/null
  grep -q "migrate phi-planner PHI_PLANNER_PATH=$WORKTREE/backend$" "$TEST_ROOT/make.log" ||
    fail "migrate did not use the running branch: $(cat "$TEST_ROOT/make.log")"
  rm -f "$TEST_ROOT/docker/phi-php"
  ! jungleforge migrate "$CHECKOUT" phi-planner 2>/dev/null || fail "migrate ran with the app stopped"
}

test_an_unregistered_repo_says_so() {
  local repo="$TEST_ROOT/projects/unregistered"
  git init -q -b main "$repo"
  ! jungleforge run "$repo" 2>"$TEST_ROOT/err" || fail "an unregistered repo ran"
  grep -q 'Not a jungleforge project' "$TEST_ROOT/err" || fail "no explanation: $(cat "$TEST_ROOT/err")"
}

test_apps_are_the_repos_registered_folders
test_targets_are_main_and_the_worktrees
test_running_reads_the_mounted_folder
test_run_starts_services_then_the_apps_on_the_folder
test_deps_run_again_only_when_a_lock_file_changes
test_run_without_names_moves_every_app
test_stop_and_migrate_use_the_running_folder
test_an_unregistered_repo_says_so

printf 'jungleforge tests passed.\n'
