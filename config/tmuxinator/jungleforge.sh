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
#   jungleforge.sh running-folders <folder>...   the folders apps run from,
#                                    among those folders' repos (the sidebar)
#   jungleforge.sh migrate|logs|shell <folder> <app>    on the running branch

set -u

die() {
  printf 'devws jungleforge: %s\n' "$1" >&2
  exit 2
}

script_dir="$(cd "$(dirname "$0")" && pwd)"
# The environment wins over .env, as in github_repo.sh.
from_environment="${DEVWS_JUNGLEFORGE:-}"
# shellcheck source=/dev/null
[ -r "$script_dir/.env" ] && . "$script_dir/.env"
DEVWS_JUNGLEFORGE="${from_environment:-${DEVWS_JUNGLEFORGE:-}}"

expand_home() {
  case "$1" in
    "~") printf '%s' "$HOME" ;;
    "~/"*) printf '%s' "$HOME/${1#\~/}" ;;
    *) printf '%s' "$1" ;;
  esac
}

jungleforge_folder() {
  local folder
  folder="$(expand_home "${DEVWS_JUNGLEFORGE:-~/Projects/jungleforge}")"
  [ -r "$folder/.env" ] || die "jungleforge not found at $folder (set DEVWS_JUNGLEFORGE in .env)"
  printf '%s' "${folder%/}"
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

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
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
  running-folders) running_folders "$@" ;;
  migrate | logs) [ "$#" -eq 2 ] || die "usage: jungleforge.sh $command <folder> <app>"; on_running_app "$command" "$1" "$2" ;;
  shell) [ "$#" -eq 2 ] || die "usage: jungleforge.sh shell <folder> <app>"; on_running_app sh "$1" "$2" ;;
  *) die "usage: jungleforge.sh <apps|targets|running|run|stop|services|migrate|logs|shell> …" ;;
esac
