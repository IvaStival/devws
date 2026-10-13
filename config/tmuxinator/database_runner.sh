#!/usr/bin/env bash
# Runs devdb in a devws database pane, connected to the database configured in
# the project's .env.
#
#   database_runner.sh <project_root>

set -u

script_dir="$(dirname "$0")"
root="${1:-$PWD}"

tmux set-option -p -t "$TMUX_PANE" @devws_role database

if ! command -v devdb >/dev/null 2>&1; then
  printf 'devdb is not installed; run ./install.sh --deps.\nPress Enter to close.'
  read -r
  exit 127
fi

. "$script_dir/database_url.sh"

# A folder picked with the menu's "Database folder" command wins. Otherwise
# the settings may live in a subfolder (backend/.env); when several folders
# have them, the pane asks which database to open.
env_dir="$(remembered_database_folder "$root")"
[ -n "$env_dir" ] && [ -r "$env_dir/.env" ] || env_dir=""
env_dirs="$(database_env_dirs "$root")"
[ -n "$env_dir" ] || env_dir="$(printf '%s\n' "$env_dirs" | head -n 1)"
if [ -z "$(remembered_database_folder "$root")" ] &&
    [ "$(printf '%s\n' "$env_dirs" | grep -c .)" -gt 1 ] && command -v fzf >/dev/null 2>&1; then
  chosen="$(
    printf '%s\n' "$env_dirs" | sed "s|^$root/||" |
      fzf --height=100% --layout=reverse --prompt='Database > ' \
        --header='Several .env files have database settings'
  )" || exit 0
  env_dir="$root/$chosen"
fi
database_connection "${env_dir:-$root}"

if [ -n "$database_problem" ]; then
  printf '%s\nCheck DB_DATABASE or DATABASE_URL in %s/.env.\nPress Enter to close.' \
    "$database_problem" "${env_dir:-$root}"
  read -r
  exit 1
fi
if [ -z "$database_provider" ]; then
  tmux set-option -p -t "$TMUX_PANE" @devws_db_kind "no database settings"
  printf 'No database settings found in %s or its subfolders.\n' "$root"
  printf 'Pick the folder with the .env under "Database folder" in the workspace menu.\n'
  printf 'Press Enter to close.'
  read -r
  exit 1
fi
tmux set-option -p -t "$TMUX_PANE" @devws_db_kind "$database_provider"

# The URL may hold a password, so it goes in the environment: an argument would
# show in `ps` output.
export DEVDB_URL="$database_url"
exec devdb
