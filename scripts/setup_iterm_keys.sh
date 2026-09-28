#!/usr/bin/env bash
# Teach iTerm2 to send a distinct sequence for Shift+Enter.
#
# iTerm2 sends a bare CR (0x0d) for both Enter and Shift+Enter, so no program
# downstream — shell, tmux, or agent — can tell them apart. This maps
# Shift+Enter to ESC CR (the same bytes as Option+Enter), which agents and
# readline treat as "insert a newline" instead of "submit".

set -euo pipefail

domain="com.googlecode.iterm2"
live_prefs="$HOME/Library/Preferences/$domain.plist"
bundled_map="/Applications/iTerm.app/Contents/Resources/DefaultGlobalKeyMap.plist"
state_root="$HOME/.devws-state"
ownership_state="$state_root/iterm-keys"

# Return (0x0d) plus NSEventModifierFlagShift (0x20000).
key="0xd-0x20000"
# Action 11 is "Send Hex Code" and 10 is "Send Escape Sequence"; both were read
# back from iTerm2's own stored key maps. If an agent ignores ESC CR, the two
# documented alternatives are Action 11 with "0x0a" (Ctrl-J) and Action 10 with
# "[13;2u" (CSI-u, which also needs the extkeys lines in config/tmux/tmux.conf).
action=11
text="0x1b 0x0d"

info() { printf '\033[36m[devws]\033[0m %s\n' "$*"; }
ok() { printf '\033[32m[done]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[warn]\033[0m %s\n' "$*" >&2; }

if [[ "$(uname -s)" != "Darwin" ]] || [[ ! -d /Applications/iTerm.app ]]; then
  info "iTerm2 is not installed; skipping the Shift+Enter key mapping"
  exit 0
fi

# iTerm2 holds its preferences in memory and rewrites the whole file when it
# quits, which would silently discard anything written here.
if pgrep -xq iTerm2; then
  warn "Quit iTerm2 first, then run: ./install.sh --iterm-keys"
  exit 1
fi

# When "load preferences from a custom folder" is enabled, that file — not the
# one in ~/Library/Preferences — is what iTerm2 reads at launch, so the mapping
# has to be written to both to survive a restart.
custom_prefs=""
if [[ "$(defaults read "$domain" LoadPrefsFromCustomFolder 2>/dev/null || true)" == 1 ]]; then
  custom_folder="$(defaults read "$domain" PrefsCustomFolder 2>/dev/null || true)"
  if [[ -n "$custom_folder" ]] && [[ -f "$custom_folder/$domain.plist" ]]; then
    custom_prefs="$custom_folder/$domain.plist"
  elif [[ -n "$custom_folder" ]]; then
    warn "Custom iTerm2 preferences folder has no $domain.plist: $custom_folder"
  fi
fi

has_mapping() {
  local file="$1"
  local stored_action stored_text

  [[ -f "$file" ]] || return 1
  stored_action="$(
    /usr/libexec/PlistBuddy -c "Print :GlobalKeyMap:$key:Action" "$file" 2>/dev/null || true
  )"
  stored_text="$(
    /usr/libexec/PlistBuddy -c "Print :GlobalKeyMap:$key:Text" "$file" 2>/dev/null || true
  )"
  [[ "$stored_action" == "$action" ]] && [[ "$stored_text" == "$text" ]]
}

configured=1
has_mapping "$live_prefs" || configured=0
if [[ -n "$custom_prefs" ]]; then
  has_mapping "$custom_prefs" || configured=0
fi
if [[ "$configured" == 1 ]]; then
  ok "Shift+Enter is already mapped in iTerm2"
  exit 0
fi

backup_file() {
  local file="$1"
  local backup_root backup

  [[ -f "$file" ]] || return 0
  [[ -r "$state_root/backup-root" ]] || return 0
  backup_root="$(cat "$state_root/backup-root")"
  [[ -n "$backup_root" ]] || return 0
  backup="$backup_root/${file#"$HOME/"}"
  [[ -e "$backup" ]] && return 0
  mkdir -p "$(dirname "$backup")"
  # A copy, not a move: these files are edited in place rather than replaced.
  cp "$file" "$backup"
  info "Backed up $file to $backup"
}

work="$(mktemp -d "${TMPDIR:-/tmp}/devws-iterm-keys.XXXXXX")"
trap 'rm -rf "$work"' EXIT

# Seed from whatever global map is in effect. While the preference is unset
# iTerm2 falls back to its bundled defaults, so writing a single-entry map here
# would silently drop every built-in global binding.
merged="$work/GlobalKeyMap.plist"
if ! plutil -extract GlobalKeyMap xml1 -o "$merged" "$live_prefs" 2>/dev/null &&
   ! { [[ -n "$custom_prefs" ]] &&
       plutil -extract GlobalKeyMap xml1 -o "$merged" "$custom_prefs" 2>/dev/null; }; then
  cp "$bundled_map" "$merged"
  plutil -convert xml1 "$merged"
  info "Seeded the global key map from iTerm2's bundled defaults"
fi

/usr/libexec/PlistBuddy -c "Delete :$key" "$merged" >/dev/null 2>&1 || true
/usr/libexec/PlistBuddy \
  -c "Add :$key dict" \
  -c "Add :$key:Action integer $action" \
  -c "Add :$key:Text string '$text'" \
  "$merged" >/dev/null

mkdir -p "$state_root"
printf '%s\n' "$key" > "$ownership_state"

backup_file "$live_prefs"
# Write through `defaults` so the change goes via cfprefsd instead of being
# overwritten by its cache.
defaults write "$domain" GlobalKeyMap "$(plutil -convert xml1 -o - "$merged")"
ok "Mapped Shift+Enter in $domain"

if [[ -n "$custom_prefs" ]]; then
  backup_file "$custom_prefs"
  patched="$work/custom.plist"
  cp "$custom_prefs" "$patched"
  /usr/libexec/PlistBuddy -c "Delete :GlobalKeyMap" "$patched" >/dev/null 2>&1 || true
  plutil -replace GlobalKeyMap -xml "$(plutil -convert xml1 -o - "$merged")" "$patched"
  cat "$patched" > "$custom_prefs"
  ok "Mapped Shift+Enter in $custom_prefs"
fi

info "Quit and reopen iTerm2 to activate Shift+Enter."
