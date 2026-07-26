#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
state_root="$HOME/.devws-state"
backup_state="$state_root/backup-root"
backup_root=""
restore_pending=0

info() { printf '\033[36m[devws]\033[0m %s\n' "$*"; }
ok() { printf '\033[32m[done]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[warn]\033[0m %s\n' "$*" >&2; }

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

rmdir "$HOME/.config/lvim/queries/markdown" 2>/dev/null || true
rmdir "$HOME/.config/lvim/queries" 2>/dev/null || true
rmdir "$HOME/.config/lvim" 2>/dev/null || true
rmdir "$HOME/.tmuxinator" 2>/dev/null || true
rmdir "$HOME/.tmux" 2>/dev/null || true

if [[ "$restore_pending" == 0 ]]; then
  rm -f "$backup_state"
  rmdir "$state_root" 2>/dev/null || true
fi

ok "Uninstall complete"
printf 'Previous configurations were restored when available.\n'
printf 'Shared applications and dependencies were preserved.\n'
printf 'Reload your shell: source ~/.zshrc\n'
