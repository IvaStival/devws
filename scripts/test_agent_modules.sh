#!/usr/bin/env bash
# The AI programs are modules (config/tmuxinator/agents/<n>-<kind>/agent.sh,
# docs/architecture/agent-modules.md): each defines the hooks it must, and
# nothing else at its top level; no script outside them names a program in a
# branch; and installing a program is adding its folder, nothing more.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPTS_SRC="$ROOT/config/tmuxinator"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/devws-agent-modules.XXXXXX")"
REAL_TMUX="$(command -v tmux)"
SOCKET="devws-agent-modules-test-$$"

cleanup() {
  "$REAL_TMUX" -L "$SOCKET" kill-server 2>/dev/null || true
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

fail() {
  printf 'Agent modules test failed: %s\n' "$*" >&2
  exit 1
}

# Every module defines the required hooks; its top level holds only its own
# <kind>_ functions, so loading every module changes nothing else.
test_each_module_defines_its_hooks_and_only_functions() {
  local module kind hook line name
  for module in "$SCRIPTS_SRC"/agents/*/agent.sh; do
    kind="$(basename "$(dirname "$module")")"
    kind="${kind#*-}"
    for hook in $(bash -c '. "$1"; printf "%s" "$agent_required_hooks"' _ "$SCRIPTS_SRC/agents.sh"); do
      bash -c '. "$1"; agent_defines "$2" "$3"' _ "$SCRIPTS_SRC/agents.sh" "$kind" "$hook" ||
        fail "the $kind module does not define $hook"
    done
    while IFS= read -r line; do
      case "$line" in
        '' | '#'* | ' '* | '}' | '}'*) ;;
        *'() {'*)
          name="${line%%()*}"
          [[ "$name" == "${kind}_"* ]] || fail "the $kind module defines $name, not a ${kind}_ hook"
          ;;
        *) fail "the $kind module runs something when loaded: $line" ;;
      esac
    done < "$module"
  done
}

# A program is named only in its module: a branch on one anywhere else is a
# place the next program would be missed in. Defaults ("claude" when nothing
# is chosen) are not branches.
test_no_script_branches_on_a_program() {
  local names="claude|codex|opencode|antigravity|agy" found
  found="$(grep -nE "^[[:space:]]*\(?($names)([[:space:]]*\|[^)]*)?\)|(=|!=)[[:space:]]+\"?($names)\"?[[:space:]]*\]|Kind::parse\(\"" \
    "$SCRIPTS_SRC"/*.sh "$ROOT/bin/devws" "$ROOT/install.sh" 2>/dev/null || true)"
  [ -z "$found" ] || fail "a script branches on a program outside its module:
$found"
  found="$(ls "$SCRIPTS_SRC" | grep -E "^($names)_" || true)"
  [ -z "$found" ] || fail "a program's own file is outside its module: $found"
}

# Installing a program is adding its folder: it is offered, listed as an
# account, shown in the header, registered, and run, no other file touched.
test_a_new_program_is_installed_by_adding_its_module() {
  local copy="$TEST_ROOT/tmuxinator" home="$TEST_ROOT/home" output pane
  cp -R "$SCRIPTS_SRC" "$copy"
  mkdir -p "$copy/agents/90-fakeai" "$home/.fakeai" "$TEST_ROOT/bin" "$TEST_ROOT/started"
  cat > "$copy/agents/90-fakeai/agent.sh" <<'EOF'
#!/usr/bin/env bash
# A program devws has never heard of.

fakeai_program() { printf 'fakeai'; }
fakeai_default_home() { printf '%s' "$HOME/.fakeai"; }
fakeai_launch() {
  shift 2
  fakeai "$@"
}
fakeai_interrupt_keys() { printf 'C-c'; }
fakeai_limits() { printf '7 8'; }
EOF
  printf '%s\n' '#!/usr/bin/env bash' "printf '%s\\n' \"\$@\" > \"$TEST_ROOT/started/fakeai\"" 'exec sleep 600' > "$TEST_ROOT/bin/fakeai"
  printf '%s\n' '#!/usr/bin/env bash' "exec \"$REAL_TMUX\" -L \"$SOCKET\" -f /dev/null \"\$@\"" > "$TEST_ROOT/bin/tmux"
  chmod +x "$TEST_ROOT/bin/fakeai" "$TEST_ROOT/bin/tmux"
  : > "$copy/.env"

  # Only the stub programs: not the ones this machine has installed.
  run() {
    env -i HOME="$home" PATH="$TEST_ROOT/bin:/usr/bin:/bin" TMPDIR="$TEST_ROOT" DEVWS_AGENTS="${agents-}" \
      DEVWS_NOTIFY_COMMAND=true "$@"
  }
  agents=""
  [ "$(run bash -c '. "$1/agents.sh"; agent_names' _ "$copy")" = fakeai ] ||
    fail "an installed new program is not offered: $(run bash -c '. "$1/agents.sh"; agent_names' _ "$copy")"
  [ "$(run "$copy/agent_cli.sh" accounts)" = "fakeai fakeai" ] || fail "the new program is not an account"
  [ "$(run bash -c '. "$1/agents.sh"; agent_kinds' _ "$copy" | tail -n 1)" = fakeai ] ||
    fail "the new program is not a kind the menu offers to register"
  output="$(run "$copy/header_agents.sh" | sed 's/#\[[^]]*\]//g')"
  case "$output" in
    *"5h"*"wk"*fakeai*"7%"*"8%"*) ;;
    *) fail "the header has no row for the new program: $output" ;;
  esac
  run env DEVWS_ENV_FILE="$copy/.env" "$copy/register_agent.sh" add mine fakeai "$home/.fakeai" >/dev/null ||
    fail "the new program could not be registered"
  grep -q '^DEVWS_AGENT_mine="fakeai ' "$copy/.env" || fail "the registration was not written: $(cat "$copy/.env")"

  # The runner starts it, in an agent pane, with devws's arguments.
  printf '%s\0' --model fast-1 > "$TEST_ROOT/args"
  mkdir -p "$home/.tmuxinator"
  run tmux new-session -d -s work -x 120 -y 30 "env DEVWS_AGENTS=mine $copy/agent_runner.sh mine --args-file $TEST_ROOT/args"
  for _ in $(seq 1 30); do [ -s "$TEST_ROOT/started/fakeai" ] && break; sleep 0.2; done
  [ "$(cat "$TEST_ROOT/started/fakeai" 2>/dev/null)" = "$(printf -- '--model\nfast-1')" ] ||
    fail "the runner did not start the new program: $(cat "$TEST_ROOT/started/fakeai" 2>/dev/null)"
  pane="$(run tmux list-panes -t work: -F '#{pane_id}')"
  [ "$(run tmux show-options -pqv -t "$pane" @devws_role)" = agent ] || fail "its pane is not an agent pane"
}

test_each_module_defines_its_hooks_and_only_functions
test_no_script_branches_on_a_program
test_a_new_program_is_installed_by_adding_its_module

printf 'Agent module tests passed\n'
