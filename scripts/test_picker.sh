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
# A home of its own: no devws-ui in it unless a test puts one there.
mkdir -p "$TEST_ROOT/home"
export PATH="$TEST_ROOT/bin:$PATH" TMPDIR="$TEST_ROOT" HOME="$TEST_ROOT/home"

# devws-ui where rust_tools.sh installs it: records its arguments, one per
# line, and answers like the fzf stub (ui.answer, "esc" for 130).
install_devws_ui() {
  mkdir -p "$HOME/.cargo/bin"
  cat > "$HOME/.cargo/bin/devws-ui" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" > "$TEST_ROOT/ui.args"
cat > "$TEST_ROOT/ui.input"
answer="\$(cat "$TEST_ROOT/ui.answer")"
[ "\$answer" = esc ] && exit 130
printf '%s\n' "\$answer"
EOF
  chmod +x "$HOME/.cargo/bin/devws-ui"
}

ui_args_are() {
  local expected
  expected="$(printf '%s\n' "$@")"
  [ "$(cat "$TEST_ROOT/ui.args")" = "$expected" ] ||
    fail "devws-ui was given: $(cat "$TEST_ROOT/ui.args")"
}

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

# With devws-ui installed, it asks instead of fzf (no flicker in a popup),
# the callers' fzf options translated; the answer and the exit pass through.
test_devws_ui_asks_with_the_callers_options_translated() {
  local status=0
  install_devws_ui
  printf 'b\t/b\n' > "$TEST_ROOT/ui.answer"
  rm -f "$TEST_ROOT/fzf.args"
  [ "$(printf 'a\t/a\nb\t/b\n' | "$PICKER" Apps 'Apps to move' --multi --bind 'load:select-all' \
    --delimiter=$'\t' --with-nth=1 --height=100%)" = $'b\t/b' ] || fail "the answer was not passed through"
  [ ! -e "$TEST_ROOT/fzf.args" ] || fail "fzf asked although devws-ui is installed"
  ui_args_are pick '--prompt=Apps › ' '--header=Apps to move' \
    "--footer=$(printf '\033[2m%s\033[0m' 'Tab untick · Esc back · Enter choose')" \
    --multi --select-all $'--delimiter=\t' --with-nth=1
  [ "$(cat "$TEST_ROOT/ui.input")" = $'a\t/a\nb\t/b' ] || fail "the choices did not reach devws-ui"

  printf 'esc\n' > "$TEST_ROOT/ui.answer"
  printf 'a\n' | "$PICKER" Folder 'Its folder' --print-query >/dev/null || status=$?
  [ "$status" = 130 ] || fail "Esc in devws-ui gave $status, not 130"
  ui_args_are pick '--prompt=Folder › ' '--header=Its folder' \
    "--footer=$(printf '\033[2m%s\033[0m' 'Esc back · Enter choose')" --print-query
  rm -rf "$HOME/.cargo"
}

# An fzf option devws-ui does not have leaves that question to fzf.
test_an_option_devws_ui_lacks_falls_back_to_fzf() {
  install_devws_ui
  printf 'x\n' > "$TEST_ROOT/fzf.answer"
  rm -f "$TEST_ROOT/ui.args"
  [ "$(printf 'x\n' | "$PICKER" X 'Header' --bind 'ctrl-r:reload(true)')" = x ] || fail "fzf did not answer"
  [ ! -e "$TEST_ROOT/ui.args" ] || fail "devws-ui was asked with an option it lacks"
  args_have '--bind' || fail "fzf did not get the caller's option"
  rm -rf "$HOME/.cargo"
}

test_the_shared_look_and_the_callers_options
test_a_multi_select_tells_how_to_untick
test_esc_is_passed_on_and_nothing_is_left_running
test_devws_ui_asks_with_the_callers_options_translated
test_an_option_devws_ui_lacks_falls_back_to_fzf

printf 'Picker tests passed.\n'
