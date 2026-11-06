#!/usr/bin/env bash
# Runs a repo's branches in jungleforge (DEVWS_JUNGLEFORGE, default
# ~/Projects/jungleforge), the Docker control plane whose registry maps an app
# to one folder (PHI_PLANNER_PATH=~/Projects/PhiPlanner/backend). Running a
# branch starts the same app, port and databases on the branch's folder, by
# overriding that path for one make call: jungleforge's .env is never edited.
# Only jungleforge's public targets are used.
#
#   jungleforge.sh apps <folder>     "<app><TAB><subfolder>" per registered app
#                                    of <folder>'s repo (same git origin);
#                                    fails, saying so, when it has none
#   jungleforge.sh targets <folder>  "<label><TAB><folder>": main (the
#                                    registry's checkout), then each worktree
#   jungleforge.sh running <folder>  "<app><TAB><folder it runs from>", empty
#                                    when stopped
#   jungleforge.sh run <folder> [app...]       starts the shared services,
#                                    then the apps (all by default) on
#                                    <folder>; installs packages when its lock
#                                    files changed since its last install
#   jungleforge.sh stop <folder> [app...]
#   jungleforge.sh services          starts the shared services
#   jungleforge.sh info              a table of every container, services and
#                                    apps, running or not: status, image, ports
#   jungleforge.sh settings <folder> <app>   "<KEY><TAB><value><TAB><comment>"
#                                    per line of the app's deploy/.docker-env
#                                    in <folder> (the one that branch runs with)
#   jungleforge.sh settings shared   the same for jungleforge's .env, without
#                                    the registry, passwords masked
#   jungleforge.sh set <folder> <app> <KEY> <value>
#   jungleforge.sh set shared <KEY> <value>
#                                    rewrites that line, keeping its comment; a
#                                    *_PORT must be a number no other app or
#                                    shared service uses
#   jungleforge.sh running-folders <folder>...   the folders apps run from,
#                                    among those folders' repos (the sidebar)
#   jungleforge.sh migrate|logs|shell <folder> <app>    on the running branch
#
# Setting jungleforge up for a user who has none (the installer and the App
# tab's Set up jungleforge / Register in jungleforge):
#   jungleforge.sh available         jungleforge is checked out and has .env
#   jungleforge.sh folder            where it is, or where setup puts it
#   jungleforge.sh setup             clones it (DEVWS_JUNGLEFORGE_REPO), writes
#                                    .env with generated passwords, checks Docker
#   jungleforge.sh templates         "<template><TAB><stack>" per deploy template
#   jungleforge.sh scaffold <app folder> <template> <app>   copies the template
#                                    to <app folder>/deploy, named after the app,
#                                    on a free port; prints the port (none for
#                                    a stack without APP_HTTP_PORT)
#   jungleforge.sh register <app> <folder>   adds or replaces <APP>_PATH
#
# DEVWS_ENV_FILE replaces devws's .env, for tests.

set -u

die() {
  printf 'devws jungleforge: %s\n' "$1" >&2
  exit 2
}

script_dir="$(cd "$(dirname "$0")" && pwd)"
# The environment wins over .env, as in github_repo.sh.
jungleforge_from_environment="${DEVWS_JUNGLEFORGE:-}"
home_from_environment="${DEVWS_HOME:-}"
projects_from_environment="${DEVWS_PROJECTS:-}"
env_file="${DEVWS_ENV_FILE:-$script_dir/.env}"
# shellcheck source=/dev/null
[ -r "$env_file" ] && . "$env_file"
DEVWS_JUNGLEFORGE="${jungleforge_from_environment:-${DEVWS_JUNGLEFORGE:-}}"
DEVWS_HOME="${home_from_environment:-${DEVWS_HOME:-~/.devws}}"
DEVWS_PROJECTS="${projects_from_environment:-${DEVWS_PROJECTS:-~/Projects}}"

expand_home() {
  case "$1" in
    "~") printf '%s' "$HOME" ;;
    "~/"*) printf '%s' "$HOME/${1#\~/}" ;;
    *) printf '%s' "$1" ;;
  esac
}

# DEVWS_JUNGLEFORGE when set; else a checkout in DEVWS_HOME (where setup
# clones) or, as devws had it before, in DEVWS_PROJECTS; else DEVWS_HOME's,
# for setup to fill.
location() {
  local candidate
  if [ -n "$DEVWS_JUNGLEFORGE" ]; then
    candidate="$(expand_home "$DEVWS_JUNGLEFORGE")"
    printf '%s' "${candidate%/}"
    return
  fi
  for candidate in "$(expand_home "$DEVWS_HOME")/jungleforge" "$(expand_home "$DEVWS_PROJECTS")/jungleforge"; do
    if [ -f "$candidate/Makefile" ]; then
      printf '%s' "$candidate"
      return
    fi
  done
  printf '%s/jungleforge' "$(expand_home "$DEVWS_HOME")"
}

available() {
  local folder
  folder="$(location)"
  [ -f "$folder/Makefile" ] && [ -r "$folder/.env" ]
}

jungleforge_folder() {
  local folder
  folder="$(location)"
  [ -r "$folder/.env" ] ||
    die "jungleforge is not set up at $folder: devws menu › App › Set up jungleforge, or ./install.sh --jungleforge"
  printf '%s' "$folder"
}

# phi-planner ↔ PHI_PLANNER_PATH, jungleforge's own naming rule.
path_variable() {
  printf '%s_PATH' "$(printf '%s' "$1" | tr 'a-z-' 'A-Z_')"
}

# "<app><TAB><folder>" per registry entry.
registry() {
  local line variable folder
  while IFS= read -r line; do
    [[ "$line" =~ ^([A-Z0-9_]+)_PATH=(.*)$ ]] || continue
    variable="${BASH_REMATCH[1]}"
    folder="${BASH_REMATCH[2]}"
    folder="${folder#[\"\']}"
    folder="${folder%[\"\']}"
    printf '%s\t%s\n' "$(printf '%s' "$variable" | tr 'A-Z_' 'a-z-')" "$(expand_home "$folder")"
  done < "$(jungleforge_folder)/.env"
}

origin_of() {
  local origin
  origin="$(git -C "$1" remote get-url origin 2>/dev/null)" || return 1
  "$script_dir/github_repo.sh" parse "$origin" 2>/dev/null || printf '%s\n' "$origin"
}

# "<app><TAB><subfolder><TAB><registry folder><TAB><its checkout>"
app_rows() {
  local wanted app folder top subfolder
  wanted="$(origin_of "$1")" || return 0
  registry | while IFS=$'\t' read -r app folder; do
    top="$(git -C "$folder" rev-parse --show-toplevel 2>/dev/null)" || continue
    [ "$(origin_of "$top")" = "$wanted" ] || continue
    subfolder="${folder#"$top"}"
    printf '%s\t%s\t%s\t%s\n' "$app" "${subfolder#/}" "$folder" "$top"
  done
}

apps() {
  local rows
  rows="$(chosen_apps "$1")" || exit 2
  cut -f1,2 <<<"$rows"
}

# Every app needs registering; asked for by name, the name must be the repo's.
chosen_apps() {
  local folder="$1" rows app
  shift
  rows="$(app_rows "$folder")"
  [ -n "$rows" ] || die "Not a jungleforge project: register it in $(jungleforge_folder)/.env"
  if [ "$#" -eq 0 ]; then
    printf '%s\n' "$rows"
    return 0
  fi
  for app in "$@"; do
    awk -F '\t' -v app="$app" '$1 == app { print; found = 1 } END { exit !found }' <<<"$rows" ||
      die "$app is not an app of this repo"
  done
}

targets() {
  local main
  main="$(app_rows "$1" | head -n 1 | cut -f4)"
  [ -n "$main" ] || return 0
  printf 'main\t%s\n' "$main"
  git -C "$1" worktree list --porcelain 2>/dev/null | awk -v main="$main" '
    /^worktree / { folder = substr($0, 10) }
    /^branch refs\/heads\// && folder != main { printf "%s\t%s\n", substr($0, 19), folder }'
}

# The container named in the app's deploy/.docker-env, as jungleforge's
# picker finds it.
container_of() {
  local env_file="$1/deploy/.docker-env" name
  name="$(sed -n 's/^PHP_CONTAINER=//p' "$env_file" 2>/dev/null | head -n 1)"
  [ -n "$name" ] || name="$(sed -n 's/^APP_CONTAINER=//p' "$env_file" 2>/dev/null | head -n 1)"
  name="${name#[\"\']}"
  printf '%s' "${name%[\"\']}"
}

# The target folder whose <subfolder> the running container mounts.
running_folder() {
  local registry_folder="$1" subfolder="$2" targets_list="$3" container inspected source label folder
  container="$(container_of "$registry_folder")"
  [ -n "$container" ] || return 0
  inspected="$(docker inspect -f '{{.State.Running}}{{range .Mounts}}|{{.Source}}{{end}}' "$container" 2>/dev/null)" ||
    return 0
  [ "${inspected%%|*}" = true ] || return 0
  while IFS=$'\t' read -r label folder; do
    for source in $(printf '%s' "${inspected#*|}" | tr '|' '\n'); do
      [ "$source" = "$folder${subfolder:+/$subfolder}" ] && { printf '%s' "$folder"; return 0; }
    done
  done <<<"$targets_list"
}

running() {
  local targets_list app subfolder registry_folder top
  targets_list="$(targets "$1")"
  app_rows "$1" | while IFS=$'\t' read -r app subfolder registry_folder top; do
    printf '%s\t%s\n' "$app" "$(running_folder "$registry_folder" "$subfolder" "$targets_list")"
  done
}

# Every folder an app runs from, for the repos of the folders given (the
# sidebar's workspaces); a repo with several worktrees is asked once.
running_folders() {
  local folder origin seen=" "
  # Without jungleforge nothing runs: no error for the sidebar to report.
  available || return 0
  for folder in "$@"; do
    origin="$(origin_of "$folder")" || continue
    case "$seen" in *" $origin "*) continue ;; esac
    seen="$seen$origin "
    running "$folder" | cut -f2
  done | grep . | LC_ALL=C sort -u
}

jungleforge_make() {
  make -C "$(jungleforge_folder)" "$@"
}

# Run and stop are background jobs (job.sh) that end long after the menu
# closed: the sidebars ask Docker again now for their ▶ running marks.
refresh_running_marks() {
  [ -n "${TMUX:-}" ] || return 0
  tmux set-option -gu @devws_states_checked 2>/dev/null
  "$script_dir/refresh_devws_pickers.sh" --reload >/dev/null 2>&1
  return 0
}

# The lock files' fingerprint, kept per app in the folder's git dir after an
# install, so a switch installs only when the branch changes its packages.
deps_file() {
  printf '%s/devws-deps-%s' "$(git -C "$1" rev-parse --path-format=absolute --git-dir)" "$2"
}

lock_fingerprint() {
  cat "$1/composer.lock" "$1/package-lock.json" 2>/dev/null | shasum | cut -d ' ' -f1
}

run() {
  local folder="$1" rows app subfolder registry_folder top app_folder fingerprint
  shift
  rows="$(chosen_apps "$folder" "$@")" || exit 2
  jungleforge_make services-start || die "the shared services did not start"
  while IFS=$'\t' read -r app subfolder registry_folder top; do
    app_folder="$folder${subfolder:+/$subfolder}"
    [ -d "$app_folder" ] || die "$app_folder does not exist on this branch"
    jungleforge_make start "$app" "$(path_variable "$app")=$app_folder" || die "$app did not start"
    fingerprint="$(lock_fingerprint "$app_folder")"
    [ "$(cat "$(deps_file "$folder" "$app")" 2>/dev/null)" = "$fingerprint" ] && continue
    jungleforge_make deps "$app" "$(path_variable "$app")=$app_folder" || die "$app's packages did not install"
    printf '%s\n' "$fingerprint" > "$(deps_file "$folder" "$app")"
  done <<<"$rows"
}

stop() {
  local folder="$1" rows app rest
  shift
  rows="$(chosen_apps "$folder" "$@")" || exit 2
  while IFS=$'\t' read -r app rest; do
    jungleforge_make stop "$app" || die "$app did not stop"
  done <<<"$rows"
}

# migrate, logs and shell act on the branch the app runs from: migrate starts
# a container of its own, which must mount that branch.
on_running_app() {
  local target="$1" folder="$2" app="$3" row subfolder registry_folder running_root
  row="$(chosen_apps "$folder" "$app")" || exit 2
  IFS=$'\t' read -r app subfolder registry_folder _ <<<"$row"
  running_root="$(running_folder "$registry_folder" "$subfolder" "$(targets "$folder")")"
  [ -n "$running_root" ] || die "$app is not running; run a branch first"
  jungleforge_make "$target" "$app" "$(path_variable "$app")=$running_root${subfolder:+/$subfolder}"
}

check_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    printf 'Docker is missing: install Docker Desktop (https://www.docker.com/products/docker-desktop/) to run apps.\n' >&2
  elif ! docker info >/dev/null 2>&1; then
    printf 'Docker is not running: start Docker Desktop before running an app.\n' >&2
  fi
  return 0
}

# .env from .env.example: every change_me gets a password of its own, and the
# example's placeholder registry entry is left commented out.
write_env() {
  local folder="$1" line
  [ -r "$folder/.env.example" ] || die "$folder has no .env.example"
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      [A-Z]*_PATH=/absolute/path/*) line="# $line" ;;
      *change_me*) line="${line//change_me/$(openssl rand -hex 16)}" ;;
    esac
    printf '%s\n' "$line"
  done < "$folder/.env.example" > "$folder/.env"
  chmod 600 "$folder/.env"
}

setup() {
  local folder repo="${DEVWS_JUNGLEFORGE_REPO:-https://github.com/IvaStival/jungleForge.git}"
  folder="$(location)"
  if [ -f "$folder/Makefile" ]; then
    printf 'jungleforge is at %s\n' "$folder"
  elif [ -n "$(ls -A "$folder" 2>/dev/null)" ]; then
    die "$folder is not empty and is not jungleforge; set DEVWS_JUNGLEFORGE to another folder"
  else
    mkdir -p "$(dirname "$folder")" || die "could not create $(dirname "$folder")"
    git clone --quiet "$repo" "$folder" || die "could not clone $repo"
    printf 'Cloned jungleforge into %s\n' "$folder"
  fi
  if [ ! -e "$folder/.env" ]; then
    write_env "$folder"
    printf 'Wrote %s/.env with generated passwords\n' "$folder"
  fi
  check_docker
  printf 'Next: register a repo (devws menu › App › Register in jungleforge). Its first Run builds jungleforge'"'"'s base image, which takes a few minutes.\n'
}

# "<template><TAB><stack>", sorted so deploy, the default php stack, leads.
templates() {
  local folder template stack
  folder="$(jungleforge_folder)" || exit 2
  for template in "$folder"/templates/deploy*/; do
    [ -d "$template" ] || continue
    template="${template%/}"
    stack="$(sed -n 's/^STACK=\([a-z]*\).*/\1/p' "$template/.docker-env.example" 2>/dev/null | head -n 1)"
    printf '%s\t%s\n' "$(basename "$template")" "${stack:-php}"
  done | LC_ALL=C sort
}

# "<KEY><TAB><value><TAB><comment>" per active line of a settings file; a
# comment is what follows a " #".
setting_rows() {
  awk '
    /^[A-Z][A-Z0-9_]*=/ {
      key = substr($0, 1, index($0, "=") - 1)
      rest = substr($0, index($0, "=") + 1)
      comment = ""
      if (match(rest, /[ \t]+#/)) {
        comment = substr(rest, RSTART + RLENGTH)
        rest = substr(rest, 1, RSTART - 1)
      }
      sub(/^[ \t]+/, "", comment)
      printf "%s\t%s\t%s\n", key, rest, comment
    }' "$1"
}

# "<owner><TAB><key><TAB><port>" per host port in use: each registered app's
# APP_HTTP_PORT and VITE_PORT (in its registry folder), and the shared
# services' *_PORT (owner "shared").
port_owners() {
  local app folder
  while IFS=$'\t' read -r app folder; do
    setting_rows "$folder/deploy/.docker-env" 2>/dev/null | awk -F '\t' -v OFS='\t' -v app="$app" \
      '$1 == "APP_HTTP_PORT" || $1 == "VITE_PORT" { print app, $1, $2 }'
  done < <(registry)
  setting_rows "$(jungleforge_folder)/.env" | awk -F '\t' -v OFS='\t' '$1 ~ /_PORT$/ { print "shared", $1, $2 }'
}

# One past the highest host port a registered app uses, from 8090.
next_port() {
  local highest
  highest="$(port_owners | awk -F '\t' '$2 == "APP_HTTP_PORT" && $3 + 0 > highest { highest = $3 + 0 }
    END { print (highest > 8089 ? highest : 8089) }')"
  printf '%s' "$((highest + 1))"
}

# The file a setting is in: <folder>'s copy of the app's deploy/.docker-env
# (a branch runs with its own), or, for "shared", jungleforge's .env.
settings_file() {
  local folder="$1" app="${2:-}" row subfolder
  if [ "$folder" = shared ]; then
    printf '%s/.env' "$(jungleforge_folder)"
    return
  fi
  row="$(chosen_apps "$folder" "$app")" || exit 2
  subfolder="$(cut -f2 <<<"$row")"
  printf '%s/deploy/.docker-env' "$folder${subfolder:+/$subfolder}"
}

# The shared ones without the registry (Register's), passwords masked.
settings() {
  local file
  file="$(settings_file "$@")" || exit 2
  [ -r "$file" ] || die "$file does not exist"
  if [ "$1" = shared ]; then
    setting_rows "$file" | awk -F '\t' -v OFS='\t' '$1 ~ /_PATH$/ { next } $1 ~ /PASSWORD|SECRET/ { $2 = "••••" } { print }'
  else
    setting_rows "$file"
  fi
}

# A port is a number no other app or service has; the owner's own key may
# keep it.
check_port() {
  local owner="$1" key="$2" port="$3" taken
  case "$port" in '' | *[!0-9]*) die "a port is a number: $port" ;; esac
  [ "$port" -ge 1 ] && [ "$port" -le 65535 ] || die "a port is from 1 to 65535: $port"
  taken="$(port_owners | awk -F '\t' -v owner="$owner" -v key="$key" -v port="$port" '
    $3 == port && !($1 == owner && $2 == key) { print ($1 == "shared" ? "the shared " $2 : $1 "\047s " $2); exit }')"
  [ -z "$taken" ] || die "$port is already $taken"
}

#   set <folder> <app> <KEY> <value>  |  set shared <KEY> <value>
set_setting() {
  local file owner key value
  if [ "${1:-}" = shared ]; then
    [ "$#" -eq 3 ] || die "usage: jungleforge.sh set shared <KEY> <value>"
    owner=shared key="$2" value="$3"
    case "$key" in *_PATH) die "$key is the registry's: Register in jungleforge changes it" ;; esac
  else
    [ "$#" -eq 4 ] || die "usage: jungleforge.sh set <folder> <app> <KEY> <value>"
    owner="$2" key="$3" value="$4"
  fi
  file="$(settings_file "$1" "${2:-}")" || exit 2
  [ -r "$file" ] || die "$file does not exist"
  setting_rows "$file" | cut -f1 | grep -qx "$key" || die "$key is not a setting in $file"
  # The files are sourced and read with sed: a value is one word.
  case "$value" in *[[:space:]#]*) die "a value has no spaces or #: $value" ;; esac
  case "$key" in *_PORT) check_port "$owner" "$key" "$value" ;; esac
  rewrite_setting "$file" "$key" "$value"
}

scaffold() {
  local app_folder="$1" template="$2" app="$3" source port
  source="$(jungleforge_folder)/templates/$template"
  [ -d "$source" ] || die "no template $template in $(jungleforge_folder)/templates"
  [ -d "$app_folder" ] || die "$app_folder does not exist"
  [ ! -e "$app_folder/deploy" ] || die "$app_folder/deploy already exists"
  port="$(next_port)"
  cp -R "$source" "$app_folder/deploy" || die "could not copy $template to $app_folder/deploy"
  sed -e "s/^IMAGE_NAME=[^ #]*/IMAGE_NAME=$app/" \
    -e "s/^PHP_CONTAINER=[^ #]*/PHP_CONTAINER=$app/" \
    -e "s/^APP_CONTAINER=[^ #]*/APP_CONTAINER=$app/" \
    -e "s/^APP_HTTP_PORT=[0-9]*/APP_HTTP_PORT=$port/" \
    "$app_folder/deploy/.docker-env.example" > "$app_folder/deploy/.docker-env" ||
    die "could not write $app_folder/deploy/.docker-env"
  # Stacks without a host port of their own (node uses VITE_PORT) print none.
  grep -q '^APP_HTTP_PORT=' "$app_folder/deploy/.docker-env" && printf '%s\n' "$port"
  return 0
}

# A setting's line rewritten, its trailing comment kept; in place (printf,
# not mv) so the file keeps its permissions.
rewrite_setting() {
  local file="$1" key="$2" value="$3" updated
  updated="$(awk -v key="$key" -v value="$value" '
    index($0, key "=") == 1 {
      rest = substr($0, length(key) + 2)
      print key "=" value (match(rest, /[ \t]+#/) ? substr(rest, RSTART) : "")
      next
    }
    { print }' "$file")" || die "could not read $file"
  printf '%s\n' "$updated" > "$file"
}

register() {
  local app="$1" folder="$2" env_file variable
  case "$app" in '' | *[!a-z0-9-]* | -* | *-) die "an app name is lowercase letters, digits and dashes: $app" ;; esac
  [ -d "$folder" ] || die "$folder does not exist"
  env_file="$(jungleforge_folder)/.env"
  variable="$(path_variable "$app")"
  if grep -q "^$variable=" "$env_file"; then
    rewrite_setting "$env_file" "$variable" "$folder"
  else
    printf '%s=%s\n' "$variable" "$folder" >> "$env_file"
  fi
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  available) available ;;
  folder) location; printf '\n' ;;
  setup) setup ;;
  templates) templates ;;
  scaffold) [ "$#" -eq 3 ] || die "usage: jungleforge.sh scaffold <app folder> <template> <app>"; scaffold "$@" ;;
  register) [ "$#" -eq 2 ] || die "usage: jungleforge.sh register <app> <folder>"; register "$@" ;;
  apps | targets | running)
    [ -n "${1:-}" ] || die "no folder given"
    "$command" "$1"
    ;;
  run | stop)
    [ -d "${1:-}" ] || die "no folder given"
    "$command" "$@"
    status=$?
    refresh_running_marks
    exit "$status"
    ;;
  services) jungleforge_make services-start ;;
  info) jungleforge_make --no-print-directory info ;;
  settings)
    { [ "${1:-}" = shared ] && [ "$#" -eq 1 ]; } || { [ "$#" -eq 2 ] && [ "$1" != shared ]; } ||
      die "usage: jungleforge.sh settings <folder> <app> | settings shared"
    settings "$@"
    ;;
  set) set_setting "$@" ;;
  running-folders) running_folders "$@" ;;
  migrate | logs) [ "$#" -eq 2 ] || die "usage: jungleforge.sh $command <folder> <app>"; on_running_app "$command" "$1" "$2" ;;
  shell) [ "$#" -eq 2 ] || die "usage: jungleforge.sh shell <folder> <app>"; on_running_app sh "$1" "$2" ;;
  *) die "usage: jungleforge.sh <apps|targets|running|run|stop|services|info|settings|set|migrate|logs|shell|available|folder|setup|templates|scaffold|register> …" ;;
esac
