#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLED_MAP="/Applications/iTerm.app/Contents/Resources/DefaultGlobalKeyMap.plist"
KEY="0xd-0x20000"

if [[ "$(uname -s)" != "Darwin" ]] || [[ ! -r "$BUNDLED_MAP" ]]; then
  printf 'iTerm2 key map tests skipped: iTerm2 is not installed.\n'
  exit 0
fi

TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-iterm.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  printf 'iTerm2 key map test failed: %s\n' "$*" >&2
  exit 1
}

# pgrep always reports iTerm2 as not running; defaults is replaced by a shim
# that records the value it is handed and mirrors it into the fake preferences
# file, the way cfprefsd would.
make_fake_bin() {
  local fake_bin="$1"

  mkdir -p "$fake_bin"
  cat > "$fake_bin/pgrep" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
  cat > "$fake_bin/defaults" <<'EOF'
#!/usr/bin/env bash
set -u
case "${1:-}" in
  read)
    case "${3:-}" in
      LoadPrefsFromCustomFolder)
        [[ -n "${FAKE_CUSTOM_FOLDER:-}" ]] || exit 1
        printf '1\n'
        ;;
      PrefsCustomFolder)
        [[ -n "${FAKE_CUSTOM_FOLDER:-}" ]] || exit 1
        printf '%s\n' "$FAKE_CUSTOM_FOLDER"
        ;;
      *) exit 1 ;;
    esac
    ;;
  write)
    printf '%s' "$4" > "$DEFAULTS_WRITE_LOG"
    prefs="$HOME/Library/Preferences/com.googlecode.iterm2.plist"
    mkdir -p "$(dirname "$prefs")"
    [[ -f "$prefs" ]] || printf '{}\n' | plutil -convert xml1 -o "$prefs" -
    plutil -replace "$3" -xml "$4" "$prefs"
    ;;
  delete) : ;;
esac
exit 0
EOF
  chmod +x "$fake_bin/pgrep" "$fake_bin/defaults"
}

run_setup() {
  HOME="$TEST_ROOT/home" \
    DEFAULTS_WRITE_LOG="$TEST_ROOT/defaults-write.xml" \
    FAKE_CUSTOM_FOLDER="${FAKE_CUSTOM_FOLDER:-}" \
    PATH="$FAKE_BIN:/usr/bin:/bin:/usr/libexec" \
    "$ROOT/scripts/setup_iterm_keys.sh" > "$TEST_ROOT/out" 2>&1
}

setup() {
  rm -rf "$TEST_ROOT/home"
  mkdir -p "$TEST_ROOT/home/.devws-state" "$TEST_ROOT/home/Library/Preferences"
  rm -f "$TEST_ROOT/defaults-write.xml"
  FAKE_CUSTOM_FOLDER=""
}

written_entries() {
  plutil -convert xml1 -o - "$TEST_ROOT/defaults-write.xml" | grep -c '<key>0x'
}

test_seeds_from_bundled_defaults() {
  setup
  run_setup || fail "the script exited non-zero: $(cat "$TEST_ROOT/out")"

  local bundled written
  bundled="$(plutil -convert xml1 -o - "$BUNDLED_MAP" | grep -c '<key>0x')"
  written="$(written_entries)"
  (( written == bundled + 1 )) ||
    fail "expected $((bundled + 1)) global bindings, wrote $written — built-ins were dropped"
  [[ "$(/usr/libexec/PlistBuddy -c "Print :$KEY:Text" "$TEST_ROOT/defaults-write.xml")" == "0x1b 0x0d" ]] ||
    fail "Shift+Enter was not mapped to ESC CR"
  [[ "$(/usr/libexec/PlistBuddy -c "Print :$KEY:Action" "$TEST_ROOT/defaults-write.xml")" == "11" ]] ||
    fail "the mapping should use the Send Hex Code action"
  [[ "$(cat "$TEST_ROOT/home/.devws-state/iterm-keys")" == "$KEY" ]] ||
    fail "ownership of the key was not recorded for delete.sh"
}

test_is_idempotent() {
  setup
  run_setup
  rm -f "$TEST_ROOT/defaults-write.xml"
  run_setup
  [[ ! -f "$TEST_ROOT/defaults-write.xml" ]] ||
    fail "a second run rewrote the key map instead of doing nothing"
  grep -q 'already mapped' "$TEST_ROOT/out" ||
    fail "a second run should report the mapping as already present"
}

test_patches_custom_preferences_folder() {
  setup
  mkdir -p "$TEST_ROOT/custom"
  printf '{}\n' | plutil -convert xml1 -o "$TEST_ROOT/custom/com.googlecode.iterm2.plist" -
  FAKE_CUSTOM_FOLDER="$TEST_ROOT/custom"
  run_setup || fail "the script exited non-zero: $(cat "$TEST_ROOT/out")"

  [[ "$(/usr/libexec/PlistBuddy -c "Print :GlobalKeyMap:$KEY:Text" \
    "$TEST_ROOT/custom/com.googlecode.iterm2.plist")" == "0x1b 0x0d" ]] ||
    fail "the custom preferences folder was not patched, so the mapping would not survive a restart"
}

test_refuses_while_iterm_runs() {
  setup
  printf '#!/usr/bin/env bash\nexit 0\n' > "$FAKE_BIN/pgrep"
  chmod +x "$FAKE_BIN/pgrep"
  if run_setup; then
    fail "the script should refuse to write while iTerm2 is running"
  fi
  grep -q 'Quit iTerm2' "$TEST_ROOT/out" || fail "the refusal should say to quit iTerm2"
  printf '#!/usr/bin/env bash\nexit 1\n' > "$FAKE_BIN/pgrep"
  chmod +x "$FAKE_BIN/pgrep"
}

FAKE_BIN="$TEST_ROOT/bin"
make_fake_bin "$FAKE_BIN"

test_seeds_from_bundled_defaults
test_is_idempotent
test_patches_custom_preferences_folder
test_refuses_while_iterm_runs

printf 'iTerm2 key map tests passed.\n'
