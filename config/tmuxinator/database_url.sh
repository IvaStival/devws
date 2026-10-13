#!/usr/bin/env bash
# Reads a project's database connection from its .env for the database pane,
# which runs devdb. Source this file, pick the folder with database_env_dirs,
# then call database_connection with that folder. It sets database_provider
# (mysql, postgres, sqlite or mongodb) and database_url, the connection devdb
# opens: a SQLite file path or a mysql://, postgres:// or mongodb:// URL. Both
# stay empty when the folder has no usable database settings; for a SQLite file
# that does not exist, database_problem says so instead.

# Variables that hold a whole connection URL, in the order they win.
URL_VARIABLES="DATABASE_URL DB_URL MONGODB_URI MONGODB_URL MONGO_URL"

# Prints the value of one .env key. The file is parsed, never sourced: .env
# values are not shell-safe and must not run as code.
env_value() {
  local env_file="$1"
  local key="$2"
  awk -v key="$key" '
    /^[[:space:]]*#/ { next }
    {
      line = $0
      sub(/^[[:space:]]*export[[:space:]]+/, "", line)
      split_at = index(line, "=")
      if (split_at == 0) next
      name = substr(line, 1, split_at - 1)
      gsub(/[[:space:]]/, "", name)
      if (name != key) next
      value = substr(line, split_at + 1)
      sub(/^[[:space:]]+/, "", value)
      if (value ~ /^"/) { sub(/^"/, "", value); sub(/".*$/, "", value) }
      else if (value ~ /^\047/) { sub(/^\047/, "", value); sub(/\047.*$/, "", value) }
      else { sub(/[[:space:]]+#.*$/, "", value); sub(/[[:space:]]+$/, "", value) }
      found = value
    }
    END { if (found != "") print found }
  ' "$env_file"
}

# Percent-encodes a URL component so passwords with @, :, / or # survive.
url_encode() {
  local text="$1"
  local encoded=""
  local index character code
  # Byte-wise in the C locale, so non-ASCII characters become their UTF-8 bytes.
  local LC_ALL=C
  for (( index = 0; index < ${#text}; index++ )); do
    character="${text:index:1}"
    case "$character" in
      [A-Za-z0-9._~-]) encoded="${encoded}${character}" ;;
      *)
        printf -v code '%d' "'$character"
        # macOS's bash 3.2 reads bytes above 127 as negative numbers.
        encoded="${encoded}$(printf '%%%02X' $(( code & 255 )))"
        ;;
    esac
  done
  printf '%s' "$encoded"
}

provider_for_url() {
  case "$1" in
    mysql://*|mariadb://*) printf 'mysql' ;;
    postgres://*|postgresql://*|pgsql://*|pg://*) printf 'postgres' ;;
    mongodb://*|mongodb+srv://*) printf 'mongodb' ;;
  esac
}

is_sqlite_url() {
  case "$1" in
    sqlite:*|sqlite3:*|file:*|/*) return 0 ;;
  esac
  return 1
}

# Laravel's sqlite:///path form is turned into a plain path. A missing file is
# reported rather than passed on, because opening it would silently create an
# empty database there.
use_sqlite_file() {
  local root="$1"
  local path="$2"

  path="${path#sqlite3:}"
  path="${path#sqlite:}"
  path="${path#file:}"
  path="${path%%\?*}"
  # sqlite:///abs/path keeps its leading slash; sqlite://rel/path is relative.
  path="${path#//}"
  case "$path" in
    /*) ;;
    *) path="$root/$path" ;;
  esac
  if [ ! -f "$path" ]; then
    database_problem="SQLite database not found: $path"
    return 0
  fi
  database_provider="sqlite"
  database_url="$path"
}

# The first connection URL the .env sets, if any.
connection_url_in() {
  local env_file="$1"
  local variable url
  for variable in $URL_VARIABLES; do
    url="$(env_value "$env_file" "$variable")"
    if [ -n "$url" ]; then
      printf '%s' "$url"
      return 0
    fi
  done
}

has_database_settings() {
  local env_file="$1"
  [ -r "$env_file" ] || return 1
  [ -n "$(connection_url_in "$env_file")$(env_value "$env_file" DB_CONNECTION)" ]
}

# Prints the folders whose .env has database settings: the project root when
# it has them, otherwise its direct subfolders, for projects that keep the app
# in e.g. backend/ next to a web/ frontend.
database_env_dirs() {
  local root="$1"
  local dir
  if has_database_settings "$root/.env"; then
    printf '%s\n' "$root"
    return 0
  fi
  for dir in "$root"/*/; do
    dir="${dir%/}"
    case "${dir##*/}" in node_modules|vendor) continue ;; esac
    has_database_settings "$dir/.env" && printf '%s\n' "$dir"
  done
  return 0
}

# Folder choices made with the workspace menu's "Database folder" command,
# one "<project root><TAB><folder relative to it>" line per project.
database_folders_file() {
  printf '%s' "$HOME/.devws-state/database-folders"
}

# Prints the remembered folder for a project as an absolute path, or nothing.
remembered_database_folder() {
  local root="$1"
  local relative
  relative="$(awk -F '\t' -v root="$root" '$1 == root { found = $2 } END { print found }' \
    "$(database_folders_file)" 2>/dev/null)"
  [ -n "$relative" ] || return 0
  if [ "$relative" = . ]; then printf '%s' "$root"; else printf '%s' "$root/$relative"; fi
}

# Remembers a folder for a project; an empty folder goes back to automatic.
remember_database_folder() {
  local root="$1"
  local relative="$2"
  local file temporary
  file="$(database_folders_file)"
  mkdir -p "$(dirname "$file")"
  temporary="$(mktemp "$file.XXXXXX")" || return 1
  awk -F '\t' -v root="$root" '$1 != root' "$file" 2>/dev/null > "$temporary"
  [ -n "$relative" ] && printf '%s\t%s\n' "$root" "$relative" >> "$temporary"
  mv "$temporary" "$file"
}

# Prints "<folder relative to root><TAB><description>" for every folder up to
# three levels down that has a .env, those with database settings first.
database_folder_choices() {
  local root="$1"
  local env_file folder kind
  while IFS= read -r env_file; do
    folder="${env_file%/.env}"
    folder="${folder#$root}"
    folder="${folder#/}"
    [ -n "$folder" ] || folder=.
    kind="$(env_value "$env_file" DB_CONNECTION)"
    [ -n "$kind" ] || kind="$(connection_url_in "$env_file")"
    kind="${kind%%:*}"
    if [ -n "$kind" ]; then
      printf '0\t%s\t%s\n' "$folder" "$kind"
    else
      printf '1\t%s\t%s\n' "$folder" "no database settings"
    fi
  done < <(find "$root" -maxdepth 4 \
      \( -name node_modules -o -name vendor -o -name .git \) -prune -o \
      -name .env -type f -print 2>/dev/null) |
    sort -t "$(printf '\t')" -k1,1 -k2,2 | cut -f2-
}

# Reads <folder>/.env; relative SQLite paths are resolved from that folder,
# as Laravel resolves them from its base path.
database_connection() {
  local root="$1"
  local env_file="$root/.env"
  local url connection port sqlite_file credentials user password name options host

  database_provider=""
  database_url=""
  database_problem=""
  [ -r "$env_file" ] || return 0

  # An explicit URL wins, as it does for Laravel itself.
  url="$(connection_url_in "$env_file")"
  if [ -n "$url" ]; then
    if is_sqlite_url "$url"; then
      use_sqlite_file "$root" "$url"
      return 0
    fi
    database_provider="$(provider_for_url "$url")"
    [ -n "$database_provider" ] && database_url="$url"
    return 0
  fi

  connection="$(env_value "$env_file" DB_CONNECTION)"
  case "$connection" in
    sqlite)
      sqlite_file="$(env_value "$env_file" DB_DATABASE)"
      use_sqlite_file "$root" "${sqlite_file:-database/database.sqlite}"
      return 0
      ;;
    mysql|mariadb) database_provider="mysql"; port=3306 ;;
    pgsql|postgres|postgresql) database_provider="postgres"; port=5432 ;;
    mongodb) database_provider="mongodb"; port=27017 ;;
    *) return 0 ;;
  esac

  [ -n "$(env_value "$env_file" DB_PORT)" ] && port="$(env_value "$env_file" DB_PORT)"
  user="$(env_value "$env_file" DB_USERNAME)"
  password="$(env_value "$env_file" DB_PASSWORD)"
  name="$(env_value "$env_file" DB_DATABASE)"
  credentials=""
  if [ -n "$user" ]; then
    credentials="$(url_encode "$user")"
    [ -n "$password" ] && credentials="${credentials}:$(url_encode "$password")"
    credentials="${credentials}@"
  fi
  options=""
  # Laravel's MongoDB package names the database that holds the user here.
  if [ "$database_provider" = mongodb ] && [ -n "$(env_value "$env_file" DB_AUTHENTICATION_DATABASE)" ]; then
    options="?authSource=$(env_value "$env_file" DB_AUTHENTICATION_DATABASE)"
  fi
  host="$(env_value "$env_file" DB_HOST)"
  database_url="${database_provider}://${credentials}${host:-127.0.0.1}:${port}/${name}${options}"
  return 0
}
