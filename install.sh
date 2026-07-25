#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WITH_DEPS=0

usage() {
  cat <<'EOF'
Usage: ./install.sh [--deps]

  --deps  Install Homebrew packages, LunarVim, and tmux plugin manager.
EOF
}

for arg in "$@"; do
  case "$arg" in
    --deps) WITH_DEPS=1 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'Unknown option: %s\n' "$arg" >&2; usage >&2; exit 2 ;;
  esac
done

info() { printf '\033[36m[devws]\033[0m %s\n' "$*"; }
ok() { printf '\033[32m[done]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[warn]\033[0m %s\n' "$*" >&2; }

install_dependencies() {
  if ! command -v brew >/dev/null 2>&1; then
    printf 'Homebrew is required for automatic dependency installation.\n' >&2
    printf 'Install it from https://brew.sh, then rerun ./install.sh --deps\n' >&2
    exit 1
  fi

  info "Installing core packages from Brewfile"
  brew bundle --file="$ROOT/Brewfile"

  if ! command -v lvim >/dev/null 2>&1; then
    info "Installing LunarVim"
    LV_BRANCH=master bash <(
      curl -fsSL https://raw.githubusercontent.com/lunarvim/lunarvim/master/utils/installer/install.sh
    )
  fi

  if [[ ! -d "$HOME/.tmux/plugins/tpm" ]]; then
    info "Installing tmux plugin manager"
    mkdir -p "$HOME/.tmux/plugins"
    git clone --depth 1 https://github.com/tmux-plugins/tpm \
      "$HOME/.tmux/plugins/tpm"
  fi
}

timestamp="$(date +%Y%m%d-%H%M%S)"
backup_root="$HOME/.devws-backups/$timestamp"
backed_up=0

backup_path() {
  local destination="$1"
  local relative="${destination#"$HOME"/}"
  local backup="$backup_root/$relative"

  mkdir -p "$(dirname "$backup")"
  mv "$destination" "$backup"
  backed_up=1
  warn "Backed up $destination to $backup"
}

link_file() {
  local source="$1"
  local destination="$2"

  mkdir -p "$(dirname "$destination")"
  if [[ -L "$destination" ]] && [[ "$(readlink "$destination")" == "$source" ]]; then
    return
  fi
  if [[ -e "$destination" ]] || [[ -L "$destination" ]]; then
    backup_path "$destination"
  fi
  ln -s "$source" "$destination"
  ok "$destination"
}

if [[ "$WITH_DEPS" == 1 ]]; then
  install_dependencies
fi

info "Installing configuration links"
link_file "$ROOT/config/tmux/tmux.conf" "$HOME/.tmux.conf"
for file in "$ROOT"/config/tmux/*; do
  [[ "$(basename "$file")" == "tmux.conf" ]] && continue
  link_file "$file" "$HOME/.tmux/$(basename "$file")"
done

for file in "$ROOT"/config/tmuxinator/* "$ROOT"/config/tmuxinator/.env; do
  link_file "$file" "$HOME/.tmuxinator/$(basename "$file")"
done

link_file "$ROOT/config/lvim/config.lua" "$HOME/.config/lvim/config.lua"
link_file "$ROOT/config/lvim/lazy-lock.json" "$HOME/.config/lvim/lazy-lock.json"
link_file "$ROOT/config/lvim/queries/markdown/highlights.scm" \
  "$HOME/.config/lvim/queries/markdown/highlights.scm"

zshrc="$HOME/.zshrc"
source_line="source \"$ROOT/shell/devws.zsh\""
touch "$zshrc"
if ! grep -Fqx "$source_line" "$zshrc"; then
  {
    printf '\n# devws terminal workspace\n'
    printf '%s\n' "$source_line"
  } >> "$zshrc"
  ok "Added devws to $zshrc"
fi

if [[ ! -d "$HOME/.tmux/plugins/tpm" ]]; then
  warn "tmux plugin manager is missing; run ./install.sh --deps"
fi
if ! command -v lvim >/dev/null 2>&1; then
  warn "LunarVim is missing; run ./install.sh --deps"
fi
if ! command -v claude >/dev/null 2>&1 &&
    ! command -v codex >/dev/null 2>&1; then
  warn "Install Claude Code or Codex before starting a workspace"
fi

if [[ "$backed_up" == 1 ]]; then
  info "Backups are in $backup_root"
fi

ok "Installation complete"
printf 'Reload your shell: source ~/.zshrc\n'
printf 'Start a workspace: devws /path/to/project\n'
