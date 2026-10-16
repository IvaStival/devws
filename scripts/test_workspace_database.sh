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
# Stands in for devdb: records its arguments and the connection it received
# through DEVDB_URL, then stays open like the real client.
cat > "$TEST_ROOT/bin/devdb" <<'EOF'
#!/usr/bin/env bash
log="$CLIENT_LOG_DIR/$$"
: > "$log.args"
[ "$#" -eq 0 ] || printf '%s\n' "$@" > "$log.args"
printf '%s' "${DEVDB_URL:-}" > "$log.url"
touch "$log.connected"
sleep 600
EOF
chmod +x "$TEST_ROOT/bin/tmux" "$TEST_ROOT/bin/devdb"
ln -s "$SCRIPTS" "$TEST_ROOT/home/.tmuxinator"
mkdir -p "$TEST_ROOT/client"
export PATH="$TEST_ROOT/bin:$PATH" HOME="$TEST_ROOT/home" CLIENT_LOG_DIR="$TEST_ROOT/client"
unset TMUX

# --- Connection settings from .env ---------------------------------------

# Prints "provider|url|problem" for a project with the given .env; any further
# arguments are files created in it first.
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
  bash -c '. "$1"; database_connection "$2"
    printf "%s|%s|%s" "$database_provider" "$database_url" "$database_problem"' \
    -- "$SCRIPTS/database_url.sh" "$project"
}

test_mysql_from_laravel_env() {
  local result
  result="$(connection_for $'# DB_PASSWORD=old\nexport DB_CONNECTION=mysql\nDB_HOST=db.local\nDB_PORT=3307\nDB_DATABASE="shop"\nDB_USERNAME=root\nDB_PASSWORD="p@ss:w/ord#é"\n')"
  [[ "$result" == 'mysql|mysql://root:p%40ss%3Aw%2Ford%23%C3%A9@db.local:3307/shop|' ]] ||
    fail "MySQL URL from .env is wrong: $result"
}

test_postgres_defaults() {
  local result
  result="$(connection_for $'DB_CONNECTION=pgsql\nDB_DATABASE=app # main database\nDB_USERNAME=app\n')"
  [[ "$result" == 'postgres|postgres://app@127.0.0.1:5432/app|' ]] ||
    fail "Postgres URL with default host and port is wrong: $result"
}

test_mongodb_from_laravel_env() {
  local result
  result="$(connection_for $'DB_CONNECTION=mongodb\nDB_HOST=mongo.local\nDB_DATABASE=app\nDB_USERNAME=admin\nDB_PASSWORD=pw\nDB_AUTHENTICATION_DATABASE=admin\n')"
  [[ "$result" == 'mongodb|mongodb://admin:pw@mongo.local:27017/app?authSource=admin|' ]] ||
    fail "MongoDB URL from .env is wrong: $result"
}

test_mongodb_uri_variables() {
  local variable result
  for variable in MONGODB_URI MONGODB_URL MONGO_URL; do
    result="$(connection_for "$variable=mongodb+srv://u:p@cluster.example.net/app"$'\n')"
    [[ "$result" == 'mongodb|mongodb+srv://u:p@cluster.example.net/app|' ]] ||
      fail "$variable was not used: $result"
  done
}

test_sqlite_default_file() {
  local result
  result="$(connection_for $'DB_CONNECTION=sqlite\n' database/database.sqlite)"
  [[ "$result" == "sqlite|$TEST_ROOT/env-"*"/database/database.sqlite|" ]] ||
    fail "SQLite default path is wrong: $result"
}

test_sqlite_url_forms() {
  local result absolute
  absolute="$TEST_ROOT/absolute.sqlite"
  : > "$absolute"
  for url in "sqlite://$absolute" "file:$absolute?loc=auto" "$absolute"; do
    result="$(connection_for "DATABASE_URL=$url"$'\n')"
    [[ "$result" == "sqlite|$absolute|" ]] ||
      fail "SQLite URL $url was not turned into its path: $result"
  done
  result="$(connection_for $'DB_URL=sqlite://data/app.sqlite\n' data/app.sqlite)"
  [[ "$result" == "sqlite|$TEST_ROOT/env-"*"/data/app.sqlite|" ]] ||
    fail "a relative sqlite:// URL was not resolved in the project: $result"
}

test_missing_sqlite_file_is_reported() {
  local result
  result="$(connection_for $'DB_CONNECTION=sqlite\nDB_DATABASE=database/typo.sqlite\n')"
  [[ "$result" == "||SQLite database not found: $TEST_ROOT/env-"*"/database/typo.sqlite" ]] ||
    fail "a missing SQLite file was not reported: $result"
}

test_explicit_url_wins() {
  local result
  result="$(connection_for $'DB_CONNECTION=mysql\nDATABASE_URL="postgres://u:p@host/db"\n')"
  [[ "$result" == 'postgres|postgres://u:p@host/db|' ]] ||
    fail "DATABASE_URL should override DB_* settings: $result"
}

test_no_database_settings() {
  local result
  result="$(connection_for $'APP_NAME=demo\n')"
  [[ "$result" == '||' ]] || fail "a .env without database settings gave: $result"
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

process_gone() {
  ! kill -0 "$1" 2>/dev/null
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

  wait_until database_connected || fail "the runner did not start devdb"
  log="$(client_log)"
  [[ ! -s "$log.args" ]] ||
    fail "devdb got command-line arguments, which show in ps: $(cat "$log.args")"
  [[ "$(cat "$log.url")" == 'mysql://root:secret-pass@127.0.0.1:3306/shop' ]] ||
    fail "devdb did not receive the connection through DEVDB_URL: $(cat "$log.url")"
  [[ "$(tmux show-options -pqv -t "$(pane_id "$session" database)" @devws_db_kind)" == mysql ]] ||
    fail "the database pane does not record its kind"
}

test_mongodb_project_opens_devdb_on_mongodb() {
  local session="dev-mongo"
  local project="$TEST_ROOT/mongo-project"
  mkdir -p "$project"
  printf 'MONGODB_URI=mongodb://app:pw@127.0.0.1:27017/shop\n' > "$project/.env"
  rm -f "$CLIENT_LOG_DIR"/*
  create_workspace "$session"
  tmux set-option -t "$session" @devws_root "$project"
  "$SCRIPTS/open_database.sh" "$session" --no-focus

  wait_until database_connected || fail "the runner did not start devdb for MongoDB"
  [[ "$(cat "$(client_log).url")" == 'mongodb://app:pw@127.0.0.1:27017/shop' ]] ||
    fail "devdb did not receive the MongoDB URL"
  [[ "$(tmux show-options -pqv -t "$(pane_id "$session" database)" @devws_db_kind)" == mongodb ]] ||
    fail "the database pane does not record mongodb as its kind"
}

test_no_settings_opens_the_connection_setup() {
  local session="dev-empty"
  local project="$TEST_ROOT/no-database"
  mkdir -p "$project"
  rm -f "$CLIENT_LOG_DIR"/*
  create_workspace "$session"
  tmux set-option -t "$session" @devws_root "$project"
  "$SCRIPTS/open_database.sh" "$session" --no-focus

  wait_until database_connected || fail "devdb was not started for a project without settings"
  [[ "$(tr '\n' ' ' < "$(client_log).args")" == "--project $project " ]] ||
    fail "devdb was not asked to set up the project's connection: $(cat "$(client_log).args")"
  [[ -z "$(cat "$(client_log).url")" ]] || fail "a connection URL was passed for a project without one"
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
  [[ "$(cat "$log.url")" == "$project/api/database/database.sqlite" ]] ||
    fail "devdb did not open the chosen database: $(cat "$log.url")"
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
  [[ "$(cat "$(client_log).url")" == "$project/admin/database/database.sqlite" ]] ||
    fail "devdb did not open the remembered folder's database"

  # Picking another folder reconnects the open pane.
  bash -c '. "$1"; remember_database_folder "$2" api' -- "$SCRIPTS/database_url.sh" "$project"
  rm -f "$CLIENT_LOG_DIR"/*
  "$SCRIPTS/open_database.sh" "$session" --reconnect
  wait_until database_connected || fail "the database pane did not reconnect"
  [[ "$(cat "$(client_log).url")" == "$project/api/database/database.sqlite" ]] ||
    fail "after reconnecting devdb did not open the newly chosen folder"
  [[ "$(tmux list-panes -t "$session:" -F '#{@devws_role}' | grep -c '^database$')" == 1 ]] ||
    fail "reconnecting opened a second database pane"
}

test_closing_pane_ends_devdb() {
  local session="dev-close"
  local client_pid
  create_workspace "$session"
  rm -f "$CLIENT_LOG_DIR"/*
  "$SCRIPTS/open_database.sh" "$session" --no-focus
  wait_until database_connected || fail "the runner did not start devdb"
  client_pid="$(basename "$(client_log)")"

  tmux kill-pane -t "$(pane_id "$session" database)"
  wait_until process_gone "$client_pid" ||
    fail "closing the database pane left devdb running"
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

# A workspace whose agent pane was closed (or whose agent runs by hand in the
# terminal) still opens the database: the terminal tells the layout, and
# without either it goes beside the editor.
test_opens_without_an_agent_pane() {
  local session="dev-no-agent"
  local editor database
  create_workspace "$session"
  "$SCRIPTS/toggle_layout.sh" "$session"
  tmux kill-pane -t "$(pane_id "$session" agent)"
  "$SCRIPTS/open_database.sh" "$session" --no-focus ||
    fail "the database did not open without an agent pane"
  editor="$(pane_id "$session" editor)"
  database="$(pane_id "$session" database)"
  [[ "$(field "$database" pane_left)" == "$(field "$editor" pane_left)" ]] &&
    (( $(field "$database" pane_top) > $(field "$editor" pane_top) )) ||
    fail "without an agent, the side layout did not put the database below the editor"

  session="dev-editor-only"
  create_workspace "$session"
  tmux kill-pane -t "$(pane_id "$session" agent)"
  tmux kill-pane -t "$(pane_id "$session" terminal)"
  "$SCRIPTS/open_database.sh" "$session" --no-focus ||
    fail "the database did not open beside a lone editor"
  editor="$(pane_id "$session" editor)"
  database="$(pane_id "$session" database)"
  [[ "$(field "$database" pane_top)" == "$(field "$editor" pane_top)" ]] &&
    (( $(field "$database" pane_left) > $(field "$editor" pane_left) )) ||
    fail "beside a lone editor the database is not right of it"
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
test_mongodb_from_laravel_env
test_mongodb_uri_variables
test_explicit_url_wins
test_settings_found_in_subfolders
test_database_folder_choices
test_remembered_database_folder
test_no_database_settings
test_runner_connects_without_exposing_password
test_mongodb_project_opens_devdb_on_mongodb
test_no_settings_opens_the_connection_setup
test_closing_pane_ends_devdb
test_runner_asks_between_several_databases
test_remembered_folder_and_reconnect
test_stacked_layout_opens_right_of_editor
test_side_layout_opens_below_editor
test_opens_without_an_agent_pane
test_opening_twice_focuses_existing_pane
test_database_hides_independently
test_database_below_editor_hides_to_one_line
test_layout_toggle_moves_database
test_database_header_click

printf 'Workspace database tests passed.\n'
