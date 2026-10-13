#!/usr/bin/env bash
# Runs Harlequin in a devws database pane, connected to the database
# configured in the project's .env.
#
#   database_runner.sh <project_root>

set -u

script_dir="$(dirname "$0")"
root="${1:-$PWD}"

tmux set-option -p -t "$TMUX_PANE" @devws_role database

if ! command -v harlequin >/dev/null 2>&1; then
  printf 'Harlequin is not installed; run ./install.sh --deps.\nPress Enter to close.'
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

# A TOML string; "$${" keeps Harlequin from reading ${ as a variable.
toml_string() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//\$\{/\$\$\{}"
  printf '"%s"' "$value"
}

# The password travels in the environment: Harlequin resolves
# ${DEVWS_DB_PASSWORD} in its config, so the password never appears in `ps`
# output or on disk. The throwaway config is removed once Harlequin has read it.
config_dir="$(mktemp -d "${TMPDIR:-/tmp}/devws-harlequin.XXXXXX")" || exit 1
# Only EXIT is trapped: bash still runs it when tmux closes the pane with
# SIGHUP, and an explicit HUP trap would make bash wait for Harlequin, which
# never receives the hang-up and would be left running.
trap 'rm -rf "$config_dir"' EXIT
config="$config_dir/harlequin.toml"
{
  printf '[profiles.devws]\n'
  printf 'adapter = %s\n' "$(toml_string "$database_provider")"
  # Matches LazyVim's colours.
  printf 'theme = "tokyo-night"\n'
  if [ "$database_provider" = sqlite ]; then
    printf 'conn_str = [%s]\n' "$(toml_string "$database_file")"
  else
    printf 'host = %s\n' "$(toml_string "${database_host:-127.0.0.1}")"
    [ -n "$database_port" ] && printf 'port = %s\n' "$(toml_string "$database_port")"
    [ -n "$database_user" ] && printf 'user = %s\n' "$(toml_string "$database_user")"
    # The MySQL adapter calls it "database", the Postgres one "dbname".
    if [ -n "$database_name" ]; then
      if [ "$database_provider" = mysql ]; then
        printf 'database = %s\n' "$(toml_string "$database_name")"
      else
        printf 'dbname = %s\n' "$(toml_string "$database_name")"
      fi
    fi
    [ -n "$database_password" ] && printf 'password = "${DEVWS_DB_PASSWORD}"\n'
  fi
} > "$config"
export DEVWS_DB_PASSWORD="$database_password"

(
  for attempt in $(seq 1 100); do
    # The footer ("^q Quit …") starts at the left edge, so it shows at any
    # width; panel titles are cut short in narrow panes.
    if tmux capture-pane -p -t "$TMUX_PANE" 2>/dev/null | grep -q 'Quit'; then
      break
    fi
    sleep 0.1
  done
  rm -rf "$config_dir"
) &

harlequin --config-path "$config" --profile devws
