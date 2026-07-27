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

test_homebrew_tmuxinator_is_upgraded() {
  local test_home="$TEST_ROOT/tmuxinator"
  local fake_bin="$test_home/bin"
  mkdir -p "$test_home/.tmux/plugins/tpm"
  make_fake_bin "$fake_bin"
  make_command "$fake_bin/fzf"
  make_command "$fake_bin/lvim"
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
  make_command "$fake_bin/lvim"
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
  make_command "$fake_bin/lvim"

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
  make_command "$fake_bin/lvim"

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
test_homebrew_tmuxinator_is_upgraded
test_non_homebrew_tmuxinator_is_preserved
test_owned_glow_is_removed
test_preexisting_glow_is_preserved
test_picker_fails_once_without_fzf

printf 'Install lifecycle tests passed.\n'
