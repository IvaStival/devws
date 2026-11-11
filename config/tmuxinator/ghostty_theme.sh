#!/usr/bin/env bash
# The Ghostty look devws keeps in config/ghostty (iTerm2's "glofi" profile:
# its light and dark colours, font and cursor), put where Ghostty reads it.
#
#   ghostty_theme.sh apply   links the config and the themes; a different
#                            file already there is backed up first, into the
#                            folder install.sh records, so delete.sh puts it
#                            back. Linked, an edit in either place is the
#                            repo's.
#
# DEVWS_ROOT names the devws checkout; else it is found through the
# ~/.tmuxinator link this runs by.

set -u

fail() {
  printf 'devws ghostty: %s\n' "$*" >&2
  exit 1
}

checkout() {
  local self="$0" target
  if [ -n "${DEVWS_ROOT:-}" ]; then
    printf '%s' "$DEVWS_ROOT"
    return
  fi
  while [ -L "$self" ]; do
    target="$(readlink "$self")"
    case "$target" in
      /*) self="$target" ;;
      *) self="$(dirname "$self")/$target" ;;
    esac
  done
  (cd "$(dirname "$self")/../.." && pwd)
}

root="$(checkout)"
source_dir="$root/config/ghostty"
[ -f "$source_dir/config.ghostty" ] || fail "no config/ghostty in the devws checkout ($root)"

# Where Ghostty reads its config: on macOS its Application Support folder,
# elsewhere the XDG one. Themes are found in the XDG folder on both.
config_path() {
  if [ "$(uname -s)" = Darwin ]; then
    printf '%s' "$HOME/Library/Application Support/com.mitchellh.ghostty/config.ghostty"
  else
    printf '%s' "${XDG_CONFIG_HOME:-$HOME/.config}/ghostty/config.ghostty"
  fi
}
themes_dir="${XDG_CONFIG_HOME:-$HOME/.config}/ghostty/themes"

# install.sh's backup folder, made and recorded when there is none yet.
backup_root() {
  local state="$HOME/.devws-state/backup-root" folder
  if [ -f "$state" ] && IFS= read -r folder < "$state" && [ -n "$folder" ]; then
    printf '%s' "$folder"
    return
  fi
  folder="$HOME/.devws-backups/$(date +%Y%m%d-%H%M%S)"
  mkdir -p "$(dirname "$state")" || fail "cannot create $(dirname "$state")"
  printf '%s\n' "$folder" > "$state"
  printf '%s' "$folder"
}

# <destination> becomes a link to <source>. A file already the same is just
# replaced; anything else is backed up first, never over an earlier backup.
link() {
  local source="$1" destination="$2" backup
  if [ -L "$destination" ] && [ "$(readlink "$destination")" = "$source" ]; then
    return 0
  fi
  mkdir -p "$(dirname "$destination")" || fail "cannot create $(dirname "$destination")"
  if [ -e "$destination" ] || [ -L "$destination" ]; then
    if [ -f "$destination" ] && [ ! -L "$destination" ] && cmp -s "$source" "$destination"; then
      rm "$destination"
    else
      backup="$(backup_root)/${destination#"$HOME"/}"
      [ ! -e "$backup" ] && [ ! -L "$backup" ] ||
        fail "$destination is kept: $backup already holds an earlier backup"
      mkdir -p "$(dirname "$backup")" && mv "$destination" "$backup" ||
        fail "could not back up $destination"
      printf 'Backed up %s to %s\n' "$destination" "$backup"
    fi
  fi
  ln -s "$source" "$destination" || fail "could not link $destination"
  printf 'Linked %s\n' "$destination"
}

apply() {
  local theme
  link "$source_dir/config.ghostty" "$(config_path)"
  for theme in "$source_dir"/themes/*; do
    link "$theme" "$themes_dir/$(basename "$theme")"
  done
  printf 'Ghostty has devws'"'"'s theme. In Ghostty, Cmd+Shift+, reloads it (new windows have it already).\n'
}

case "${1:-}" in
  apply) apply ;;
  *) fail "usage: ghostty_theme.sh apply" ;;
esac
