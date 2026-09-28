#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WITH_DEPS=0
WITH_ITERM_KEYS=0
state_root="$HOME/.devws-state"
dependency_state="$state_root/installed-dependencies"

usage() {
  cat <<'EOF'
Usage: ./install.sh [--deps] [--iterm-keys]

  --deps        Install Homebrew packages, LunarVim, and tmux plugin manager.
  --iterm-keys  Map Shift+Enter to a newline in iTerm2 (quit iTerm2 first).
EOF
}

for arg in "$@"; do
  case "$arg" in
    --deps) WITH_DEPS=1 ;;
    --iterm-keys) WITH_ITERM_KEYS=1 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'Unknown option: %s\n' "$arg" >&2; usage >&2; exit 2 ;;
  esac
done

info() { printf '\033[36m[devws]\033[0m %s\n' "$*"; }
ok() { printf '\033[32m[done]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[warn]\033[0m %s\n' "$*" >&2; }

record_installed_dependency() {
  local dependency="$1"

  mkdir -p "$state_root"
  if [[ ! -f "$dependency_state" ]] ||
      ! grep -Fqx "$dependency" "$dependency_state"; then
    printf '%s\n' "$dependency" >> "$dependency_state"
  fi
}

ensure_fzf() {
  local reply

  command -v fzf >/dev/null 2>&1 && return

  warn "fzf is required but is not installed or is unavailable on PATH"
  if ! command -v brew >/dev/null 2>&1; then
    printf 'Install Homebrew from https://brew.sh, then rerun the installer.\n' >&2
    exit 1
  fi

  printf 'Install fzf with Homebrew now? [y/N] '
  if ! IFS= read -r reply; then
    printf '\nInstallation stopped before configuration was changed.\n' >&2
    exit 1
  fi
  case "$reply" in
    y|Y|yes|YES|Yes) ;;
    *)
      printf 'Installation stopped before configuration was changed.\n' >&2
      exit 1
      ;;
  esac

  info "Installing fzf"
  brew install fzf
  if ! command -v fzf >/dev/null 2>&1; then
    printf 'fzf was installed but is still unavailable on PATH.\n' >&2
    printf 'Restart your shell and rerun ./install.sh\n' >&2
    exit 1
  fi
  record_installed_dependency "fzf"
  ok "Installed fzf"
}

upgrade_tmuxinator_if_needed() {
  if ! command -v brew >/dev/null 2>&1; then
    command -v tmuxinator >/dev/null 2>&1 &&
      warn "Cannot check tmuxinator updates because Homebrew is unavailable"
    return
  fi
  if brew list --formula tmuxinator >/dev/null 2>&1; then
    if brew outdated --quiet tmuxinator | grep -q .; then
      info "Upgrading Homebrew tmuxinator"
      brew upgrade tmuxinator
    fi
  elif command -v tmuxinator >/dev/null 2>&1; then
    warn "Existing tmuxinator is not managed by Homebrew; leaving it unchanged"
  fi
}

install_dependencies() {
  local glow_was_installed=0

  if ! command -v brew >/dev/null 2>&1; then
    printf 'Homebrew is required for automatic dependency installation.\n' >&2
    printf 'Install it from https://brew.sh, then rerun ./install.sh --deps\n' >&2
    exit 1
  fi

  command -v glow >/dev/null 2>&1 && glow_was_installed=1

  info "Installing core packages from Brewfile"
  brew bundle --file="$ROOT/Brewfile"
  if ! command -v glow >/dev/null 2>&1; then
    printf 'Glow installation failed or Glow is unavailable on PATH.\n' >&2
    exit 1
  fi
  if [[ "$glow_was_installed" == 0 ]]; then
    record_installed_dependency "glow"
  fi

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
backup_state="$state_root/backup-root"
backed_up=0

ensure_fzf
upgrade_tmuxinator_if_needed

mkdir -p "$state_root"
if [[ -f "$backup_state" ]]; then
  IFS= read -r backup_root < "$backup_state"
else
  if [[ -L "$HOME/.tmux.conf" ]] &&
      [[ "$(readlink "$HOME/.tmux.conf")" == "$ROOT/config/tmux/tmux.conf" ]]; then
    for legacy_backup in "$HOME"/.devws-backups/*; do
      [[ -d "$legacy_backup" ]] || continue
      backup_root="$legacy_backup"
    done
    if [[ "$backup_root" != "$HOME/.devws-backups/$timestamp" ]]; then
      warn "Recovered backup state from existing installation: $backup_root"
    fi
  fi
  printf '%s\n' "$backup_root" > "$backup_state"
fi

backup_path() {
  local destination="$1"
  local relative="${destination#"$HOME"/}"
  local backup="$backup_root/$relative"

  if [[ -e "$backup" ]] || [[ -L "$backup" ]]; then
    warn "Kept existing pre-devws backup at $backup"
    warn "Skipped $destination to avoid overwriting either configuration"
    return 1
  fi
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
    backup_path "$destination" || return
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

setup_iterm_keys() {
  local reply

  [[ "$(uname -s)" == "Darwin" ]] || return 0
  [[ -d /Applications/iTerm.app ]] || return 0

  if [[ "$WITH_ITERM_KEYS" == 1 ]]; then
    "$ROOT/scripts/setup_iterm_keys.sh" || true
    return 0
  fi
  # Only offer it when it is not already configured and someone is there to answer.
  if /usr/libexec/PlistBuddy -c 'Print :GlobalKeyMap:0xd-0x20000' \
      "$HOME/Library/Preferences/com.googlecode.iterm2.plist" >/dev/null 2>&1; then
    return 0
  fi
  [[ -t 0 ]] || return 0

  printf 'Map Shift+Enter to a newline in iTerm2 now? [y/N] '
  if ! IFS= read -r reply; then
    printf '\nLeaving the iTerm2 key map unchanged.\n' >&2
    return 0
  fi
  case "$reply" in
    y|Y|yes|YES|Yes) "$ROOT/scripts/setup_iterm_keys.sh" || true ;;
    *) info "Skipped the iTerm2 key map; run ./install.sh --iterm-keys later" ;;
  esac
}

setup_iterm_keys

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
printf '\n\n'
printf '\033[1;33mReload your shell before using devws:\033[0m\n'
printf '\033[1;36m  source ~/.zshrc\033[0m\n'
printf '\n'
printf 'Start a workspace: devws /path/to/project\n'
