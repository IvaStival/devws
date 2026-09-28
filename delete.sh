#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
state_root="$HOME/.devws-state"
backup_state="$state_root/backup-root"
dependency_state="$state_root/installed-dependencies"
backup_root=""
restore_pending=0

info() { printf '\033[36m[devws]\033[0m %s\n' "$*"; }
ok() { printf '\033[32m[done]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[warn]\033[0m %s\n' "$*" >&2; }

remove_dependency_record() {
  local dependency="$1"
  local temporary

  [[ -f "$dependency_state" ]] || return
  temporary="$(mktemp "${TMPDIR:-/tmp}/devws-dependencies.XXXXXX")"
  grep -Fvx "$dependency" "$dependency_state" > "$temporary" || true
  if [[ -s "$temporary" ]]; then
    mv "$temporary" "$dependency_state"
  else
    rm "$temporary" "$dependency_state"
  fi
}

remove_owned_homebrew_dependency() {
  local dependency="$1"

  [[ -f "$dependency_state" ]] ||
    return 0
  grep -Fqx "$dependency" "$dependency_state" ||
    return 0

  if ! command -v brew >/dev/null 2>&1; then
    warn "Kept devws-installed $dependency because Homebrew is unavailable"
    return
  fi
  if brew list --formula "$dependency" >/dev/null 2>&1; then
    info "Removing $dependency installed by devws"
    if ! brew uninstall "$dependency"; then
      warn "Could not remove $dependency; keeping its ownership state for retry"
      return
    fi
    ok "Removed $dependency"
  fi
  remove_dependency_record "$dependency"
}

remove_iterm_keys() {
  local domain="com.googlecode.iterm2"
  local live_prefs="$HOME/Library/Preferences/com.googlecode.iterm2.plist"
  local bundled_map="/Applications/iTerm.app/Contents/Resources/DefaultGlobalKeyMap.plist"
  local ownership_state="$state_root/iterm-keys"
  local custom_prefs="" custom_folder work merged key

  [[ -f "$ownership_state" ]] || return 0
  IFS= read -r key < "$ownership_state"
  [[ -n "$key" ]] || { rm -f "$ownership_state"; return 0; }

  if pgrep -xq iTerm2; then
    warn "Quit iTerm2 and rerun ./delete.sh to remove the Shift+Enter key map"
    return 0
  fi

  if [[ "$(defaults read "$domain" LoadPrefsFromCustomFolder 2>/dev/null || true)" == 1 ]]; then
    custom_folder="$(defaults read "$domain" PrefsCustomFolder 2>/dev/null || true)"
    [[ -n "$custom_folder" ]] && [[ -f "$custom_folder/$domain.plist" ]] &&
      custom_prefs="$custom_folder/$domain.plist"
  fi

  work="$(mktemp -d "${TMPDIR:-/tmp}/devws-iterm-keys.XXXXXX")"
  merged="$work/GlobalKeyMap.plist"
  if plutil -extract GlobalKeyMap xml1 -o "$merged" "$live_prefs" 2>/dev/null ||
      { [[ -n "$custom_prefs" ]] &&
        plutil -extract GlobalKeyMap xml1 -o "$merged" "$custom_prefs" 2>/dev/null; }; then
    /usr/libexec/PlistBuddy -c "Delete :$key" "$merged" >/dev/null 2>&1 || true
    # Leaving nothing but iTerm2's own defaults is the same as never having set
    # the preference, so unset it and let iTerm2 fall back to the bundled map.
    if [[ -f "$bundled_map" ]] &&
        diff -q <(plutil -convert xml1 -o - "$merged") \
                <(plutil -convert xml1 -o - "$bundled_map") >/dev/null 2>&1; then
      defaults delete "$domain" GlobalKeyMap 2>/dev/null || true
    else
      defaults write "$domain" GlobalKeyMap "$(plutil -convert xml1 -o - "$merged")"
    fi
    if [[ -n "$custom_prefs" ]]; then
      cp "$custom_prefs" "$work/custom.plist"
      /usr/libexec/PlistBuddy -c "Delete :GlobalKeyMap:$key" "$work/custom.plist" \
        >/dev/null 2>&1 || true
      cat "$work/custom.plist" > "$custom_prefs"
    fi
    ok "Removed the devws Shift+Enter key map from iTerm2"
  fi
  rm -rf "$work"
  rm -f "$ownership_state"
}

if [[ -f "$backup_state" ]]; then
  IFS= read -r backup_root < "$backup_state"
  case "$backup_root" in
    "$HOME"/.devws-backups/*) ;;
    *)
      warn "Ignored invalid backup state: $backup_root"
      backup_root=""
      ;;
  esac
fi

restore_link() {
  local source="$1"
  local destination="$2"
  local relative="${destination#"$HOME"/}"
  local backup=""

  if [[ -n "$backup_root" ]]; then
    backup="$backup_root/$relative"
  fi

  if [[ -L "$destination" ]] && [[ "$(readlink "$destination")" == "$source" ]]; then
    rm "$destination"
    ok "Removed $destination"
  elif [[ -e "$destination" ]] || [[ -L "$destination" ]]; then
    warn "Kept $destination because it is not managed by this devws checkout"
    if [[ -n "$backup" ]] && { [[ -e "$backup" ]] || [[ -L "$backup" ]]; }; then
      restore_pending=1
      warn "Previous configuration remains available at $backup"
    fi
    return
  fi

  if [[ -n "$backup" ]] && { [[ -e "$backup" ]] || [[ -L "$backup" ]]; }; then
    mkdir -p "$(dirname "$destination")"
    mv "$backup" "$destination"
    ok "Restored $destination"
  fi
}

remove_zshrc_block() {
  local zshrc="$HOME/.zshrc"
  local source_line="source \"$ROOT/shell/devws.zsh\""
  local temporary line next previous=""
  local have_previous=0

  [[ -f "$zshrc" ]] || return
  temporary="$(mktemp "${TMPDIR:-/tmp}/devws-zshrc.XXXXXX")"

  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == "# devws terminal workspace" ]]; then
      if IFS= read -r next && [[ "$next" == "$source_line" ]]; then
        if [[ "$have_previous" == 1 ]] && [[ -n "$previous" ]]; then
          printf '%s\n' "$previous" >> "$temporary"
        fi
        have_previous=0
        previous=""
        unset next
        continue
      fi
      if [[ "$have_previous" == 1 ]]; then
        printf '%s\n' "$previous" >> "$temporary"
      fi
      printf '%s\n' "$line" >> "$temporary"
      if [[ -n "${next+x}" ]]; then
        previous="$next"
        have_previous=1
      else
        have_previous=0
      fi
      unset next
    else
      if [[ "$have_previous" == 1 ]]; then
        printf '%s\n' "$previous" >> "$temporary"
      fi
      previous="$line"
      have_previous=1
    fi
  done < "$zshrc"
  if [[ "$have_previous" == 1 ]]; then
    printf '%s\n' "$previous" >> "$temporary"
  fi

  if cmp -s "$zshrc" "$temporary"; then
    rm "$temporary"
    return
  fi

  if [[ "$(uname -s)" == "Darwin" ]]; then
    chmod "$(stat -f '%Lp' "$zshrc")" "$temporary"
  else
    chmod "$(stat -c '%a' "$zshrc")" "$temporary"
  fi
  mv "$temporary" "$zshrc"
  ok "Removed devws from $zshrc"
}

info "Removing configuration links"
restore_link "$ROOT/config/tmux/tmux.conf" "$HOME/.tmux.conf"
for file in "$ROOT"/config/tmux/*; do
  [[ "$(basename "$file")" == "tmux.conf" ]] && continue
  restore_link "$file" "$HOME/.tmux/$(basename "$file")"
done

for file in "$ROOT"/config/tmuxinator/* "$ROOT"/config/tmuxinator/.env; do
  restore_link "$file" "$HOME/.tmuxinator/$(basename "$file")"
done

restore_link "$ROOT/config/lvim/config.lua" "$HOME/.config/lvim/config.lua"
restore_link "$ROOT/config/lvim/lazy-lock.json" "$HOME/.config/lvim/lazy-lock.json"
restore_link "$ROOT/config/lvim/queries/markdown/highlights.scm" \
  "$HOME/.config/lvim/queries/markdown/highlights.scm"

remove_zshrc_block
remove_owned_homebrew_dependency "fzf"
remove_owned_homebrew_dependency "glow"
remove_iterm_keys

rm -f "$state_root/sessions.tsv"
rmdir "$state_root/restore.lock" 2>/dev/null || true

rmdir "$HOME/.config/lvim/queries/markdown" 2>/dev/null || true
rmdir "$HOME/.config/lvim/queries" 2>/dev/null || true
rmdir "$HOME/.config/lvim" 2>/dev/null || true
rmdir "$HOME/.tmuxinator" 2>/dev/null || true
rmdir "$HOME/.tmux" 2>/dev/null || true

if [[ "$restore_pending" == 0 ]]; then
  rm -f "$backup_state"
fi
rmdir "$state_root" 2>/dev/null || true

ok "Uninstall complete"
printf 'Previous configurations were restored when available.\n'
printf 'Pre-existing applications and dependencies were preserved.\n'
printf 'Reload your shell: source ~/.zshrc\n'
