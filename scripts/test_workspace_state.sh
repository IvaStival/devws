#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-state.XXXXXX")"
TAB="$(printf '\t')"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  printf 'Workspace state test failed: %s\n' "$*" >&2
  exit 1
}

# The fake tmux answers the read-only queries save_state.sh and restore_state.sh
# make from fixture files, and logs every command it is asked to run. Pane
# fixtures hold one "id|index|role|title|path" row per pane; the shim renders
# whichever of those fields the caller's -F format asks for.
make_fake_bin() {
  local fake_bin="$1"

  mkdir -p "$fake_bin"
  cat > "$fake_bin/tmux" <<'EOF'
#!/usr/bin/env bash
set -u
printf '%s\n' "$*" >> "$TMUX_LOG"

format=""
target=""
previous=""
for argument in "$@"; do
  case "$previous" in
    -F) format="$argument" ;;
    -t) target="$argument" ;;
  esac
  previous="$argument"
done
session="${target%%:*}"

case "${1:-}" in
  list-sessions)
    [[ -s "${FIXTURE_SESSIONS:-/nonexistent}" ]] || exit 1
    [[ -n "$format" ]] && cat "$FIXTURE_SESSIONS"
    exit 0
    ;;
  display-message)
    [[ "$*" == *'#{window_layout}'* ]] && printf '%s\n' "${FIXTURE_LAYOUT:-}"
    exit 0
    ;;
  list-panes)
    file="${FIXTURE_PANES_DIR:-/nonexistent}/${session:-unknown}"
    [[ -r "$file" ]] || exit 0
    # Render exactly the fields the caller's format asks for, in order.
    program=""
    while IFS= read -r token || [[ -n "$token" ]]; do
      case "$token" in
        '#{pane_id}') column='$1' ;;
        '#{pane_index}') column='$2' ;;
        '#{@devws_role}') column='$3' ;;
        '#{pane_title}') column='$4' ;;
        '#{pane_current_path}') column='$5' ;;
        *) column="\"$token\"" ;;
      esac
      if [[ -z "$program" ]]; then program="$column"; else program="$program, $column"; fi
    done < <(printf '%s' "$format" | tr '\t' '\n')
    awk -F '|' -v OFS='\t' "{ print $program }" "$file"
    exit 0
    ;;
  split-window)
    printf '%%split-%s\n' "$(cat "$TMUX_LOG" | grep -c '^split-window')"
    exit 0
    ;;
  has-session)
    [[ " ${FIXTURE_OPEN_SESSIONS:-} " == *" ${target#=} "* ]] && exit 0
    exit 1
    ;;
esac
exit 0
EOF
  cat > "$fake_bin/tmuxinator" <<'EOF'
#!/usr/bin/env bash
set -u
printf '%s\n' "$*" >> "$TMUXINATOR_LOG"
EOF
  chmod +x "$fake_bin/tmux" "$fake_bin/tmuxinator"
}

write_panes() {
  local session="$1"
  shift
  mkdir -p "$TEST_ROOT/panes"
  printf '%s\n' "$@" > "$TEST_ROOT/panes/$session"
}

setup() {
  rm -rf "$TEST_ROOT/home" "$TEST_ROOT/panes"
  mkdir -p "$TEST_ROOT/home/.devws-state" "$TEST_ROOT/home/.tmuxinator"
  cp "$ROOT/config/tmuxinator/save_state.sh" \
    "$ROOT/config/tmuxinator/restore_state.sh" "$TEST_ROOT/home/.tmuxinator/"
  cp "$ROOT/config/tmuxinator/.env" "$TEST_ROOT/home/.tmuxinator/.env"
  : > "$TEST_ROOT/tmux.log"
  : > "$TEST_ROOT/tmuxinator.log"
  FIXTURE_SESSIONS=""
  FIXTURE_LAYOUT=""
  FIXTURE_OPEN_SESSIONS=""
}

run_devws_script() {
  local script="$1"
  shift
  HOME="$TEST_ROOT/home" \
    TMUX_LOG="$TEST_ROOT/tmux.log" TMUXINATOR_LOG="$TEST_ROOT/tmuxinator.log" \
    FIXTURE_SESSIONS="$FIXTURE_SESSIONS" FIXTURE_LAYOUT="$FIXTURE_LAYOUT" \
    FIXTURE_PANES_DIR="$TEST_ROOT/panes" \
    FIXTURE_OPEN_SESSIONS="$FIXTURE_OPEN_SESSIONS" \
    PATH="$FAKE_BIN:/usr/bin:/bin" \
    "$TEST_ROOT/home/.tmuxinator/$script" "$@"
}

run_save() { run_devws_script save_state.sh; }
run_restore() {
  run_devws_script restore_state.sh \
    > "$TEST_ROOT/restore.out" 2> "$TEST_ROOT/restore.err"
}

state_file() { printf '%s' "$TEST_ROOT/home/.devws-state/sessions.tsv"; }

# dev-one: picker open, base terminal moved out of the root, one extra terminal.
# dev-two: picker closed, no extras.
two_workspace_fixture() {
  local one="$TEST_ROOT/project-one"
  local two="$TEST_ROOT/project-two"
  mkdir -p "$one/sub" "$two"

  printf 'dev-one\t%s\tclaude\tlvim\ndev-two\t%s\tcodex\tnvim\n' "$one" "$two" \
    > "$TEST_ROOT/sessions"
  write_panes dev-one \
    "%picker-one|1||devws-picker|$one" \
    "%editor-one|2|editor||$one" \
    "%agent-one|3|agent||$one" \
    "%terminal-one|4|terminal||$one/sub" \
    "%extra-one|5|extra||$one"
  write_panes dev-two \
    "%editor-two|1|editor||$two" \
    "%agent-two|2|agent||$two" \
    "%terminal-two|3|terminal||$two"
  FIXTURE_SESSIONS="$TEST_ROOT/sessions"
  FIXTURE_LAYOUT="LAYOUT-ONE"
}

test_save_writes_records() {
  setup
  two_workspace_fixture
  run_save

  [[ "$(sed -n '1p' "$(state_file)")" == "#devws-state${TAB}1" ]] ||
    fail "state file is missing its version header"
  grep -q "^W${TAB}dev-one${TAB}$TEST_ROOT/project-one${TAB}claude${TAB}lvim${TAB}1${TAB}LAYOUT-ONE$" \
    "$(state_file)" || fail "dev-one record is wrong: $(grep '^W' "$(state_file)")"
  grep -q "^W${TAB}dev-two${TAB}$TEST_ROOT/project-two${TAB}codex${TAB}nvim${TAB}0${TAB}" \
    "$(state_file)" || fail "dev-two should be recorded with the picker closed"
  grep -q "^P${TAB}dev-one${TAB}1${TAB}picker${TAB}" "$(state_file)" ||
    fail "the picker pane was not recognised by title"
  grep -q "^P${TAB}dev-one${TAB}4${TAB}terminal${TAB}$TEST_ROOT/project-one/sub$" \
    "$(state_file)" || fail "the base terminal's directory was not recorded"
  grep -q "^P${TAB}dev-one${TAB}5${TAB}extra${TAB}$TEST_ROOT/project-one$" "$(state_file)" ||
    fail "an untitled pane should be recorded as an extra terminal"
  [[ -z "$(find "$TEST_ROOT/home/.devws-state" -name 'sessions.??????')" ]] ||
    fail "a temporary state file was left behind"
}

test_save_adopts_terminal_in_legacy_workspace() {
  setup
  local one="$TEST_ROOT/project-one"
  mkdir -p "$one"
  printf 'dev-one\t%s\tclaude\tlvim\n' "$one" > "$TEST_ROOT/sessions"
  FIXTURE_SESSIONS="$TEST_ROOT/sessions"
  # A workspace from before the terminal pane marked itself: its title has been
  # replaced by the shell, so nothing claims the terminal role.
  write_panes dev-one \
    "%picker-one|1||devws-picker|$one" \
    "%editor-one|2|editor||$one" \
    "%agent-one|3|agent||$one" \
    "%terminal-one|4||user@host:~|$one" \
    "%extra-one|5||user@host:~|$one"
  run_save

  grep -q "^P${TAB}dev-one${TAB}4${TAB}terminal${TAB}" "$(state_file)" ||
    fail "the first unclaimed pane should take the terminal role"
  grep -q "^P${TAB}dev-one${TAB}5${TAB}extra${TAB}" "$(state_file)" ||
    fail "later unclaimed panes should stay extra terminals"
}

test_save_is_idempotent() {
  setup
  two_workspace_fixture
  run_save
  local before
  before="$(cat "$(state_file)")"
  run_save
  [[ "$(cat "$(state_file)")" == "$before" ]] ||
    fail "a second identical save changed the state file"
}

test_save_skips_during_restore() {
  setup
  two_workspace_fixture
  mkdir "$TEST_ROOT/home/.devws-state/restore.lock"
  run_save
  [[ ! -f "$(state_file)" ]] ||
    fail "save should be a no-op while a restore holds the lock"
}

test_restore_replays_workspaces() {
  setup
  two_workspace_fixture
  run_save
  : > "$TEST_ROOT/tmuxinator.log"
  : > "$TEST_ROOT/tmux.log"
  run_restore

  grep -q "^start --no-attach dev project_root=$TEST_ROOT/project-one agent=claude editor=lvim$" \
    "$TEST_ROOT/tmuxinator.log" ||
    fail "dev-one was not replayed: $(cat "$TEST_ROOT/tmuxinator.log")"
  grep -q "^start --no-attach dev project_root=$TEST_ROOT/project-two agent=codex editor=nvim$" \
    "$TEST_ROOT/tmuxinator.log" || fail "dev-two was not replayed"
  [[ "$(sed -n '1p' "$TEST_ROOT/tmuxinator.log")" == *project-one* ]] ||
    fail "workspaces were not restored in file order"
  grep -q "split-window -d -h -t %terminal-one -c $TEST_ROOT/project-one -P -F" "$TEST_ROOT/tmux.log" ||
    fail "the extra terminal was not recreated: $(grep split-window "$TEST_ROOT/tmux.log" || true)"
  grep -q "set-option -p -t %split-1 @devws_role extra" "$TEST_ROOT/tmux.log" ||
    fail "a recreated terminal was not tagged with its role"
  grep -q "select-layout -t dev-one: LAYOUT-ONE" "$TEST_ROOT/tmux.log" ||
    fail "the saved layout was not applied"
  grep -q "send-keys -t %terminal-one cd $TEST_ROOT/project-one/sub Enter" "$TEST_ROOT/tmux.log" ||
    fail "the base terminal was not returned to its saved directory"
  grep -qx 'dev-one' "$TEST_ROOT/restore.out" ||
    fail "restore should report the sessions it rebuilt on stdout"
  [[ ! -d "$TEST_ROOT/home/.devws-state/restore.lock" ]] ||
    fail "the restore lock was not released"
}

test_restore_skips_open_and_missing() {
  setup
  two_workspace_fixture
  run_save
  rm -rf "$TEST_ROOT/project-two"
  : > "$TEST_ROOT/tmuxinator.log"
  FIXTURE_OPEN_SESSIONS="dev-one"
  run_restore

  [[ ! -s "$TEST_ROOT/tmuxinator.log" ]] ||
    fail "restore started a workspace it should have skipped: $(cat "$TEST_ROOT/tmuxinator.log")"
  [[ ! -s "$TEST_ROOT/restore.out" ]] ||
    fail "restore reported a session it did not rebuild"
  grep -q 'folder is gone' "$TEST_ROOT/restore.err" ||
    fail "a workspace whose folder was deleted should be reported on stderr"
}

test_restore_falls_back_to_default_agent() {
  setup
  mkdir -p "$TEST_ROOT/project-one"
  printf '#devws-state\t1\nW\tdev-one\t%s\tghostwriter\tedlin\t1\t\n' \
    "$TEST_ROOT/project-one" > "$(state_file)"
  write_panes dev-one "%terminal-one|1|terminal||$TEST_ROOT/project-one"
  run_restore

  grep -q 'agent=claude editor=lvim' "$TEST_ROOT/tmuxinator.log" ||
    fail "an unknown agent/editor should fall back to the configured defaults"
  grep -q 'unknown agent' "$TEST_ROOT/restore.err" ||
    fail "the fallback should be reported on stderr"
}

test_restore_closes_picker() {
  setup
  mkdir -p "$TEST_ROOT/project-two"
  printf '#devws-state\t1\nW\tdev-two\t%s\tcodex\tnvim\t0\t\n' \
    "$TEST_ROOT/project-two" > "$(state_file)"
  write_panes dev-two \
    "%picker-two|1||devws-picker|$TEST_ROOT/project-two" \
    "%terminal-two|2|terminal||$TEST_ROOT/project-two"
  run_restore

  grep -q 'kill-pane -t %picker-two' "$TEST_ROOT/tmux.log" ||
    fail "a workspace saved with the picker closed should have it killed"
}

FAKE_BIN="$TEST_ROOT/bin"
make_fake_bin "$FAKE_BIN"

test_save_writes_records
test_save_adopts_terminal_in_legacy_workspace
test_save_is_idempotent
test_save_skips_during_restore
test_restore_replays_workspaces
test_restore_skips_open_and_missing
test_restore_falls_back_to_default_agent
test_restore_closes_picker

printf 'Workspace state tests passed.\n'
