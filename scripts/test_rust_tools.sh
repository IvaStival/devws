#!/usr/bin/env bash
# devws's Rust programs (rust_tools.sh): Rust new enough for both crates,
# devagentics built from agentic/ and devws-ui from devws-ui/. Every program
# is a stub that logs its call; nothing is downloaded or built.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/devws-rust-tools.XXXXXX")" && pwd -P)"
INSTALLER="$ROOT/config/tmuxinator/rust_tools.sh"
# The highest rust-version of the two crates.
NEEDED="$(sed -n 's/^rust-version *= *"\(.*\)"/\1/p' "$ROOT/agentic/Cargo.toml" "$ROOT/devws-ui/Cargo.toml" |
  sort -t. -k1,1n -k2,2n | tail -n 1)"
BOTH="build devagentics"$'\n'"build devws-ui"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Rust tools test failed: %s\n' "$*" >&2
  exit 1
}

[ -n "$NEEDED" ] || fail "the crates have no rust-version"

# A cargo of <version> in <bin>: --version reports it, install makes the
# crate's program next to it, every call is logged.
make_cargo() {
  local bin="$1" version="$2"
  mkdir -p "$bin"
  cat > "$bin/cargo" <<EOF
#!/usr/bin/env bash
printf 'cargo %s\n' "\$*" >> "\$LOG"
case "\$1" in
  --version) printf 'cargo $version (stub)\n' ;;
  install)
    case "\$3" in */agentic) program=devagentics ;; *) program="\$(basename "\$3")" ;; esac
    printf '#!/usr/bin/env bash\nexit 0\n' > "$bin/\$program"; chmod +x "$bin/\$program" ;;
esac
EOF
  chmod +x "$bin/cargo"
}

# rustup update makes the cargo in <bin> the needed version.
make_rustup() {
  local bin="$1"
  cat > "$bin/rustup" <<EOF
#!/usr/bin/env bash
printf 'rustup %s\n' "\$*" >> "\$LOG"
[ "\$1 \$2" = "update stable" ] && sed -i '' 's/cargo [0-9.]* (stub)/cargo $NEEDED.0 (stub)/' "$bin/cargo"
exit 0
EOF
  chmod +x "$bin/rustup"
}

# A fresh home and PATH per case: <name>, with only the system's own tools.
# curl serves a rustup installer that puts cargo and rustup in ~/.cargo/bin.
setup_case() {
  CASE="$TEST_ROOT/$1"
  export HOME="$CASE/home" LOG="$CASE/log"
  mkdir -p "$HOME" "$CASE/bin"
  : > "$LOG"
  cat > "$CASE/bin/curl" <<EOF
#!/usr/bin/env bash
printf 'curl %s\n' "\$*" >> "\$LOG"
cat <<'INSTALLER'
printf 'rustup-init %s\n' "\$*" >> "\$LOG"
$(declare -f make_cargo make_rustup)
NEEDED=$NEEDED
make_cargo "\$HOME/.cargo/bin" "$NEEDED.0"
make_rustup "\$HOME/.cargo/bin"
INSTALLER
EOF
  chmod +x "$CASE/bin/curl"
  export PATH="$CASE/bin:/usr/bin:/bin"
}

installer() {
  DEVWS_ROOT="$ROOT" "$INSTALLER" "$@"
}

recorded() {
  grep -qx "$1" "$HOME/.devws-state/installed-dependencies" 2>/dev/null
}

test_no_rust_installs_rust_then_builds() {
  setup_case no-rust
  [ "$(installer missing)" = "rust"$'\n'"$BOTH" ] || fail "missing without Rust: $(installer missing)"
  installer install --yes >"$CASE/out" 2>&1 || fail "install failed: $(cat "$CASE/out")"
  grep -q '^curl .*https://sh.rustup.rs' "$LOG" || fail "the rustup installer was not fetched: $(cat "$LOG")"
  grep -q '^rustup-init .*-y' "$LOG" && grep -q -- '--no-modify-path' "$LOG" ||
    fail "rustup-init did not run unattended without touching the shell's profile: $(cat "$LOG")"
  grep -qx "cargo install --path $ROOT/agentic --locked" "$LOG" || fail "devagentics was not built: $(cat "$LOG")"
  grep -qx "cargo install --path $ROOT/devws-ui --locked" "$LOG" || fail "devws-ui was not built: $(cat "$LOG")"
  recorded rustup || fail "rustup was not recorded as devws's"
  recorded devagentics || fail "devagentics was not recorded as devws's"
  recorded devws-ui || fail "devws-ui was not recorded as devws's"
  [ -z "$(installer missing)" ] || fail "something is still missing: $(installer missing)"
}

test_an_old_rust_is_updated_with_rustup_then_builds() {
  setup_case old-rust
  make_cargo "$HOME/.cargo/bin" 1.83.0
  make_rustup "$HOME/.cargo/bin"
  [ "$(installer missing)" = "rust-update 1.83.0 $NEEDED"$'\n'"$BOTH" ] ||
    fail "missing with Rust 1.83: $(installer missing)"
  installer install --yes >"$CASE/out" 2>&1 || fail "install failed: $(cat "$CASE/out")"
  [ "$(grep -n 'rustup update stable' "$LOG" | cut -d: -f1)" -lt "$(grep -n 'cargo install' "$LOG" | head -n 1 | cut -d: -f1)" ] ||
    fail "Rust was not updated before the build: $(cat "$LOG")"
  ! grep -q '^curl' "$LOG" || fail "Rust was installed again over a rustup one"
  ! recorded rustup || fail "a rustup devws did not install was recorded as devws's"
  recorded devagentics || fail "devagentics was not recorded as devws's"
}

test_an_old_rust_without_rustup_is_left_alone() {
  setup_case old-brew-rust
  make_cargo "$CASE/bin" 1.80.0
  ! installer install --yes >"$CASE/out" 2>&1 || fail "install claimed success with an old Rust"
  grep -q 'brew upgrade rust' "$CASE/out" || fail "it did not say how to update: $(cat "$CASE/out")"
  ! grep -q 'cargo install' "$LOG" || fail "it built with a Rust too old for agentic/"
}

# A program already on PATH, as a user may have built it.
make_program() {
  printf '#!/usr/bin/env bash\nexit 0\n' > "$CASE/bin/$1"
  chmod +x "$CASE/bin/$1"
}

test_nothing_to_do_when_ready() {
  setup_case ready
  make_cargo "$CASE/bin" "$NEEDED.0"
  make_program devagentics
  make_program devws-ui
  [ -z "$(installer missing)" ] || fail "missing when ready: $(installer missing)"
  installer install </dev/null >"$CASE/out" 2>&1 || fail "install failed when ready: $(cat "$CASE/out")"
  ! grep -q 'cargo install' "$LOG" || fail "it rebuilt without --rebuild"
}

# install.sh rebuilds every time, without asking, but never claims a
# program it did not put there.
test_rebuild_builds_existing_programs_without_claiming_them() {
  setup_case rebuild
  make_cargo "$CASE/bin" "$NEEDED.0"
  make_program devagentics
  make_program devws-ui
  installer install --rebuild </dev/null >"$CASE/out" 2>&1 || fail "rebuild failed: $(cat "$CASE/out")"
  grep -qx "cargo install --path $ROOT/agentic --locked" "$LOG" || fail "it did not rebuild devagentics"
  grep -qx "cargo install --path $ROOT/devws-ui --locked" "$LOG" || fail "it did not rebuild devws-ui"
  ! recorded devagentics || fail "it claimed a devagentics it did not install"
  ! recorded devws-ui || fail "it claimed a devws-ui it did not install"
}

# Each program on its own: only the missing one is built, and recorded.
test_only_the_missing_program_is_built() {
  setup_case one-missing
  make_cargo "$CASE/bin" "$NEEDED.0"
  make_program devagentics
  [ "$(installer missing)" = "build devws-ui" ] || fail "missing with devagentics there: $(installer missing)"
  installer install --yes >"$CASE/out" 2>&1 || fail "install failed: $(cat "$CASE/out")"
  ! grep -q "install --path $ROOT/agentic " "$LOG" || fail "it rebuilt devagentics without --rebuild"
  grep -qx "cargo install --path $ROOT/devws-ui --locked" "$LOG" || fail "devws-ui was not built: $(cat "$LOG")"
  recorded devws-ui || fail "devws-ui was not recorded as devws's"
  ! recorded devagentics || fail "it claimed a devagentics it did not install"
}

test_declining_installs_nothing() {
  setup_case declined
  make_cargo "$HOME/.cargo/bin" 1.83.0
  make_rustup "$HOME/.cargo/bin"
  ! printf 'n\n' | installer install >"$CASE/out" 2>&1 || fail "declining reported success"
  grep -q 'Rust 1.83.0 is too old' "$CASE/out" || fail "the question did not say what it would do: $(cat "$CASE/out")"
  ! grep -q 'rustup update\|cargo install' "$LOG" || fail "it installed after a no: $(cat "$LOG")"
  # With no one to ask, nothing is changed either.
  ! installer install </dev/null >"$CASE/out" 2>&1 || fail "it went ahead with no answer"
  ! grep -q 'rustup update\|cargo install' "$LOG" || fail "it installed with no answer: $(cat "$LOG")"
}

test_no_rust_installs_rust_then_builds
test_an_old_rust_is_updated_with_rustup_then_builds
test_an_old_rust_without_rustup_is_left_alone
test_nothing_to_do_when_ready
test_rebuild_builds_existing_programs_without_claiming_them
test_only_the_missing_program_is_built
test_declining_installs_nothing

printf 'Rust tools tests passed.\n'
