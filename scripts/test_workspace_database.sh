#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-database.XXXXXX")"
SCRIPTS="$ROOT/config/tmuxinator"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-database-test-$$"
CLIENT_SOCKET="devws-database-client-$$"
# The same layout dev.yml opens a workspace with: picker, editor, agent, terminal.
DEFAULT_LAYOUT="12f9,200x50,0,0{25x50,0,0,58,174x50,26,0[174x24,26,0,57,174x25,26,25{86x25,26,25,59,87x25,113,25,60}]}"

cleanup() {
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server 2>/dev/null || true
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Workspace database test failed: %s\n' "$*" >&2
  exit 1
}

# Every tmux call, including the scripts' own, goes to an isolated server.
mkdir -p "$TEST_ROOT/bin" "$TEST_ROOT/home" "$TEST_ROOT/project"
cat > "$TEST_ROOT/bin/tmux" <<EOF
#!/usr/bin/env bash
exec "$REAL_TMUX" -L "$SOCKET" -f /dev/null "\$@"
EOF
# Stands in for Harlequin: records its arguments, its config and the password
# it received through the environment, then draws Harlequin's footer, which
# the runner waits for before removing the config.
cat > "$TEST_ROOT/bin/harlequin" <<'EOF'
#!/usr/bin/env bash
log="$CLIENT_LOG_DIR/$$"
printf '%s\n' "$@" > "$log.args"
config=""
previous=""
for argument in "$@"; do
  [ "$previous" = --config-path ] && config="$argument"
  previous="$argument"
done
[ -n "$config" ] && cp "$config" "$log.config"
printf '%s' "${DEVWS_DB_PASSWORD:-}" > "$log.password"
touch "$log.connected"
printf ' ^q Quit  f1 Help\n'
sleep 600
EOF
chmod +x "$TEST_ROOT/bin/tmux" "$TEST_ROOT/bin/harlequin"
ln -s "$SCRIPTS" "$TEST_ROOT/home/.tmuxinator"
mkdir -p "$TEST_ROOT/client" "$TEST_ROOT/tmp"
# The runner's throwaway Harlequin configs land here, so leftovers can be counted.
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" CLIENT_LOG_DIR="$TEST_ROOT/client" \
  TMPDIR="$TEST_ROOT/tmp"
unset TMUX

# --- Connection settings from .env ---------------------------------------

# Prints "provider|file|host|port|name|user|password|problem" for a project
# with the given .env; any further arguments are files created in it first.
connection_for() {
  local env_text="$1"
  shift
  local project="$TEST_ROOT/env-$RANDOM"
  local file
  mkdir -p "$project"
  printf '%s' "$env_text" > "$project/.env"
  for file in "$@"; do
    mkdir -p "$(dirname "$project/$file")"
    : > "$project/$file"
  done
  bash -c '. "$1"; database_connection "$2"; printf "%s|%s|%s|%s|%s|%s|%s|%s" \
    "$database_provider" "$database_file" "$database_host" "$database_port" \
    "$database_name" "$database_user" "$database_password" "$database_problem"' \
    -- "$SCRIPTS/database_url.sh" "$project"
}

test_mysql_from_laravel_env() {
  local result
  result="$(connection_for $'# DB_PASSWORD=old\nexport DB_CONNECTION=mysql\nDB_HOST=db.local\nDB_PORT=3307\nDB_DATABASE="shop"\nDB_USERNAME=root\nDB_PASSWORD="p@ss:w/ord#é"\n')"
  [[ "$result" == 'mysql||db.local|3307|shop|root|p@ss:w/ord#é|' ]] ||
    fail "MySQL settings from .env are wrong: $result"
}

test_postgres_defaults() {
  local result
  result="$(connection_for $'DB_CONNECTION=pgsql\nDB_DATABASE=app # main database\nDB_USERNAME=app\n')"
  [[ "$result" == 'postgres||||app|app||' ]] ||
    fail "Postgres settings without host and port are wrong: $result"
}

test_sqlite_default_file() {
  local result
  result="$(connection_for $'DB_CONNECTION=sqlite\n' database/database.sqlite)"
  [[ "$result" == "sqlite|$TEST_ROOT/env-"*"/database/database.sqlite||||||" ]] ||
    fail "SQLite default path is wrong: $result"
}

test_sqlite_url_forms() {
  local result absolute
  absolute="$TEST_ROOT/absolute.sqlite"
  : > "$absolute"
  for url in "sqlite://$absolute" "file:$absolute?loc=auto" "$absolute"; do
    result="$(connection_for "DATABASE_URL=$url"$'\n')"
    [[ "$result" == "sqlite|$absolute||||||" ]] ||
      fail "SQLite URL $url was not turned into its path: $result"
  done
  result="$(connection_for $'DB_URL=sqlite://data/app.sqlite\n' data/app.sqlite)"
  [[ "$result" == "sqlite|$TEST_ROOT/env-"*"/data/app.sqlite||||||" ]] ||
    fail "a relative sqlite:// URL was not resolved in the project: $result"
}

test_missing_sqlite_file_is_reported() {
  local result
  result="$(connection_for $'DB_CONNECTION=sqlite\nDB_DATABASE=database/typo.sqlite\n')"
  [[ "$result" == "|||||||SQLite database not found: $TEST_ROOT/env-"*"/database/typo.sqlite" ]] ||
    fail "a missing SQLite file was not reported: $result"
}

test_explicit_url_wins() {
  local result
  result="$(connection_for $'DB_CONNECTION=mysql\nDATABASE_URL="postgres://u:p@host/db"\n')"
  [[ "$result" == 'postgres||host||db|u|p|' ]] ||
    fail "DATABASE_URL should override DB_* settings: $result"
}

test_server_url_is_split() {
  local result
  result="$(connection_for $'DATABASE_URL=postgres://app:s%40cret@db.local:6543/main?sslmode=require\n')"
  [[ "$result" == 'postgres||db.local|6543|main|app|s@cret|' ]] ||
    fail "a server URL was not split into its parts: $result"
}

test_no_database_settings() {
  local result
  result="$(connection_for $'APP_NAME=demo\n')"
  [[ "$result" == '|||||||' ]] || fail "a .env without database settings gave: $result"
}

env_dirs_for() {
  bash -c '. "$1"; database_env_dirs "$2"' -- "$SCRIPTS/database_url.sh" "$1" |
    sed "s|^$1|.|" | tr '\n' ' '
}

test_settings_found_in_subfolders() {
  local project="$TEST_ROOT/monorepo"
  mkdir -p "$project/backend" "$project/web" "$project/vendor/lib" "$project/node_modules/x"
  printf 'APP_NAME=demo\n' > "$project/.env"
  printf 'NEXT_PUBLIC_API=x\n' > "$project/web/.env"
  printf 'DB_CONNECTION=sqlite\n' > "$project/backend/.env"
  printf 'DB_CONNECTION=mysql\n' > "$project/vendor/.env"
  printf 'DB_CONNECTION=mysql\n' > "$project/node_modules/.env"
  [[ "$(env_dirs_for "$project")" == './backend ' ]] ||
    fail "database settings in backend/.env were not found: $(env_dirs_for "$project")"

  printf 'DB_CONNECTION=pgsql\n' > "$project/.env"
  [[ "$(env_dirs_for "$project")" == '. ' ]] ||
    fail "the root .env should win over subfolders: $(env_dirs_for "$project")"
}

test_database_folder_choices() {
  local project="$TEST_ROOT/choices"
  local listed
  mkdir -p "$project/apps/api" "$project/web" "$project/node_modules/pkg" \
    "$project/a/b/c/d"
  printf 'APP_NAME=demo\n' > "$project/.env"
  printf 'DB_CONNECTION=pgsql\n' > "$project/apps/api/.env"
  printf 'NEXT_PUBLIC_API=x\n' > "$project/web/.env"
  printf 'DB_CONNECTION=mysql\n' > "$project/node_modules/pkg/.env"
  printf 'DB_CONNECTION=mysql\n' > "$project/a/b/c/d/.env"
  listed="$(bash -c '. "$1"; database_folder_choices "$2"' -- "$SCRIPTS/database_url.sh" "$project" |
    tr '\t\n' ':;')"
  [[ "$listed" == 'apps/api:pgsql;.:no database settings;web:no database settings;' ]] ||
    fail "database folder choices are wrong: $listed"
}

test_remembered_database_folder() {
  local project="$TEST_ROOT/remember"
  run_remember() {
    bash -c '. "$1"; shift; "$@"' -- "$SCRIPTS/database_url.sh" "$@"
  }
  run_remember remember_database_folder "$project" backend
  run_remember remember_database_folder "$TEST_ROOT/other" .
  [[ "$(run_remember remembered_database_folder "$project")" == "$project/backend" ]] ||
    fail "the chosen folder was not remembered"
  [[ "$(run_remember remembered_database_folder "$TEST_ROOT/other")" == "$TEST_ROOT/other" ]] ||
    fail "choosing the project root was not remembered"
  run_remember remember_database_folder "$project" ""
  [[ -z "$(run_remember remembered_database_folder "$project")" ]] ||
    fail "Automatic did not forget the chosen folder"
  [[ "$(run_remember remembered_database_folder "$TEST_ROOT/other")" == "$TEST_ROOT/other" ]] ||
    fail "forgetting one project's folder affected another"
}

# --- Workspaces ----------------------------------------------------------

printf 'DB_CONNECTION=mysql\nDB_DATABASE=shop\nDB_USERNAME=root\nDB_PASSWORD=secret-pass\n' \
  > "$TEST_ROOT/project/.env"

tmux new-session -d -s setup -x 200 -y 50 'sleep 600'
tmux set -g mouse on
sed -n '/^# devws bindings go after the reset/,$p' "$ROOT/config/tmux/tmux.conf" \
  > "$TEST_ROOT/devws.conf"
tmux source-file "$TEST_ROOT/devws.conf"

create_workspace() {
  local session="$1"
  local role pane

  tmux new-session -d -s "$session" -x 200 -y 50 -c "$TEST_ROOT/project" 'sleep 600'
  tmux set-option -t "$session" @devws_agent claude
  tmux set-option -t "$session" @devws_root "$TEST_ROOT/project"
  tmux set-option -w -t "$session:" pane-border-status top
  tmux select-pane -t "$session:" -T devws-picker
  for role in editor agent terminal; do
    pane="$(tmux split-window -t "$session:" -P -F '#{pane_id}' 'sleep 600')"
    tmux set-option -p -t "$pane" @devws_role "$role"
    tmux select-layout -t "$session:" tiled
  done
  tmux select-layout -t "$session:" "$DEFAULT_LAYOUT"
}

pane_id() {
  tmux list-panes -s -t "$1" -F '#{pane_id} #{@devws_role}' |
    awk -v role="$2" '$2 == role { print $1; exit }'
}

field() {
  tmux display-message -p -t "$1" "#{$2}"
}

window_name_of() {
  field "$1" window_name
}

wait_until() {
  local attempt
  for attempt in $(seq 1 30); do
    "$@" && return 0
    sleep 0.2
  done
  return 1
}

database_kind_recorded() {
  [[ -n "$(tmux show-options -pqv -t "$(pane_id "$1" database)" @devws_db_kind)" ]]
}

database_hidden() {
  [[ -n "$(pane_id "$1" minimized-database)" ]]
}

no_client_configs_left() {
  ! ls -d "$TMPDIR"/devws-harlequin.* >/dev/null 2>&1
}

database_connected() {
  ls "$CLIENT_LOG_DIR"/*.connected >/dev/null 2>&1
}

# The log prefix of the client run that connected.
client_log() {
  local log
  log="$(ls "$CLIENT_LOG_DIR"/*.connected | head -n 1)"
  printf '%s' "${log%.connected}"
}

test_runner_connects_without_exposing_password() {
  local session="dev-runner"
  local log
  create_workspace "$session"
  "$SCRIPTS/open_database.sh" "$session"

  wait_until database_connected || fail "the runner did not start Harlequin"
  log="$(client_log)"
  ! grep -q 'secret-pass' "$log.args" ||
    fail "the password was passed on Harlequin's command line"
  [[ "$(tr '\n' ' ' < "$log.args")" == "--config-path $TMPDIR/devws-harlequin."*"/harlequin.toml --profile devws " ]] ||
    fail "Harlequin was not started with the devws profile: $(tr '\n' ' ' < "$log.args")"
  grep -qx '\[profiles.devws\]' "$log.config" &&
    grep -qx 'adapter = "mysql"' "$log.config" &&
    grep -qx 'host = "127.0.0.1"' "$log.config" &&
    grep -qx 'user = "root"' "$log.config" &&
    grep -qx 'database = "shop"' "$log.config" ||
    fail "the Harlequin profile for MySQL is wrong: $(cat "$log.config")"
  grep -qxF 'password = "${DEVWS_DB_PASSWORD}"' "$log.config" ||
    fail "the profile does not read the password from the environment"
  ! grep -q 'secret-pass' "$log.config" || fail "the password was written to the config"
  [[ "$(cat "$log.password")" == secret-pass ]] ||
    fail "Harlequin did not receive the password through the environment"
  grep -qx 'theme = "tokyo-night"' "$log.config" || fail "the Tokyo Night theme is not set"
  [[ "$(tmux show-options -pqv -t "$(pane_id "$session" database)" @devws_db_kind)" == mysql ]] ||
    fail "the database pane does not record its kind"
  wait_until no_client_configs_left ||
    fail "the throwaway Harlequin config was not removed once read"
}

test_postgres_profile() {
  local session="dev-postgres"
  local project="$TEST_ROOT/pg-project"
  local log
  mkdir -p "$project"
  printf 'DATABASE_URL=postgres://app:s%%40cret@db.local:6543/main\n' > "$project/.env"
  rm -f "$CLIENT_LOG_DIR"/*
  create_workspace "$session"
  tmux set-option -t "$session" @devws_root "$project"
  "$SCRIPTS/open_database.sh" "$session" --no-focus

  wait_until database_connected || fail "the runner did not start Harlequin for Postgres"
  log="$(client_log)"
  grep -qx 'adapter = "postgres"' "$log.config" &&
    grep -qx 'host = "db.local"' "$log.config" &&
    grep -qx 'port = "6543"' "$log.config" &&
    grep -qx 'dbname = "main"' "$log.config" &&
    grep -qx 'user = "app"' "$log.config" ||
    fail "the Harlequin profile for Postgres is wrong: $(cat "$log.config")"
  [[ "$(cat "$log.password")" == 's@cret' ]] ||
    fail "the decoded Postgres password did not reach Harlequin"
}

test_no_settings_shows_how_to_pick_a_folder() {
  local session="dev-empty"
  local project="$TEST_ROOT/no-database"
  mkdir -p "$project"
  create_workspace "$session"
  tmux set-option -t "$session" @devws_root "$project"
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  sleep 1
  grep -q 'Database folder' <<< "$(tmux capture-pane -p -J -t "$(pane_id "$session" database)")" ||
    fail "without database settings the pane does not point at the Database folder command"
}

test_runner_asks_between_several_databases() {
  local session="dev-choose"
  local project="$TEST_ROOT/two-apps"
  local log
  mkdir -p "$project/api/database" "$project/admin/database"
  printf 'DB_CONNECTION=sqlite\n' > "$project/api/.env"
  printf 'DB_CONNECTION=sqlite\n' > "$project/admin/.env"
  : > "$project/api/database/database.sqlite"
  : > "$project/admin/database/database.sqlite"
  # Picks the last folder offered, and records what was offered.
  printf '#!/usr/bin/env bash\ntee "%s" | tail -n 1\n' "$TEST_ROOT/fzf-offered" > "$TEST_ROOT/bin/fzf"
  chmod +x "$TEST_ROOT/bin/fzf"
  rm -f "$CLIENT_LOG_DIR"/*

  create_workspace "$session"
  tmux set-option -t "$session" @devws_root "$project"
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  wait_until database_connected || fail "the runner did not connect after the choice"
  rm -f "$TEST_ROOT/bin/fzf"

  [[ "$(tr '\n' ' ' < "$TEST_ROOT/fzf-offered")" == 'admin api ' ]] ||
    fail "the choice did not offer both folders: $(cat "$TEST_ROOT/fzf-offered")"
  log="$(client_log)"
  grep -qxF "conn_str = [\"$project/api/database/database.sqlite\"]" "$log.config" &&
    grep -qx 'adapter = "sqlite"' "$log.config" ||
    fail "Harlequin did not open the chosen database: $(cat "$log.config")"
}

test_remembered_folder_and_reconnect() {
  local session="dev-remembered"
  local project="$TEST_ROOT/two-apps"
  local log
  bash -c '. "$1"; remember_database_folder "$2" admin' -- "$SCRIPTS/database_url.sh" "$project"
  rm -f "$CLIENT_LOG_DIR"/* "$TEST_ROOT/fzf-offered"

  create_workspace "$session"
  tmux set-option -t "$session" @devws_root "$project"
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  wait_until database_connected || fail "the runner did not connect to the remembered folder"
  [[ ! -e "$TEST_ROOT/fzf-offered" ]] || fail "the runner asked although a folder was remembered"
  grep -qF "$project/admin/database/database.sqlite" "$(client_log).config" ||
    fail "Harlequin did not open the remembered folder's database"

  # Picking another folder reconnects the open pane.
  bash -c '. "$1"; remember_database_folder "$2" api' -- "$SCRIPTS/database_url.sh" "$project"
  rm -f "$CLIENT_LOG_DIR"/*
  "$SCRIPTS/open_database.sh" "$session" --reconnect
  wait_until database_connected || fail "the database pane did not reconnect"
  grep -qF "$project/api/database/database.sqlite" "$(client_log).config" ||
    fail "after reconnecting Harlequin did not open the newly chosen folder"
  [[ "$(tmux list-panes -t "$session:" -F '#{@devws_role}' | grep -c '^database$')" == 1 ]] ||
    fail "reconnecting opened a second database pane"
}

test_closing_pane_removes_config() {
  local session="dev-close"
  local runner_pid
  create_workspace "$session"
  # A client that never draws its footer keeps the config until the pane closes.
  mv "$TEST_ROOT/bin/harlequin" "$TEST_ROOT/bin/harlequin.real"
  printf '#!/usr/bin/env bash\nsleep 600\n' > "$TEST_ROOT/bin/harlequin"
  chmod +x "$TEST_ROOT/bin/harlequin"
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  sleep 1
  ls -d "$TMPDIR"/devws-harlequin.* >/dev/null 2>&1 ||
    fail "the runner did not create its config"

  runner_pid="$(field "$(pane_id "$session" database)" pane_pid)"

  tmux kill-pane -t "$(pane_id "$session" database)"
  mv "$TEST_ROOT/bin/harlequin.real" "$TEST_ROOT/bin/harlequin"
  wait_until no_client_configs_left ||
    fail "closing the database pane left its Harlequin config behind"
  ! kill -0 "$runner_pid" 2>/dev/null ||
    fail "closing the database pane left the runner process running"
}

test_stacked_layout_opens_right_of_editor() {
  local session="dev-stacked"
  local editor database
  create_workspace "$session"
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  editor="$(pane_id "$session" editor)"
  database="$(pane_id "$session" database)"

  [[ -n "$database" ]] || fail "no database pane was opened"
  [[ "$(field "$database" pane_top)" == "$(field "$editor" pane_top)" ]] &&
    (( $(field "$database" pane_left) > $(field "$editor" pane_left) )) ||
    fail "in the default layout the database is not right of the editor"
  [[ "$(field "$session:" pane_id)" != "$database" ]] ||
    fail "--no-focus moved focus to the database"
}

test_side_layout_opens_below_editor() {
  local session="dev-side"
  local editor database
  create_workspace "$session"
  "$SCRIPTS/toggle_layout.sh" "$session"
  "$SCRIPTS/open_database.sh" "$session"
  editor="$(pane_id "$session" editor)"
  database="$(pane_id "$session" database)"

  [[ "$(field "$database" pane_left)" == "$(field "$editor" pane_left)" ]] &&
    (( $(field "$database" pane_top) > $(field "$editor" pane_top) )) ||
    fail "in the side layout the database is not below the editor"
  [[ "$(field "$session:" pane_id)" == "$database" ]] ||
    fail "opening the database did not focus it"
}

test_opening_twice_focuses_existing_pane() {
  local session="dev-twice"
  local count
  create_workspace "$session"
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  count="$(tmux list-panes -t "$session:" | grep -c .)"

  "$SCRIPTS/open_database.sh" "$session"
  [[ "$(tmux list-panes -t "$session:" | grep -c .)" == "$count" ]] ||
    fail "opening the database again added another pane"
  [[ "$(field "$session:" pane_id)" == "$(pane_id "$session" database)" ]] ||
    fail "opening the database again did not focus it"
}

test_database_hides_independently() {
  local session="dev-independent"
  local database_width agents_layout
  create_workspace "$session"
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  database_width="$(field "$(pane_id "$session" database)" pane_width)"

  "$SCRIPTS/minimize_panes.sh" "$session" toggle database
  [[ "$(window_name_of "$(pane_id "$session" database)")" == devws-minimized ]] ||
    fail "the database pane was not hidden: $(tmux list-panes -s -t "$session" -F '#{window_name} #{pane_id} #{@devws_role}' | tr '\n' ';')"
  [[ "$(field "$(pane_id "$session" minimized-database)" pane_width)" == 9 ]] ||
    fail "the database beside the editor did not shrink to a strip"
  [[ -z "$(pane_id "$session" minimized-agent)" ]] ||
    fail "hiding the database also hid the agent"

  "$SCRIPTS/minimize_panes.sh" "$session" toggle agents
  agents_layout="$(field "$(pane_id "$session" minimized-agent)" pane_height)"
  "$SCRIPTS/minimize_panes.sh" "$session" toggle database
  [[ "$(field "$(pane_id "$session" database)" pane_width)" == "$database_width" ]] ||
    fail "the database did not come back at its previous width"
  [[ -n "$(pane_id "$session" minimized-agent)" ]] &&
    [[ "$(field "$(pane_id "$session" minimized-agent)" pane_height)" == "$agents_layout" ]] ||
    fail "restoring the database also expanded the agent"

  "$SCRIPTS/minimize_panes.sh" "$session" toggle agents
  [[ -z "$(pane_id "$session" minimized-agent)" ]] &&
    ! tmux list-windows -t "$session" -F '#{window_name}' | grep -qx devws-minimized ||
    fail "restoring both groups left placeholders or the hidden window behind"
}

test_database_below_editor_hides_to_one_line() {
  local session="dev-side-hide"
  create_workspace "$session"
  "$SCRIPTS/toggle_layout.sh" "$session"
  "$SCRIPTS/open_database.sh" "$session" --no-focus

  "$SCRIPTS/minimize_panes.sh" "$session" toggle database
  [[ "$(field "$(pane_id "$session" minimized-database)" pane_height)" == 1 ]] ||
    fail "the database below the editor did not shrink to one line"
}

test_layout_toggle_moves_database() {
  local session="dev-toggle"
  local editor database
  create_workspace "$session"
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  "$SCRIPTS/minimize_panes.sh" "$session" toggle database

  "$SCRIPTS/toggle_layout.sh" "$session" || fail "toggling with a database pane failed"
  editor="$(pane_id "$session" editor)"
  database="$(pane_id "$session" database)"
  [[ "$(window_name_of "$database")" != devws-minimized ]] ||
    fail "the layout toggle did not expand the hidden database"
  [[ "$(field "$database" pane_left)" == "$(field "$editor" pane_left)" ]] &&
    (( $(field "$database" pane_top) > $(field "$editor" pane_top) )) ||
    fail "after switching to the side layout the database is not below the editor"
  [[ "$(field "$editor" pane_top)" == "$(field "$(pane_id "$session" agent)" pane_top)" ]] ||
    fail "the agent did not move to the right column"

  "$SCRIPTS/toggle_layout.sh" "$session"
  editor="$(pane_id "$session" editor)"
  database="$(pane_id "$session" database)"
  [[ "$(field "$database" pane_top)" == "$(field "$editor" pane_top)" ]] &&
    (( $(field "$database" pane_left) > $(field "$editor" pane_left) )) ||
    fail "after switching back the database is not right of the editor"
}

# A real client, running inside a second tmux server, so headers are drawn
# and mouse events go through tmux's own key bindings.
test_database_header_click() {
  local session="dev-mouse"
  local row screen
  create_workspace "$session"
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  wait_until database_kind_recorded "$session" || fail "the database kind was never recorded"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" -f /dev/null new-session -d -s client -x 200 -y 50 \
    "TMUX= $REAL_TMUX -L $SOCKET attach -t $session"
  sleep 1

  screen="$("$REAL_TMUX" -L "$CLIENT_SOCKET" capture-pane -p -t client)"
  grep -q 'Database · mysql' <<< "$screen" || fail "the database header is missing"
  row="$(grep -n 'Database · mysql' <<< "$screen" | head -1 | cut -d: -f1)"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" send-keys -t client -l $'\e[<0;150;'"$row"'M'
  "$REAL_TMUX" -L "$CLIENT_SOCKET" send-keys -t client -l $'\e[<0;150;'"$row"'m'
  wait_until database_hidden "$session" ||
    fail "clicking the database header did not hide it"
  [[ -z "$(pane_id "$session" minimized-agent)" ]] ||
    fail "clicking the database header also hid the agent"
  sleep 0.5
  grep -q '▸ DB' <<< "$("$REAL_TMUX" -L "$CLIENT_SOCKET" capture-pane -p -t client)" ||
    fail "the hidden database header does not read ▸ DB"
  "$REAL_TMUX" -L "$CLIENT_SOCKET" kill-server
}

test_mysql_from_laravel_env
test_postgres_defaults
test_sqlite_default_file
test_sqlite_url_forms
test_missing_sqlite_file_is_reported
test_explicit_url_wins
test_server_url_is_split
test_settings_found_in_subfolders
test_database_folder_choices
test_remembered_database_folder
test_no_database_settings
test_runner_connects_without_exposing_password
test_postgres_profile
test_no_settings_shows_how_to_pick_a_folder
test_closing_pane_removes_config
test_runner_asks_between_several_databases
test_remembered_folder_and_reconnect
test_stacked_layout_opens_right_of_editor
test_side_layout_opens_below_editor
test_opening_twice_focuses_existing_pane
test_database_hides_independently
test_database_below_editor_hides_to_one_line
test_layout_toggle_moves_database
test_database_header_click

printf 'Workspace database tests passed.\n'
