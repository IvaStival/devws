#!/usr/bin/env bash
# The Ghostty theme devws keeps (config/ghostty): applying it links the
# config and the themes where Ghostty reads them, backing up whatever else
# was there where delete.sh restores it from.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/devws-ghostty.XXXXXX")" && pwd -P)"
THEME="$ROOT/config/tmuxinator/ghostty_theme.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Ghostty theme test failed: %s\n' "$*" >&2
  exit 1
}

export HOME="$TEST_ROOT/home"
mkdir -p "$HOME"
CONFIG="$HOME/Library/Application Support/com.mitchellh.ghostty/config.ghostty"
THEMES="$HOME/.config/ghostty/themes"

linked_to() {
  [ -L "$1" ] && [ "$(readlink "$1")" = "$2" ]
}

test_apply_links_the_config_and_the_themes() {
  DEVWS_ROOT="$ROOT" "$THEME" apply >"$TEST_ROOT/out" 2>&1 || fail "apply failed: $(cat "$TEST_ROOT/out")"
  linked_to "$CONFIG" "$ROOT/config/ghostty/config.ghostty" || fail "the config is not linked"
  linked_to "$THEMES/glofi-light" "$ROOT/config/ghostty/themes/glofi-light" || fail "the light theme is not linked"
  linked_to "$THEMES/glofi-dark" "$ROOT/config/ghostty/themes/glofi-dark" || fail "the dark theme is not linked"
  grep -q 'Cmd+Shift+,' "$TEST_ROOT/out" || fail "it did not say how Ghostty reloads: $(cat "$TEST_ROOT/out")"
  # Again: nothing to do, nothing backed up.
  DEVWS_ROOT="$ROOT" "$THEME" apply >"$TEST_ROOT/out" 2>&1 || fail "a second apply failed"
  [ ! -d "$HOME/.devws-backups" ] || fail "a second apply backed something up"
}

# A config of the user's own is moved to devws's backup folder (the one
# install.sh records, so delete.sh puts it back); one that is already the
# same as devws's is simply replaced.
test_a_different_config_is_backed_up() {
  rm -rf "$HOME"
  mkdir -p "$(dirname "$CONFIG")" "$THEMES" "$HOME/.devws-state"
  printf 'font-size = 20\n' > "$CONFIG"
  cp "$ROOT/config/ghostty/themes/glofi-dark" "$THEMES/glofi-dark"
  printf '%s\n' "$HOME/.devws-backups/earlier" > "$HOME/.devws-state/backup-root"
  DEVWS_ROOT="$ROOT" "$THEME" apply >"$TEST_ROOT/out" 2>&1 || fail "apply failed: $(cat "$TEST_ROOT/out")"
  linked_to "$CONFIG" "$ROOT/config/ghostty/config.ghostty" || fail "the user's config was not replaced"
  [ "$(cat "$HOME/.devws-backups/earlier/Library/Application Support/com.mitchellh.ghostty/config.ghostty")" = 'font-size = 20' ] ||
    fail "the user's config was not backed up where delete.sh looks"
  [ ! -e "$HOME/.devws-backups/earlier/.config/ghostty/themes/glofi-dark" ] ||
    fail "a theme identical to devws's was backed up"
  linked_to "$THEMES/glofi-dark" "$ROOT/config/ghostty/themes/glofi-dark" || fail "the identical theme was not linked"
}

test_a_backup_folder_is_made_and_recorded_when_there_is_none() {
  rm -rf "$HOME"
  mkdir -p "$(dirname "$CONFIG")"
  printf 'font-size = 20\n' > "$CONFIG"
  DEVWS_ROOT="$ROOT" "$THEME" apply >/dev/null 2>&1 || fail "apply failed"
  local root
  root="$(cat "$HOME/.devws-state/backup-root" 2>/dev/null)" || fail "no backup folder was recorded"
  [ -f "$root/Library/Application Support/com.mitchellh.ghostty/config.ghostty" ] || fail "the config is not in $root"
}

test_apply_links_the_config_and_the_themes
test_a_different_config_is_backed_up
test_a_backup_folder_is_made_and_recorded_when_there_is_none

printf 'Ghostty theme tests passed.\n'
