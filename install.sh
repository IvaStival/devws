#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WITH_DEPS=0
WITH_ITERM_KEYS=0
WITH_AI_MEMORY=0
WITH_JUNGLEFORGE=0
WITH_VERBOSE=0
state_root="$HOME/.devws-state"
dependency_state="$state_root/installed-dependencies"
install_log="$state_root/install.log"

usage() {
  cat <<'EOF'
Usage: ./install.sh [--deps] [--iterm-keys] [--ai-memory] [--jungleforge] [--verbose]

  --deps        Install Homebrew packages, devdb and tmux plugin manager.
  --iterm-keys  Map Shift+Enter to a newline in iTerm2 (quit iTerm2 first).
  --ai-memory   Offer ai-memory (long-term memory for agents) again after a no.
  --jungleforge Offer jungleforge (runs branches in Docker) again after a no.
  --verbose     Show every tool's output instead of one line per step.
EOF
}

for arg in "$@"; do
  case "$arg" in
    --deps) WITH_DEPS=1 ;;
    --iterm-keys) WITH_ITERM_KEYS=1 ;;
    --ai-memory) WITH_AI_MEMORY=1 ;;
    --jungleforge) WITH_JUNGLEFORGE=1 ;;
    --verbose) WITH_VERBOSE=1 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'Unknown option: %s\n' "$arg" >&2; usage >&2; exit 2 ;;
  esac
done

# The installer shows one line per step. What the tools print goes to
# install.log, so fds 3 and 4 keep the terminal for the step lines, the
# questions and the warnings.
exec 3>&1 4>&2
current_step=""
step_label=""
step_messages=()
progress_shown=0

info() { printf '\033[36m[devws]\033[0m %s\n' "$*"; }
ok() { printf '\033[32m[done]\033[0m %s\n' "$*"; }

# Inside a step, a message is logged and shown under the step's line once it
# finishes; outside one, it is shown right away.
report() {
  local mark="$1" message="$2"
  if [[ -n "$current_step" ]] && [[ "$WITH_VERBOSE" == 0 ]]; then
    printf '%s\n' "$message" >>"$install_log"
    step_messages+=("$mark $message")
  else
    printf '%s %s\n' "$mark" "$message" >&4
  fi
}
warn() { report $'\033[33m!\033[0m' "$*"; }
note() { report $'\033[36m·\033[0m' "$*"; }

clear_progress() {
  [[ "$progress_shown" == 1 ]] || return 0
  printf '\r\033[K' >&3
  progress_shown=0
}

# Runs a command with the terminal, for what someone has to see or answer.
on_terminal() {
  clear_progress
  "$@" >&3 2>&4
}

# Asks a question on the terminal; the answer is in the caller's $reply.
ask() {
  clear_progress
  printf '%s' "$1" >&3
  IFS= read -r reply
}

# A step runs in this shell, so what it sets stays set, and set -e still stops
# the install when a command in it fails (the EXIT trap names the step). The
# step may change step_label to report what it found.
step() {
  local message
  current_step="$1"
  step_label="$1"
  step_messages=()
  shift
  if [[ "$WITH_VERBOSE" == 1 ]]; then
    printf '\033[36m==>\033[0m %s\n' "$current_step" >&3
    "$@"
  else
    if [[ -t 3 ]]; then
      printf '  \033[2m•\033[0m %s…' "$current_step" >&3
      progress_shown=1
    fi
    printf '\n== %s\n' "$current_step" >>"$install_log"
    { "$@"; } >>"$install_log" 2>&1
  fi
  clear_progress
  printf '  \033[32m✓\033[0m %s\n' "$step_label" >&3
  for message in ${step_messages[@]+"${step_messages[@]}"}; do
    printf '      %s\n' "$message" >&4
  done
  current_step=""
}

show_failed_step() {
  local status=$?
  [[ "$status" != 0 ]] && [[ -n "$current_step" ]] || return 0
  clear_progress
  printf '  \033[31m✗\033[0m %s\n' "$current_step" >&4
  [[ "$WITH_VERBOSE" == 0 ]] || return 0
  tail -n 20 "$install_log" | sed 's/^/      /' >&4
  printf 'Full log: %s\n' "$install_log" >&4
}
trap show_failed_step EXIT

# Stops the install with a message of its own, not the step's log.
stop() {
  current_step=""
  clear_progress
  printf '%s\n' "$@" >&4
  exit 1
}

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
    stop 'Install Homebrew from https://brew.sh, then rerun the installer.'
  fi

  if ! ask 'Install fzf with Homebrew now? [y/N] '; then
    stop '' 'Installation stopped before configuration was changed.'
  fi
  case "$reply" in
    y|Y|yes|YES|Yes) ;;
    *) stop 'Installation stopped before configuration was changed.' ;;
  esac

  info "Installing fzf"
  brew install fzf
  if ! command -v fzf >/dev/null 2>&1; then
    stop 'fzf was installed but is still unavailable on PATH.' \
      'Restart your shell and rerun ./install.sh'
  fi
  record_installed_dependency "fzf"
  ok "Installed fzf"
}

# GitHub workspaces (`devws` with no folder) list repos and PRs through gh,
# which needs a login once per machine. Declining only skips GitHub
# workspaces; local folders work without it.
ensure_github_login() {
  local reply

  if ! command -v gh >/dev/null 2>&1; then
    warn "gh is missing, so devws cannot open GitHub workspaces; run ./install.sh --deps"
    return
  fi
  gh auth status >/dev/null 2>&1 && return

  if ! ask 'Log in to GitHub now, for GitHub workspaces (gh auth login)? [Y/n] '; then
    printf '\n' >&3
    warn "Not logged in to GitHub; run gh auth login before using devws with no folder"
    return
  fi
  case "$reply" in
    n|N|no|NO|No)
      warn "Not logged in to GitHub; run gh auth login before using devws with no folder"
      return
      ;;
  esac
  # gh asks which protocol devws clones with: SSH uses your key, HTTPS has
  # gh log git in as well.
  on_terminal gh auth login ||
    warn "GitHub login failed; run gh auth login before using devws with no folder"
}

# ai-memory (github.com/akitaonrails/ai-memory) gives the agents long-term
# memory. Offered, never installed silently: yes runs its official installer
# (which checks its own download) and `ai-memory setup` (which shows its plan
# and asks), then devws wires every agent account (memory.sh). A "no" is kept,
# so it is not asked again; --ai-memory offers it again. Already installed,
# agents registered since are wired.
ensure_ai_memory() {
  local reply declined="$state_root/ai-memory-declined"

  PATH="$HOME/.local/bin:$PATH"
  if command -v ai-memory >/dev/null 2>&1; then
    "$ROOT/config/tmuxinator/memory.sh" wire-all 2>&4
    return
  fi
  [[ "$WITH_AI_MEMORY" == 1 ]] && rm -f "$declined"
  [[ -e "$declined" ]] && return

  if ! ask 'Install ai-memory (long-term memory for agents)? [y/N] '; then
    printf '\n' >&3
    return
  fi
  case "$reply" in
    y|Y|yes|YES|Yes) ;;
    *)
      mkdir -p "$state_root" && : > "$declined"
      note "Skipped ai-memory; ./install.sh --ai-memory offers it again"
      return
      ;;
  esac
  info "Installing ai-memory"
  if ! curl -fsSL https://github.com/akitaonrails/ai-memory/releases/latest/download/install.sh | sh; then
    warn "ai-memory did not install"
    return
  fi
  on_terminal ai-memory setup || warn "ai-memory setup did not finish; run ai-memory setup"
  "$ROOT/config/tmuxinator/memory.sh" wire-all 2>&4
}

# jungleforge (github.com/IvaStival/jungleForge) runs a branch's apps in
# Docker from the JungleForge tab. Offered like ai-memory: yes clones it and writes
# its .env (jungleforge.sh setup); a "no" is kept, and --jungleforge offers it
# again. Already there, only a stopped or missing Docker is reported.
ensure_jungleforge() {
  local reply declined="$state_root/jungleforge-declined"
  local jungleforge="$ROOT/config/tmuxinator/jungleforge.sh"

  if "$jungleforge" available; then
    command -v docker >/dev/null 2>&1 ||
      warn "Install Docker Desktop to run branches from the JungleForge tab"
    return
  fi
  [[ "$WITH_JUNGLEFORGE" == 1 ]] && rm -f "$declined"
  [[ -e "$declined" ]] && return

  if ! ask "Set up jungleforge (run branches in Docker) in $("$jungleforge" folder)? [y/N] "; then
    printf '\n' >&3
    return
  fi
  case "$reply" in
    y|Y|yes|YES|Yes) ;;
    *)
      mkdir -p "$state_root" && : > "$declined"
      note "Skipped jungleforge; ./install.sh --jungleforge offers it again"
      return
      ;;
  esac
  on_terminal "$jungleforge" setup ||
    warn "JungleForge setup failed. Choose JungleForge › Set up JungleForge in the devws menu to retry."
}

# devws's Rust programs (devagentics for agent teams, devws-ui for the menu
# and pickers) are rebuilt on every install (rust_tools.sh, which devws
# agentic also uses). Installing or updating Rust is asked, and only with
# someone at the terminal; devws works without them (fzf asks instead of
# devws-ui). What it installs is recorded for delete.sh, never a program or a
# Rust the user had.
build_rust_tools() {
  local input=/dev/null

  [[ -t 0 ]] && input=/dev/stdin
  DEVWS_ROOT="$ROOT" "$ROOT/config/tmuxinator/rust_tools.sh" install --rebuild <"$input" ||
    warn "devws agentic will not work until devagentics builds: devws agentic offers it again, or run ./install.sh in a terminal"
}

# A rebuild alone asks nothing, so its compiler output is only logged.
# Installing or updating Rust asks first, so it runs on the terminal.
ensure_rust_tools() {
  local missing
  missing="$(DEVWS_ROOT="$ROOT" "$ROOT/config/tmuxinator/rust_tools.sh" missing)"
  if grep -q '^rust' <<<"$missing"; then
    on_terminal build_rust_tools
  else
    build_rust_tools
  fi
}

upgrade_tmuxinator_if_needed() {
  if ! command -v brew >/dev/null 2>&1; then
    command -v tmuxinator >/dev/null 2>&1 &&
      warn "Cannot check tmuxinator updates because Homebrew is unavailable"
    return 0
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

require_homebrew() {
  command -v brew >/dev/null 2>&1 && return
  stop 'Homebrew is required for automatic dependency installation.' \
    'Install it from https://brew.sh, then rerun ./install.sh --deps'
}

install_homebrew_packages() {
  local glow_was_installed=0

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
}

# devdb (the database pane) lives in its own repository and is installed
# with uv from that checkout.
install_devdb() {
  local devdb_source="${DEVWS_DEVDB_SOURCE:-$HOME/Projects/devdb}"

  command -v devdb >/dev/null 2>&1 && return
  if [[ ! -f "$devdb_source/pyproject.toml" ]]; then
    warn "devdb was not found at $devdb_source; set DEVWS_DEVDB_SOURCE to its checkout"
    return
  fi
  command -v uv >/dev/null 2>&1 || brew install uv
  if ! command -v uv >/dev/null 2>&1; then
    warn "uv is unavailable, so devdb was not installed"
    return
  fi
  info "Installing devdb"
  # Compiled now rather than on the first launch, which would take seconds longer.
  uv tool install --compile-bytecode "$devdb_source" ||
    warn "devdb installation failed; the database pane will not open"
}

install_tmux_plugin_manager() {
  [[ -d "$HOME/.tmux/plugins/tpm" ]] && return
  info "Installing tmux plugin manager"
  mkdir -p "$HOME/.tmux/plugins"
  git clone --depth 1 https://github.com/tmux-plugins/tpm \
    "$HOME/.tmux/plugins/tpm"
}

timestamp="$(date +%Y%m%d-%H%M%S)"
backup_root="$HOME/.devws-backups/$timestamp"
backup_state="$state_root/backup-root"
backed_up=0

mkdir -p "$state_root"
[[ "$WITH_VERBOSE" == 1 ]] || : > "$install_log"
printf '\033[1mdevws install\033[0m\n' >&3

step "fzf" ensure_fzf
step "tmuxinator" upgrade_tmuxinator_if_needed

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
    return 0
  fi
  if [[ -e "$destination" ]] || [[ -L "$destination" ]]; then
    backup_path "$destination" || return 0
  fi
  ln -s "$source" "$destination"
  ok "$destination"
}

# devws used to configure LunarVim. Its links point at files that no longer
# exist, so they are removed and any configuration they replaced is restored.
retire_link() {
  local source="$1"
  local destination="$2"
  local backup="$backup_root/${destination#"$HOME"/}"

  [[ -L "$destination" ]] && [[ "$(readlink "$destination")" == "$source" ]] ||
    return 0
  rm "$destination"
  if [[ -e "$backup" ]] || [[ -L "$backup" ]]; then
    mv "$backup" "$destination"
    ok "Restored $destination from $backup"
  else
    ok "Removed $destination"
  fi
}

link_configuration() {
  local file

  link_file "$ROOT/config/tmux/tmux.conf" "$HOME/.tmux.conf"
  for file in "$ROOT"/config/tmux/*; do
    [[ "$(basename "$file")" == "tmux.conf" ]] && continue
    link_file "$file" "$HOME/.tmux/$(basename "$file")"
  done

  for file in "$ROOT"/config/tmuxinator/* "$ROOT"/config/tmuxinator/.env; do
    link_file "$file" "$HOME/.tmuxinator/$(basename "$file")"
  done
  # Links to scripts an update removed go too (devws does this on every start).
  "$ROOT/config/tmuxinator/link_scripts.sh" >/dev/null

  # The devws command, on PATH: an executable, so open shells run the current
  # version.
  link_file "$ROOT/bin/devws" "$HOME/.local/bin/devws"
  case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) warn "~/.local/bin is not on PATH; add it to run devws (export PATH=\"\$HOME/.local/bin:\$PATH\")" ;;
  esac

  # LazyVim writes lazy-lock.json and lazyvim.json into its config folder, so
  # the whole folder is linked and those files are tracked in the repo.
  link_file "$ROOT/config/nvim" "$HOME/.config/nvim"

  retire_link "$ROOT/config/lvim/config.lua" "$HOME/.config/lvim/config.lua"
  retire_link "$ROOT/config/lvim/lazy-lock.json" "$HOME/.config/lvim/lazy-lock.json"
  retire_link "$ROOT/config/lvim/queries/markdown/highlights.scm" \
    "$HOME/.config/lvim/queries/markdown/highlights.scm"
  rmdir "$HOME/.config/lvim/queries/markdown" "$HOME/.config/lvim/queries" 2>/dev/null || true
}

setup_iterm_keys() {
  local reply

  if [[ "$WITH_ITERM_KEYS" == 1 ]]; then
    on_terminal "$ROOT/scripts/setup_iterm_keys.sh" || true
    return 0
  fi
  # Only offer it when it is not already configured and someone is there to answer.
  if /usr/libexec/PlistBuddy -c 'Print :GlobalKeyMap:0xd-0x20000' \
      "$HOME/Library/Preferences/com.googlecode.iterm2.plist" >/dev/null 2>&1; then
    return 0
  fi
  [[ -t 0 ]] || return 0

  if ! ask 'Map Shift+Enter to a newline in iTerm2 now? [y/N] '; then
    printf '\n' >&3
    note "Leaving the iTerm2 key map unchanged."
    return 0
  fi
  case "$reply" in
    y|Y|yes|YES|Yes) on_terminal "$ROOT/scripts/setup_iterm_keys.sh" || true ;;
    *) note "Skipped the iTerm2 key map; run ./install.sh --iterm-keys later" ;;
  esac
}

add_shell_integration() {
  local zshrc="$HOME/.zshrc"
  local source_line="source \"$ROOT/shell/devws.zsh\""

  touch "$zshrc"
  grep -Fqx "$source_line" "$zshrc" && return
  {
    printf '\n# devws terminal workspace\n'
    printf '%s\n' "$source_line"
  } >> "$zshrc"
  ok "Added devws to $zshrc"
}

# The system monitor's panes (system.yml): the Brewfile has both tools, so
# only --deps installs them.
check_system_monitor_tools() {
  local tool missing=()
  for tool in btop lazydocker; do
    command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
  done
  case "${#missing[@]}" in
    0) ;;
    1) warn "${missing[0]} (system monitor) is missing; run ./install.sh --deps" ;;
    *) warn "${missing[0]} and ${missing[1]} (system monitor) are missing; run ./install.sh --deps" ;;
  esac
}

check_missing_tools() {
  [[ -d "$HOME/.tmux/plugins/tpm" ]] ||
    warn "tmux plugin manager is missing; run ./install.sh --deps"
  command -v nvim >/dev/null 2>&1 ||
    warn "Neovim is missing; run ./install.sh --deps"
  command -v devdb >/dev/null 2>&1 ||
    warn "devdb (database pane) is missing; run ./install.sh --deps"
  check_system_monitor_tools
  if ! command -v claude >/dev/null 2>&1 &&
      ! command -v codex >/dev/null 2>&1; then
    warn "Install Claude Code or Codex before starting a workspace"
  fi
  return 0
}

# Open workspaces take the update now; agents and editors keep running.
reload_open_workspaces() {
  local output

  if ! output="$("$HOME/.tmuxinator/reload_devws.sh")"; then
    printf '%s\n' "$output"
    warn "Could not reload the open workspaces; run devws reload"
    return 0
  fi
  printf '%s\n' "$output"
  step_label="Open workspaces reloaded ($(tail -n 1 <<<"$output" | sed 's/^devws: //'))"
}

if [[ "$WITH_DEPS" == 1 ]]; then
  require_homebrew
  step "Homebrew packages" install_homebrew_packages
  step "devdb" install_devdb
  step "tmux plugin manager" install_tmux_plugin_manager
fi

step "Configuration links" link_configuration
step "devagentics and devws-ui" ensure_rust_tools
if [[ "$(uname -s)" == "Darwin" ]] && [[ -d /Applications/iTerm.app ]]; then
  step "iTerm2 Shift+Enter" setup_iterm_keys
fi
step "Shell integration (~/.zshrc)" add_shell_integration
step "GitHub login" ensure_github_login
step "ai-memory" ensure_ai_memory
step "jungleforge" ensure_jungleforge
step "Checks" check_missing_tools

if command -v tmux >/dev/null 2>&1 && tmux list-sessions >/dev/null 2>&1; then
  step "Reloading the open workspaces" reload_open_workspaces
fi

printf '\n' >&3
if [[ "$backed_up" == 1 ]]; then
  printf 'Backups are in %s\n' "$backup_root" >&3
fi
if [[ "$WITH_VERBOSE" == 1 ]]; then
  printf '\033[32m✓\033[0m Installation complete\n' >&3
else
  printf '\033[32m✓\033[0m Installation complete \033[2m(log: %s)\033[0m\n' "$install_log" >&3
fi
printf '\n' >&3
printf '\033[1;33mReload your shell before using devws:\033[0m\n' >&3
printf '\033[1;36m  source ~/.zshrc\033[0m\n' >&3
printf '\n' >&3
printf 'Start a workspace: devws (a GitHub repo) or devws /path/to/project\n' >&3
