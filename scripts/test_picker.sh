#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-picker.XXXXXX")"
PICKER="$ROOT/config/tmuxinator/picker.sh"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Picker test failed: %s\n' "$*" >&2
  exit 1
}

# fzf records its arguments, one per line, and answers from fzf.answer: a
# line to print, or "esc" for Esc (exit 130).
mkdir -p "$TEST_ROOT/bin"
cat > "$TEST_ROOT/bin/fzf" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" > "$TEST_ROOT/fzf.args"
cat > /dev/null
answer="\$(cat "$TEST_ROOT/fzf.answer")"
[ "\$answer" = esc ] && exit 130
printf '%s\n' "\$answer"
EOF
chmod +x "$TEST_ROOT/bin/fzf"
export PATH="$TEST_ROOT/bin:$PATH" TMPDIR="$TEST_ROOT"

args_have() {
  grep -qxF -- "$1" "$TEST_ROOT/fzf.args"
}

test_the_shared_look_and_the_callers_options() {
  printf 'two\n' > "$TEST_ROOT/fzf.answer"
  [ "$(printf 'one\ntwo\n' | "$PICKER" Repo 'Pick a repo' --delimiter=$'\t')" = two ] ||
    fail "the choice was not passed through"
  args_have '--prompt=Repo › ' || fail "no shared prompt: $(cat "$TEST_ROOT/fzf.args")"
  args_have '--header=Pick a repo' || fail "no header"
  grep -q -- '^--footer=.*Esc back · Enter choose' "$TEST_ROOT/fzf.args" || fail "no shared footer"
  ! grep -q -- '^--listen=' "$TEST_ROOT/fzf.args" || fail "the picker still repaints itself (it flickers)"
  args_have $'--delimiter=\t' || fail "the caller's own option was dropped"
}

test_a_multi_select_tells_how_to_untick() {
  printf 'a\n' > "$TEST_ROOT/fzf.answer"
  printf 'a\n' | "$PICKER" Apps 'Apps to move' --multi >/dev/null
  grep -q -- '^--footer=.*Tab untick' "$TEST_ROOT/fzf.args" || fail "a multi-select footer has no Tab untick"
}

test_esc_is_passed_on_and_nothing_is_left_running() {
  local status=0
  printf 'esc\n' > "$TEST_ROOT/fzf.answer"
  printf 'a\n' | "$PICKER" X 'Header' >/dev/null || status=$?
  [ "$status" = 130 ] || fail "Esc gave $status, not 130"
  sleep 0.2
  ! pgrep -f "$TEST_ROOT/devws-picker" >/dev/null || fail "the repaint loop outlived the picker"
  [ -z "$(ls "$TEST_ROOT" | grep '\.sock$')" ] || fail "the socket was left behind"
}

test_the_shared_look_and_the_callers_options
test_a_multi_select_tells_how_to_untick
test_esc_is_passed_on_and_nothing_is_left_running

printf 'Picker tests passed.\n'
