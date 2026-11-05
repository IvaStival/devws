#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-lifecycle.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  printf 'Lifecycle test failed: %s\n' "$*" >&2
  exit 1
}

make_fake_bin() {
  local fake_bin="$1"

  mkdir -p "$fake_bin"
  cat > "$fake_bin/brew" <<'EOF'
#!/usr/bin/env bash
set -u
printf '%s\n' "$*" >> "$BREW_LOG"
case "${1:-} ${2:-}" in
  "install fzf")
    printf '#!/usr/bin/env bash\nexit 0\n' > "$FAKE_BIN/fzf"
    chmod +x "$FAKE_BIN/fzf"
    ;;
  "uninstall glow")
    rm -f "$FAKE_BIN/glow"
    ;;
  "uninstall fzf")
    rm -f "$FAKE_BIN/fzf"
    ;;
  "list --formula")
    [[ "${3:-}" == "tmuxinator" && "${BREW_TMUXINATOR:-0}" == 1 ]] ||
      [[ "${3:-}" == "fzf" && -x "$FAKE_BIN/fzf" ]] ||
      [[ "${3:-}" == "glow" && -x "$FAKE_BIN/glow" ]]
    ;;
  "outdated --quiet")
    [[ "${3:-}" == "tmuxinator" &&
       "${BREW_TMUXINATOR_OUTDATED:-0}" == 1 ]] &&
      printf 'tmuxinator\n'
    ;;
  bundle*)
    if [[ ! -x "$FAKE_BIN/glow" ]]; then
      printf '#!/usr/bin/env bash\nexit 0\n' > "$FAKE_BIN/glow"
      chmod +x "$FAKE_BIN/glow"
    fi
    ;;
esac
EOF
  chmod +x "$fake_bin/brew"
}

make_command() {
  local path="$1"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$path"
  chmod +x "$path"
}

run_install() {
  local test_home="$1"
  local fake_bin="$2"
  shift 2
  HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/install.sh" "$@"
}

test_declining_fzf_leaves_home_untouched() {
  local test_home="$TEST_ROOT/decline"
  local fake_bin="$test_home/bin"
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"

  if printf 'n\n' | run_install "$test_home" "$fake_bin" >/dev/null 2>&1; then
    fail "declining fzf installation should fail"
  fi
  [[ ! -e "$test_home/.tmux.conf" ]] ||
    fail "configuration changed after declining fzf"
  ! grep -q '^install fzf$' "$test_home/brew.log" 2>/dev/null ||
    fail "fzf was installed after consent was declined"
}

test_owned_fzf_is_removed() {
  local test_home="$TEST_ROOT/owned"
  local fake_bin="$test_home/bin"
  local install_output="$test_home/install-output"
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"

  printf 'y\n' | run_install "$test_home" "$fake_bin" > "$install_output" 2>&1
  grep -qx 'fzf' "$test_home/.devws-state/installed-dependencies" ||
    fail "devws-installed fzf was not recorded"
  grep -Fq $'\033[1;36m  source ~/.zshrc\033[0m' "$install_output" ||
    fail "colored shell reload instruction was not printed"

  HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/delete.sh" >/dev/null
  grep -q '^uninstall fzf$' "$test_home/brew.log" ||
    fail "owned fzf was not uninstalled"
}

test_preexisting_fzf_is_preserved() {
  local test_home="$TEST_ROOT/preexisting"
  local fake_bin="$test_home/bin"
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"

  run_install "$test_home" "$fake_bin" >/dev/null 2>&1
  [[ ! -f "$test_home/.devws-state/installed-dependencies" ]] ||
    fail "pre-existing fzf was recorded as owned"

  HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/delete.sh" >/dev/null
  ! grep -q '^uninstall fzf$' "$test_home/brew.log" 2>/dev/null ||
    fail "pre-existing fzf was uninstalled"
}

# gh is logged in when GH_LOGGED_IN is 1; every call is logged.
make_fake_gh() {
  cat > "$1/gh" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$HOME/gh.log"
case "$1 $2" in
  "auth status") [[ "${GH_LOGGED_IN:-0}" == 1 ]] ;;
esac
EOF
  chmod +x "$1/gh"
}

test_install_offers_the_github_login() {
  local test_home="$TEST_ROOT/github"
  local fake_bin="$test_home/bin"
  local output
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_fake_gh "$fake_bin"

  GH_LOGGED_IN=1 run_install "$test_home" "$fake_bin" </dev/null >/dev/null 2>&1
  ! grep -q '^auth login' "$test_home/gh.log" || fail "a logged-in gh was asked to log in"

  output="$(printf 'n\n' | run_install "$test_home" "$fake_bin" 2>&1)"
  ! grep -q '^auth login' "$test_home/gh.log" || fail "gh logged in after the answer was no"
  grep -q 'run gh auth login' <<<"$output" || fail "declining the login gave no hint"

  printf 'y\n' | run_install "$test_home" "$fake_bin" >/dev/null 2>&1
  grep -q '^auth login' "$test_home/gh.log" || fail "the GitHub login was not offered"

  rm "$fake_bin/gh"
  output="$(run_install "$test_home" "$fake_bin" </dev/null 2>&1)"
  grep -q 'gh is missing' <<<"$output" || fail "a missing gh was not reported"
}

# ai-memory is offered, never installed silently. A stub curl serves an
# "installer" that puts a logging ai-memory in ~/.local/bin.
test_install_offers_ai_memory() {
  local test_home="$TEST_ROOT/ai-memory"
  local fake_bin="$test_home/bin"
  local output
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  cat > "$fake_bin/curl" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$HOME/curl.log"
cat <<'INSTALLER'
mkdir -p "$HOME/.local/bin"
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$*" >> "$HOME/ai-memory.log"\n' > "$HOME/.local/bin/ai-memory"
chmod +x "$HOME/.local/bin/ai-memory"
INSTALLER
EOF
  chmod +x "$fake_bin/curl"

  output="$(printf 'n\n' | run_install "$test_home" "$fake_bin" 2>&1)"
  grep -q 'Install ai-memory' <<<"$output" || fail "ai-memory was not offered"
  [ ! -e "$test_home/curl.log" ] || fail "ai-memory was downloaded after a no"
  output="$(run_install "$test_home" "$fake_bin" </dev/null 2>&1)"
  ! grep -q 'Install ai-memory' <<<"$output" || fail "ai-memory was offered again after a no"

  printf 'y\n' | run_install "$test_home" "$fake_bin" --ai-memory >/dev/null 2>&1
  grep -q 'akitaonrails/ai-memory/releases/latest/download/install.sh' "$test_home/curl.log" ||
    fail "the official installer was not fetched"
  grep -qx 'setup' "$test_home/ai-memory.log" || fail "ai-memory setup did not run"
  grep -q 'install-mcp --client claude-code --apply' "$test_home/ai-memory.log" ||
    fail "the agents were not wired: $(cat "$test_home/ai-memory.log")"
}

# jungleforge is offered like ai-memory: a no is kept, --jungleforge asks
# again, and yes clones it (from a local remote) with its .env.
test_install_offers_jungleforge() {
  local test_home="$TEST_ROOT/jungleforge"
  local fake_bin="$test_home/bin" remote="$TEST_ROOT/jungleForge-remote"
  local output folder
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  git init -q -b main "$remote"
  : > "$remote/Makefile"
  # jungleforge's placeholder, as an argument: no hard-coded-password shape.
  printf 'POSTGRES_PASSWORD=%s\n' change_me > "$remote/.env.example"
  git -C "$remote" add -A
  git -C "$remote" -c user.name=devws -c user.email=devws@example.com commit -q -m engine
  folder="$test_home/.devws/jungleforge"
  export DEVWS_JUNGLEFORGE="$folder" DEVWS_JUNGLEFORGE_REPO="$remote"
  # ai-memory, asked just before, already declined: every answer is this one's.
  mkdir -p "$test_home/.devws-state" && : > "$test_home/.devws-state/ai-memory-declined"

  output="$(printf 'n\n' | run_install "$test_home" "$fake_bin" 2>&1)"
  grep -q 'Set up jungleforge' <<<"$output" || fail "jungleforge was not offered"
  [ ! -e "$folder" ] || fail "jungleforge was cloned after a no"
  output="$(run_install "$test_home" "$fake_bin" </dev/null 2>&1)"
  ! grep -q 'Set up jungleforge' <<<"$output" || fail "jungleforge was offered again after a no"

  printf 'y\n' | run_install "$test_home" "$fake_bin" --jungleforge >/dev/null 2>&1
  [ -f "$folder/Makefile" ] || fail "jungleforge was not cloned after a yes"
  [ -f "$folder/.env" ] && ! grep -q change_me "$folder/.env" || fail "jungleforge's .env was not written"
  output="$(run_install "$test_home" "$fake_bin" </dev/null 2>&1)"
  ! grep -q 'Set up jungleforge' <<<"$output" || fail "an installed jungleforge was offered again"
  unset DEVWS_JUNGLEFORGE DEVWS_JUNGLEFORGE_REPO
}

test_homebrew_tmuxinator_is_upgraded() {
  local test_home="$TEST_ROOT/tmuxinator"
  local fake_bin="$test_home/bin"
  mkdir -p "$test_home/.tmux/plugins/tpm"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_command "$fake_bin/nvim"
  make_command "$fake_bin/tmuxinator"

  HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    BREW_TMUXINATOR=1 BREW_TMUXINATOR_OUTDATED=1 \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/install.sh" >/dev/null 2>&1
  grep -q '^upgrade tmuxinator$' "$test_home/brew.log" ||
    fail "Homebrew tmuxinator was not upgraded"
}

test_non_homebrew_tmuxinator_is_preserved() {
  local test_home="$TEST_ROOT/external-tmuxinator"
  local fake_bin="$test_home/bin"
  local install_output="$test_home/install-output"
  mkdir -p "$test_home/.tmux/plugins/tpm"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_command "$fake_bin/nvim"
  make_command "$fake_bin/tmuxinator"

  HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    BREW_TMUXINATOR=0 PATH="$fake_bin:/usr/bin:/bin" \
    "$ROOT/install.sh" --deps > "$install_output" 2>&1
  ! grep -q '^upgrade tmuxinator$' "$test_home/brew.log" ||
    fail "non-Homebrew tmuxinator was upgraded"
  grep -q 'tmuxinator is not managed by Homebrew' "$install_output" ||
    fail "non-Homebrew tmuxinator preservation was not reported"
}

test_owned_glow_is_removed() {
  local test_home="$TEST_ROOT/owned-glow"
  local fake_bin="$test_home/bin"
  mkdir -p "$test_home/.tmux/plugins/tpm"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_command "$fake_bin/nvim"

  run_install "$test_home" "$fake_bin" --deps >/dev/null 2>&1
  grep -qx 'glow' "$test_home/.devws-state/installed-dependencies" ||
    fail "devws-installed Glow was not recorded"

  HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/delete.sh" >/dev/null
  grep -q '^uninstall glow$' "$test_home/brew.log" ||
    fail "owned Glow was not uninstalled"
}

test_preexisting_glow_is_preserved() {
  local test_home="$TEST_ROOT/preexisting-glow"
  local fake_bin="$test_home/bin"
  mkdir -p "$test_home/.tmux/plugins/tpm"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_command "$fake_bin/glow"
  make_command "$fake_bin/nvim"

  run_install "$test_home" "$fake_bin" --deps >/dev/null 2>&1
  if [[ -f "$test_home/.devws-state/installed-dependencies" ]]; then
    ! grep -qx 'glow' "$test_home/.devws-state/installed-dependencies" ||
      fail "pre-existing Glow was recorded as owned"
  fi

  HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/delete.sh" >/dev/null
  ! grep -q '^uninstall glow$' "$test_home/brew.log" ||
    fail "pre-existing Glow was uninstalled"
}

# The devws command goes on PATH as a link to bin/devws, and goes again on
# uninstall.
test_devws_command_is_linked_and_removed() {
  local test_home="$TEST_ROOT/devws-command"
  local fake_bin="$test_home/bin"
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"

  run_install "$test_home" "$fake_bin" >/dev/null 2>&1
  [[ "$(readlink "$test_home/.local/bin/devws")" == "$ROOT/bin/devws" ]] ||
    fail "~/.local/bin/devws does not link to bin/devws"

  HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/delete.sh" >/dev/null
  [[ ! -e "$test_home/.local/bin/devws" ]] || fail "uninstall left ~/.local/bin/devws"
}

test_nvim_config_is_linked_and_restored() {
  local test_home="$TEST_ROOT/nvim-config"
  local fake_bin="$test_home/bin"
  mkdir -p "$test_home/.config/nvim"
  printf 'previous\n' > "$test_home/.config/nvim/init.lua"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_command "$fake_bin/nvim"

  run_install "$test_home" "$fake_bin" >/dev/null 2>&1
  [[ "$(readlink "$test_home/.config/nvim")" == "$ROOT/config/nvim" ]] ||
    fail "~/.config/nvim does not link to the repo's LazyVim config"

  HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/delete.sh" >/dev/null
  [[ ! -L "$test_home/.config/nvim" ]] &&
    [[ "$(cat "$test_home/.config/nvim/init.lua")" == "previous" ]] ||
    fail "the previous ~/.config/nvim was not restored on delete"
}

test_lunarvim_links_are_retired() {
  local test_home="$TEST_ROOT/retire-lvim"
  local fake_bin="$test_home/bin"
  local backup_root="$test_home/.devws-backups/old"
  mkdir -p "$test_home/.config/lvim/queries/markdown" "$backup_root/.config/lvim" \
    "$test_home/.devws-state"
  printf '%s\n' "$backup_root" > "$test_home/.devws-state/backup-root"
  printf 'mine\n' > "$backup_root/.config/lvim/config.lua"
  ln -s "$ROOT/config/lvim/config.lua" "$test_home/.config/lvim/config.lua"
  ln -s "$ROOT/config/lvim/lazy-lock.json" "$test_home/.config/lvim/lazy-lock.json"
  ln -s "$ROOT/config/lvim/queries/markdown/highlights.scm" \
    "$test_home/.config/lvim/queries/markdown/highlights.scm"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_command "$fake_bin/nvim"

  run_install "$test_home" "$fake_bin" >/dev/null 2>&1
  [[ ! -L "$test_home/.config/lvim/config.lua" ]] &&
    [[ "$(cat "$test_home/.config/lvim/config.lua")" == "mine" ]] ||
    fail "the pre-devws LunarVim config was not restored"
  [[ ! -e "$test_home/.config/lvim/lazy-lock.json" ]] &&
    [[ ! -e "$test_home/.config/lvim/queries" ]] ||
    fail "stale devws LunarVim links were left behind"
}

test_deps_install_devdb_from_its_checkout() {
  local test_home="$TEST_ROOT/devdb"
  local fake_bin="$test_home/bin"
  local devdb_source="$test_home/src/devdb"
  local output="$test_home/install-output"
  mkdir -p "$test_home/.tmux/plugins/tpm"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_command "$fake_bin/nvim"
  printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$*" >> "%s/uv.log"\n' "$test_home" > "$fake_bin/uv"
  chmod +x "$fake_bin/uv"

  # Without a checkout there is nothing to install, and the user is told so.
  DEVWS_DEVDB_SOURCE="$devdb_source" run_install "$test_home" "$fake_bin" --deps > "$output" 2>&1
  grep -q "devdb was not found at $devdb_source" "$output" ||
    fail "a missing devdb checkout was not reported"
  [[ ! -s "$test_home/uv.log" ]] || fail "uv ran without a devdb checkout"

  mkdir -p "$devdb_source"
  : > "$devdb_source/pyproject.toml"
  DEVWS_DEVDB_SOURCE="$devdb_source" run_install "$test_home" "$fake_bin" --deps >/dev/null 2>&1
  grep -qx "tool install --compile-bytecode $devdb_source" "$test_home/uv.log" ||
    fail "--deps did not install devdb from its checkout: $(cat "$test_home/uv.log" 2>/dev/null)"

  rm -f "$test_home/uv.log"
  make_command "$fake_bin/devdb"
  DEVWS_DEVDB_SOURCE="$devdb_source" run_install "$test_home" "$fake_bin" --deps >/dev/null 2>&1
  [[ ! -s "$test_home/uv.log" ]] || fail "--deps reinstalled an existing devdb"
}

# The agent team's program is built from agentic/ when cargo exists, and
# uninstall removes it only when install put it there.
test_agentic_is_built_and_removed_when_owned() {
  local test_home="$TEST_ROOT/agentic-owned"
  local fake_bin="$test_home/bin"
  local output="$test_home/install-output"
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  cat > "$fake_bin/cargo" <<'EOF2'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$CARGO_LOG"
case "$1" in
  install) printf '#!/usr/bin/env bash\nexit 0\n' > "$FAKE_BIN/devagentics"; chmod +x "$FAKE_BIN/devagentics" ;;
  uninstall) rm -f "$FAKE_BIN/devagentics" ;;
esac
EOF2
  chmod +x "$fake_bin/cargo"

  CARGO_LOG="$test_home/cargo.log" run_install "$test_home" "$fake_bin" >"$output" 2>&1
  grep -qx "install --path $ROOT/agentic --locked" "$test_home/cargo.log" ||
    fail "install did not build agentic/: $(cat "$test_home/cargo.log" 2>/dev/null)"
  grep -qx devagentics "$test_home/.devws-state/installed-dependencies" ||
    fail "install did not record that it installed devagentics"

  CARGO_LOG="$test_home/cargo.log" HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/delete.sh" >/dev/null 2>&1
  grep -qx "uninstall devagentics" "$test_home/cargo.log" ||
    fail "uninstall did not remove the devagentics it installed"
  [[ ! -e "$fake_bin/devagentics" ]] || fail "devagentics is still there after uninstall"
}

test_a_preexisting_agentic_is_rebuilt_but_kept_on_uninstall() {
  local test_home="$TEST_ROOT/agentic-preexisting"
  local fake_bin="$test_home/bin"
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_command "$fake_bin/devagentics"
  printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$*" >> "$CARGO_LOG"\n' > "$fake_bin/cargo"
  chmod +x "$fake_bin/cargo"

  CARGO_LOG="$test_home/cargo.log" run_install "$test_home" "$fake_bin" >/dev/null 2>&1
  grep -qx "install --path $ROOT/agentic --locked" "$test_home/cargo.log" ||
    fail "install did not rebuild an existing devagentics"
  ! grep -qx devagentics "$test_home/.devws-state/installed-dependencies" 2>/dev/null ||
    fail "install claimed a devagentics it did not put there"

  CARGO_LOG="$test_home/cargo.log" HOME="$test_home" FAKE_BIN="$fake_bin" BREW_LOG="$test_home/brew.log" \
    PATH="$fake_bin:/usr/bin:/bin" "$ROOT/delete.sh" >/dev/null 2>&1
  ! grep -q '^uninstall' "$test_home/cargo.log" || fail "uninstall removed a devagentics it does not own"
}

test_install_without_cargo_says_what_agentic_needs() {
  local test_home="$TEST_ROOT/agentic-no-cargo"
  local fake_bin="$test_home/bin"
  local output="$test_home/install-output"
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"

  run_install "$test_home" "$fake_bin" >"$output" 2>&1 || fail "install failed without cargo"
  grep -q "devws agentic needs Rust" "$output" ||
    fail "install did not say agentic needs Rust: $(cat "$output")"
  [[ ! -e "$test_home/.devws-state/installed-dependencies" ]] ||
    fail "install recorded something without cargo"
}

# Install reloads the open workspaces when a tmux server answers, and leaves
# tmux alone otherwise. The fake tmux logs its calls; TMUX_UP says whether a
# server answers.
test_install_reloads_open_workspaces() {
  local test_home="$TEST_ROOT/reload"
  local fake_bin="$test_home/bin"
  local output
  mkdir -p "$test_home"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  cat > "$fake_bin/tmux" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$TMUX_LOG"
[ "${1:-}" = list-sessions ] && [ "${TMUX_UP:-0}" != 1 ] && exit 1
exit 0
EOF
  chmod +x "$fake_bin/tmux"

  output="$(TMUX_LOG="$test_home/tmux.log" TMUX_UP=0 run_install "$test_home" "$fake_bin" 2>&1)" ||
    fail "install failed without a tmux server: $output"
  ! grep -q '^source-file' "$test_home/tmux.log" || fail "install reloaded tmux without a server"
  ! grep -q 'Reloading the open workspaces' <<< "$output" || fail "install announced a reload without a server"

  output="$(TMUX_LOG="$test_home/tmux.log" TMUX_UP=1 run_install "$test_home" "$fake_bin" 2>&1)" ||
    fail "install failed with a tmux server: $output"
  grep -q "^source-file $test_home/.tmux.conf$" "$test_home/tmux.log" ||
    fail "install did not reload the open workspaces"
  grep -q 'devws: reloaded tmux.conf' <<< "$output" || fail "install did not report the reload: $output"
}

test_picker_fails_once_without_fzf() {
  local test_home="$TEST_ROOT/picker"
  local output
  mkdir -p "$test_home"

  if output="$(
    HOME="$test_home" PATH="/usr/bin:/bin" \
      /bin/bash "$ROOT/config/tmuxinator/session_picker.sh" 2>&1
  )"; then
    fail "picker should fail when fzf is missing"
  fi
  [[ "$(printf '%s\n' "$output" | grep -c 'fzf')" == 1 ]] ||
    fail "picker did not emit exactly one missing-fzf error"
}

test_declining_fzf_leaves_home_untouched
test_owned_fzf_is_removed
test_preexisting_fzf_is_preserved
test_install_offers_the_github_login
test_install_offers_ai_memory
test_install_offers_jungleforge
test_homebrew_tmuxinator_is_upgraded
test_non_homebrew_tmuxinator_is_preserved
test_owned_glow_is_removed
test_preexisting_glow_is_preserved
test_nvim_config_is_linked_and_restored
test_devws_command_is_linked_and_removed
test_lunarvim_links_are_retired
test_deps_install_devdb_from_its_checkout
test_agentic_is_built_and_removed_when_owned
test_a_preexisting_agentic_is_rebuilt_but_kept_on_uninstall
test_install_without_cargo_says_what_agentic_needs
test_install_reloads_open_workspaces
test_picker_fails_once_without_fzf

printf 'Install lifecycle tests passed.\n'
