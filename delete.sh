#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

info() { printf '\033[36m[devws]\033[0m %s\n' "$*"; }
ok() { printf '\033[32m[done]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[warn]\033[0m %s\n' "$*" >&2; }

remove_link() {
  local source="$1"
  local destination="$2"

  if [[ -L "$destination" ]] && [[ "$(readlink "$destination")" == "$source" ]]; then
    rm "$destination"
    ok "Removed $destination"
  elif [[ -e "$destination" ]] || [[ -L "$destination" ]]; then
    warn "Kept $destination because it is not managed by this devws checkout"
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
remove_link "$ROOT/config/tmux/tmux.conf" "$HOME/.tmux.conf"
for file in "$ROOT"/config/tmux/*; do
  [[ "$(basename "$file")" == "tmux.conf" ]] && continue
  remove_link "$file" "$HOME/.tmux/$(basename "$file")"
done

for file in "$ROOT"/config/tmuxinator/* "$ROOT"/config/tmuxinator/.env; do
  remove_link "$file" "$HOME/.tmuxinator/$(basename "$file")"
done

remove_link "$ROOT/config/lvim/config.lua" "$HOME/.config/lvim/config.lua"
remove_link "$ROOT/config/lvim/lazy-lock.json" "$HOME/.config/lvim/lazy-lock.json"
remove_link "$ROOT/config/lvim/queries/markdown/highlights.scm" \
  "$HOME/.config/lvim/queries/markdown/highlights.scm"

remove_zshrc_block

rmdir "$HOME/.config/lvim/queries/markdown" 2>/dev/null || true
rmdir "$HOME/.config/lvim/queries" 2>/dev/null || true
rmdir "$HOME/.config/lvim" 2>/dev/null || true
rmdir "$HOME/.tmuxinator" 2>/dev/null || true
rmdir "$HOME/.tmux" 2>/dev/null || true

ok "Uninstall complete"
printf 'Backups and shared dependencies were preserved.\n'
printf 'Reload your shell: source ~/.zshrc\n'
