#!/usr/bin/env bash
# devws's Rust programs, and installing what they need: Rust at least the
# highest rust-version of their crates, then each program built from its
# crate in the devws checkout:
#   devagentics  agentic/   agent teams (devws agentic)
#   devws-ui     devws-ui/  the menu and pickers, without popup flicker
# devws agentic offers this when devagentics is missing; install.sh rebuilds.
#
#   rust_tools.sh missing     the steps still needed, one per line: rust,
#                             rust-update <found> <needed>, build <program>;
#                             nothing when every program is there
#   rust_tools.sh install [--yes] [--rebuild]
#                             says what it will do, asks (--yes does not),
#                             then does it; --rebuild builds every program
#                             even when it is there, without asking for that
#                             alone. Exits 0 once nothing is missing.
#
# What devws installs is recorded in ~/.devws-state/installed-dependencies
# (as install.sh does), so delete.sh removes only that. A Rust devws did not
# install is never replaced: without rustup, an old one is left to the user.
# DEVWS_ROOT names the devws checkout; else it is found through the
# ~/.tmuxinator link this runs by.

set -u

fail() {
  printf 'devws: %s\n' "$*" >&2
  exit 1
}

# rustup puts cargo here, and does not touch the shell's profile for devws.
PATH="$HOME/.cargo/bin:$PATH"
dependency_state="$HOME/.devws-state/installed-dependencies"
# "<program> <crate folder>", in build order.
programs='devagentics agentic
devws-ui devws-ui'

checkout() {
  local self="$0" target
  if [ -n "${DEVWS_ROOT:-}" ]; then
    printf '%s' "$DEVWS_ROOT"
    return
  fi
  while [ -L "$self" ]; do
    target="$(readlink "$self")"
    case "$target" in
      /*) self="$target" ;;
      *) self="$(dirname "$self")/$target" ;;
    esac
  done
  (cd "$(dirname "$self")/../.." && pwd)
}

root="$(checkout)"
while read -r program crate; do
  [ -f "$root/$crate/Cargo.toml" ] || fail "no $crate/ in the devws checkout ($root)"
done <<<"$programs"

crate_of() {
  awk -v program="$1" '$1 == program { print $2 }' <<<"$programs"
}

# The highest rust-version among the crates.
needed_version() {
  local program crate
  while read -r program crate; do
    sed -n 's/^rust-version *= *"\(.*\)"/\1/p' "$root/$crate/Cargo.toml" | head -n 1
  done <<<"$programs" | sort -t. -k1,1n -k2,2n | tail -n 1
}

# cargo's version is its toolchain's; empty when it does not say.
found_version() {
  cargo --version 2>/dev/null | awk '{ print $2 }' | grep -E '^[0-9]+\.[0-9]+' || true
}

# <found> is older than <needed>, by major then minor.
older() {
  local found_major found_minor needed_major needed_minor
  IFS=. read -r found_major found_minor _ <<<"$1"
  IFS=. read -r needed_major needed_minor _ <<<"$2"
  [ "$found_major" -lt "$needed_major" ] ||
    { [ "$found_major" -eq "$needed_major" ] && [ "${found_minor:-0}" -lt "${needed_minor:-0}" ]; }
}

missing() {
  local found needed program crate
  needed="$(needed_version)"
  if ! command -v cargo >/dev/null 2>&1; then
    printf 'rust\n'
  else
    found="$(found_version)"
    # A version cargo does not report is not held against it: the build says.
    if [ -n "$found" ] && [ -n "$needed" ] && older "$found" "$needed"; then
      printf 'rust-update %s %s\n' "$found" "$needed"
    fi
  fi
  while read -r program crate; do
    command -v "$program" >/dev/null 2>&1 || printf 'build %s\n' "$program"
  done <<<"$programs"
}

record() {
  mkdir -p "$(dirname "$dependency_state")"
  grep -Fqx "$1" "$dependency_state" 2>/dev/null || printf '%s\n' "$1" >> "$dependency_state"
}

# What the steps will do, in words, for the question.
describe() {
  local step first second
  while read -r step first second; do
    case "$step" in
      rust) printf -- '- install Rust (rustup, from https://rustup.rs, into ~/.cargo)\n' ;;
      rust-update) printf -- '- Rust %s is too old for devws'"'"'s programs (they need %s): update it with rustup\n' "$first" "$second" ;;
      build) printf -- '- build %s from %s/ (a few minutes)\n' "$first" "$(crate_of "$first")" ;;
    esac
  done <<<"$1"
}

# Only what changes the machine asks: install.sh's own rebuild does not.
confirm() {
  local steps="$1" reply
  printf 'devws needs:\n%s\nContinue? [Y/n] ' "$(describe "$steps")"
  if ! IFS= read -r reply; then
    printf '\nNothing was installed.\n'
    return 1
  fi
  case "$reply" in
    '' | y | Y | yes | Yes | YES) return 0 ;;
    *) printf 'Nothing was installed.\n'; return 1 ;;
  esac
}

install_rust() {
  printf 'Installing Rust (rustup)…\n'
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path ||
    fail "Rust did not install (rustup's output is above)"
  hash -r
  command -v cargo >/dev/null 2>&1 || fail "rustup installed, but cargo is not in ~/.cargo/bin"
  record rustup
}

update_rust() {
  local found="$1" needed="$2"
  command -v rustup >/dev/null 2>&1 ||
    fail "Rust $found is too old for devws's programs (they need $needed), and devws did not install it: update it (brew upgrade rust), then try again"
  printf 'Updating Rust %s…\n' "$found"
  rustup update stable || fail "Rust did not update (rustup's output is above)"
  hash -r
}

build() {
  local program="$1" owned=0
  command -v "$program" >/dev/null 2>&1 || owned=1
  printf 'Building %s…\n' "$program"
  cargo install --path "$root/$(crate_of "$program")" --locked ||
    fail "$program did not build (cargo's output is above)"
  hash -r
  [ "$owned" = 1 ] && record "$program"
  return 0
}

install() {
  local assume_yes=0 rebuild=0 option steps step first second program crate
  for option in "$@"; do
    case "$option" in
      --yes) assume_yes=1 ;;
      --rebuild) rebuild=1 ;;
      *) fail "usage: rust_tools.sh install [--yes] [--rebuild]" ;;
    esac
  done
  steps="$(missing)"
  if [ "$rebuild" = 1 ]; then
    while read -r program crate; do
      grep -qx "build $program" <<<"$steps" || steps="${steps:+$steps$'\n'}build $program"
    done <<<"$programs"
  fi
  [ -n "$steps" ] || return 0
  # A rebuild alone is install.sh's own job, not a question.
  if [ "$assume_yes" = 0 ] && { [ "$rebuild" = 0 ] || grep -q '^rust' <<<"$steps"; }; then
    confirm "$steps" || return 1
  fi
  while read -r step first second; do
    case "$step" in
      rust) install_rust ;;
      rust-update) update_rust "$first" "$second" ;;
      build) build "$first" ;;
    esac
  done <<<"$steps"
  steps="$(missing)"
  [ -z "$steps" ] || fail "still missing after installing: $(tr '\n' ' ' <<<"$steps")"
}

command="${1:-}"
[ "$#" -gt 0 ] && shift
case "$command" in
  missing) missing ;;
  install) install "$@" ;;
  *) fail "usage: rust_tools.sh <missing|install [--yes] [--rebuild]>" ;;
esac
