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
# Never this machine's devws settings.
: > "$TEST_ROOT/devws.env"
export DEVWS_ENV_FILE="$TEST_ROOT/devws.env"

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

# jungleforge itself: its Makefile, two deploy templates, the registry.
mkdir -p "$DEVWS_JUNGLEFORGE/templates/deploy" "$DEVWS_JUNGLEFORGE/templates/deploy-node"
: > "$DEVWS_JUNGLEFORGE/Makefile"
printf 'PHP_VERSION=8.4\nIMAGE_NAME=example-app\nAPP_HTTP_PORT=8080      # nginx\nPHP_CONTAINER=example-app\n' \
  > "$DEVWS_JUNGLEFORGE/templates/deploy/.docker-env.example"
printf 'server {}\n' > "$DEVWS_JUNGLEFORGE/templates/deploy/nginx.conf"
printf 'STACK=node              # node\nIMAGE_NAME=app\nAPP_HTTP_PORT=5173\nAPP_CONTAINER=app-web\n' \
  > "$DEVWS_JUNGLEFORGE/templates/deploy-node/.docker-env.example"
printf 'APP_HTTP_PORT=8092\n' >> "$CHECKOUT/backend/deploy/.docker-env"
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

test_info_is_jungleforges_table() {
  : > "$TEST_ROOT/make.log"
  jungleforge info >/dev/null || fail "info failed"
  [ "$(cat "$TEST_ROOT/make.log")" = "-C $DEVWS_JUNGLEFORGE --no-print-directory info" ] ||
    fail "info: $(cat "$TEST_ROOT/make.log")"
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

# Unset DEVWS_JUNGLEFORGE: a checkout in DEVWS_HOME, else the old one in
# DEVWS_PROJECTS, else DEVWS_HOME's for setup; set, it wins.
test_the_folder_is_found_in_order() {
  local home="$TEST_ROOT/order/devws" projects="$TEST_ROOT/order/projects"
  folder() {
    env -u DEVWS_JUNGLEFORGE DEVWS_HOME="$home" DEVWS_PROJECTS="$projects" "$JUNGLEFORGE" folder
  }
  [ "$(folder)" = "$home/jungleforge" ] || fail "with none, not DEVWS_HOME's: $(folder)"
  mkdir -p "$projects/jungleforge" && : > "$projects/jungleforge/Makefile"
  [ "$(folder)" = "$projects/jungleforge" ] || fail "the DEVWS_PROJECTS checkout was not found: $(folder)"
  mkdir -p "$home/jungleforge" && : > "$home/jungleforge/Makefile"
  [ "$(folder)" = "$home/jungleforge" ] || fail "DEVWS_HOME's checkout does not come first: $(folder)"
  [ "$("$JUNGLEFORGE" folder)" = "$DEVWS_JUNGLEFORGE" ] || fail "DEVWS_JUNGLEFORGE does not win"
  ! env -u DEVWS_JUNGLEFORGE DEVWS_HOME="$home" DEVWS_PROJECTS="$projects" "$JUNGLEFORGE" available ||
    fail "a checkout without .env is available"
  "$JUNGLEFORGE" available || fail "the configured jungleforge is not available"
}

# Setup clones the public repo (here a local one), writes .env with its own
# passwords, never overwrites an .env, and says when Docker is not running.
test_setup_clones_and_writes_env() {
  local remote="$TEST_ROOT/jungleForge-remote" home="$TEST_ROOT/fresh/devws" folder output
  git init -q -b main "$remote"
  : > "$remote/Makefile"
  # jungleforge's placeholder, passed as an argument so no line reads as a
  # hard-coded password to a secret scanner.
  printf 'EXAMPLE_APP_PATH=/absolute/path/to/your-app\nMARIADB_ROOT_PASSWORD=%s\nPOSTGRES_PASSWORD=%s\nREDIS_PORT=6379\n' \
    change_me change_me \
    > "$remote/.env.example"
  git -C "$remote" add -A && git -C "$remote" commit -q -m engine
  folder="$home/jungleforge"
  output="$(env -u DEVWS_JUNGLEFORGE DEVWS_HOME="$home" DEVWS_PROJECTS="$TEST_ROOT/fresh/projects" DEVWS_JUNGLEFORGE_REPO="$remote" "$JUNGLEFORGE" setup 2>&1)" ||
    fail "setup failed: $output"
  [ -f "$folder/Makefile" ] || fail "jungleforge was not cloned into DEVWS_HOME"
  ! grep -q change_me "$folder/.env" || fail "a change_me password is left: $(cat "$folder/.env")"
  [ "$(grep -c '_PASSWORD=[0-9a-f]\{32\}$' "$folder/.env")" = 2 ] || fail "passwords were not generated: $(cat "$folder/.env")"
  [ "$(sed -n 's/^MARIADB_ROOT_PASSWORD=//p' "$folder/.env")" != "$(sed -n 's/^POSTGRES_PASSWORD=//p' "$folder/.env")" ] ||
    fail "two services share a password"
  grep -qx '# EXAMPLE_APP_PATH=/absolute/path/to/your-app' "$folder/.env" || fail "the placeholder app is registered"
  grep -q 'Docker is not running' <<<"$output" || fail "a stopped Docker was not reported: $output"
  printf 'OTHER_PATH=/kept\n' >> "$folder/.env"
  env -u DEVWS_JUNGLEFORGE DEVWS_HOME="$home" DEVWS_PROJECTS="$TEST_ROOT/fresh/projects" DEVWS_JUNGLEFORGE_REPO="$remote" "$JUNGLEFORGE" setup >/dev/null 2>&1 ||
    fail "setup failed on an existing jungleforge"
  grep -qx 'OTHER_PATH=/kept' "$folder/.env" || fail "setup overwrote an existing .env"
}

test_templates_name_their_stack() {
  [ "$(jungleforge templates)" = "deploy"$'\t'"php"$'\n'"deploy-node"$'\t'"node" ] ||
    fail "templates: $(jungleforge templates)"
}

# Scaffold copies the template as deploy/, named after the app, on the port
# after the highest registered one (phi-planner uses 8092).
test_scaffold_names_the_app_on_a_free_port() {
  local app="$TEST_ROOT/projects/shop"
  mkdir -p "$app"
  [ "$(jungleforge scaffold "$app" deploy shop)" = 8093 ] || fail "not the next free port"
  [ -f "$app/deploy/nginx.conf" ] || fail "the template was not copied"
  grep -qx 'IMAGE_NAME=shop' "$app/deploy/.docker-env" &&
    grep -qx 'PHP_CONTAINER=shop' "$app/deploy/.docker-env" &&
    grep -q '^APP_HTTP_PORT=8093 ' "$app/deploy/.docker-env" ||
    fail "the .docker-env is not the app's: $(cat "$app/deploy/.docker-env")"
  ! jungleforge scaffold "$app" deploy shop 2>/dev/null || fail "an existing deploy/ was overwritten"
}

test_register_adds_then_replaces_the_line() {
  local folder="$TEST_ROOT/projects/shop"
  jungleforge register shop "$folder" || fail "register failed"
  grep -qx "SHOP_PATH=$folder" "$DEVWS_JUNGLEFORGE/.env" || fail "the line was not added"
  jungleforge register shop "$CHECKOUT" || fail "register again failed"
  [ "$(grep -c '^SHOP_PATH=' "$DEVWS_JUNGLEFORGE/.env")" = 1 ] && grep -qx "SHOP_PATH=$CHECKOUT" "$DEVWS_JUNGLEFORGE/.env" ||
    fail "the line was not replaced: $(cat "$DEVWS_JUNGLEFORGE/.env")"
  grep -qx 'REGISTRY=ghcr.io/acme' "$DEVWS_JUNGLEFORGE/.env" || fail "another line was lost"
  ! jungleforge register 'Bad_Name' "$folder" 2>/dev/null || fail "an invalid name was registered"
  sed -i '' '/^SHOP_PATH=/d' "$DEVWS_JUNGLEFORGE/.env"
}

# Settings: the app's .docker-env in the folder given, and the shared .env
# without the registry, its passwords masked; commented-out lines are not
# settings.
test_settings_list_the_apps_and_the_shared_ones() {
  printf 'APP_HTTP_PORT=8091   # the web\n# VITE_PORT=5173\n' >> "$CHECKOUT/web/deploy/.docker-env"
  # A stand-in value, passed as an argument so no line reads as a hard-coded
  # password to a secret scanner.
  printf 'POSTGRES_PASSWORD=%s\nPOSTGRES_PORT=5432    # postgres\n' not-a-real-value >> "$DEVWS_JUNGLEFORGE/.env"
  [ "$(jungleforge settings "$CHECKOUT" phi-planner-web)" = "APP_CONTAINER"$'\t'"phi-web"$'\t'$'\n'"APP_HTTP_PORT"$'\t'"8091"$'\t'"the web" ] ||
    fail "app settings: $(jungleforge settings "$CHECKOUT" phi-planner-web)"
  [ "$(jungleforge settings shared)" = "REGISTRY"$'\t'"ghcr.io/acme"$'\t'$'\n'"POSTGRES_PASSWORD"$'\t'"••••"$'\t'$'\n'"POSTGRES_PORT"$'\t'"5432"$'\t'"postgres" ] ||
    fail "shared settings: $(jungleforge settings shared)"
  ! jungleforge settings "$CHECKOUT" shop 2>/dev/null || fail "settings of an app that is not the repo's"
}

# Set rewrites the line in place, its comment and the file's mode kept; a
# port must be a free number, the error naming who has it.
test_set_changes_the_line_and_checks_ports() {
  local error
  jungleforge set "$CHECKOUT" phi-planner-web APP_HTTP_PORT 8095 || fail "set failed"
  grep -qx 'APP_HTTP_PORT=8095   # the web' "$CHECKOUT/web/deploy/.docker-env" ||
    fail "the line: $(cat "$CHECKOUT/web/deploy/.docker-env")"
  jungleforge set "$CHECKOUT" phi-planner-web APP_HTTP_PORT 8095 || fail "its own port was refused"
  error="$(jungleforge set "$CHECKOUT" phi-planner-web APP_HTTP_PORT 8092 2>&1)" && fail "a taken port was set"
  [[ "$error" == *"phi-planner's APP_HTTP_PORT"* ]] || fail "the owner is not named: $error"
  error="$(jungleforge set "$CHECKOUT" phi-planner-web APP_HTTP_PORT 5432 2>&1)" && fail "a shared port was set"
  [[ "$error" == *"shared POSTGRES_PORT"* ]] || fail "the shared owner is not named: $error"
  local wrong
  for wrong in abc 0 70000; do
    ! jungleforge set "$CHECKOUT" phi-planner-web APP_HTTP_PORT "$wrong" 2>/dev/null || fail "port $wrong was set"
  done
  ! jungleforge set "$CHECKOUT" phi-planner-web NOPE 1 2>/dev/null || fail "an unknown key was set"
  ! jungleforge set "$CHECKOUT" phi-planner-web APP_CONTAINER 'a b' 2>/dev/null || fail "a value with a space was set"

  chmod 600 "$DEVWS_JUNGLEFORGE/.env"
  jungleforge set shared POSTGRES_PORT 5433 || fail "set shared failed"
  grep -qx 'POSTGRES_PORT=5433    # postgres' "$DEVWS_JUNGLEFORGE/.env" || fail "the shared line: $(cat "$DEVWS_JUNGLEFORGE/.env")"
  [ "$(stat -f %Lp "$DEVWS_JUNGLEFORGE/.env")" = 600 ] || fail ".env lost its mode"

  jungleforge set "$WORKTREE" phi-planner PHP_CONTAINER phi-feat || fail "set on the branch failed"
  grep -qx 'PHP_CONTAINER=phi-feat' "$WORKTREE/backend/deploy/.docker-env" &&
    grep -qx 'PHP_CONTAINER=phi-php' "$CHECKOUT/backend/deploy/.docker-env" ||
    fail "set did not change the branch's file only"
  git -C "$WORKTREE" checkout -q -- backend/deploy/.docker-env
}

test_apps_are_the_repos_registered_folders
test_targets_are_main_and_the_worktrees
test_the_folder_is_found_in_order
test_setup_clones_and_writes_env
test_templates_name_their_stack
test_scaffold_names_the_app_on_a_free_port
test_register_adds_then_replaces_the_line
test_running_reads_the_mounted_folder
test_run_starts_services_then_the_apps_on_the_folder
test_deps_run_again_only_when_a_lock_file_changes
test_run_without_names_moves_every_app
test_info_is_jungleforges_table
test_stop_and_migrate_use_the_running_folder
test_an_unregistered_repo_says_so
test_settings_list_the_apps_and_the_shared_ones
test_set_changes_the_line_and_checks_ports

printf 'jungleforge tests passed.\n'
