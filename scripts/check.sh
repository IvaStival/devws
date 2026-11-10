#!/usr/bin/env bash
# Every check devws has: syntax, executable bits and dev.yml first, then every
# test suite (scripts/test_*.sh), in parallel — each to its own log, shown
# only when it fails.
#
#   scripts/check.sh           everything (a few minutes)
#   scripts/check.sh --quick   the syntax checks and the suites that need no
#                              tmux server or real timing (seconds)
#
# DEVWS_CHECK_JOBS suites run at once (4 by default): more makes the suites
# that wait on timings flaky on a loaded machine.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
quick=""
[ "${1:-}" = --quick ] && quick=1
parallel="${DEVWS_CHECK_JOBS:-4}"

# The suites --quick runs: no tmux server, no waiting on real time.
quick_suites="test_picker test_recent test_shared_files test_worktree test_github_repo
  test_jungleforge test_link_scripts test_tidy test_memory test_guide test_agentic_install"

shell_scripts=(
  "$ROOT/install.sh" "$ROOT/delete.sh"
  "$ROOT"/scripts/*.sh "$ROOT/shell/render_mermaid_preview.sh"
  "$ROOT"/config/tmux/*.sh "$ROOT"/config/tmuxinator/*.sh
)

for file in "${shell_scripts[@]}"; do
  bash -n "$file"
  [[ -x "$file" ]] || {
    printf 'Not executable: %s\n' "$file" >&2
    exit 1
  }
done
zsh -n "$ROOT/shell/devws.zsh"
zsh -n "$ROOT/bin/devws"
[[ -x "$ROOT/bin/devws" ]] || { printf 'Not executable: %s\n' "$ROOT/bin/devws" >&2; exit 1; }

# default-keys.conf wipes every binding made before it: one there is dead.
dead_keys="$(awk '/^source-file .*default-keys\.conf/ { exit } /^[[:space:]]*(bind|bind-key|unbind) / { print FILENAME ":" NR ": " $0 }' \
  "$ROOT/config/tmux/tmux.conf")"
[ -z "$dead_keys" ] || {
  printf 'tmux.conf: bound before default-keys.conf, which wipes it:\n%s\n' "$dead_keys" >&2
  exit 1
}

ruby -rerb -ryaml -e '
  render = lambda do |settings|
    context = Object.new
    context.instance_variable_set(:@settings, settings)
    rendered = ERB.new(File.read(ARGV[0])).result(context.instance_eval { binding })
    YAML.safe_load(rendered, aliases: true)
  end
  agent_pane = lambda do |config|
    config["windows"][0]["workspace"]["panes"].find { |pane| pane.is_a?(Hash) && pane.key?("agent") }["agent"]
  end
  config = render.call({ "agent" => "codex", "project_root" => Dir.pwd })
  # A stray " #" in an unquoted value silently truncates it as a comment.
  abort "dev.yml: pre_window was cut short" unless config["pre_window"].end_with?("; clear")
  abort "dev.yml: a new workspace resumes its agent" if agent_pane.call(config).include?("--resume")
  restored = render.call({ "agent" => "codex", "project_root" => Dir.pwd, "resume" => "1" })
  abort "dev.yml: a restored workspace does not resume its agent" unless agent_pane.call(restored).end_with?("codex --resume")
' "$ROOT/config/tmuxinator/dev.yml"

# The agent team's template, the same way: a leader that takes the arguments
# file (quotes in its path kept whole) and the log's command.
ruby -rerb -ryaml -e '
  context = Object.new
  context.instance_variable_set(:@settings, {
    "agent" => "codex", "project_root" => Dir.pwd, "task" => "team-1",
    "agent_args" => "/tmp/it'"'"'s/args", "agentic_command" => "devagentics"
  })
  config = YAML.safe_load(ERB.new(File.read(ARGV[0])).result(context.instance_eval { binding }), aliases: true)
  abort "agentic.yml: pre_window was cut short" unless config["pre_window"].end_with?("; clear")
  panes = config["windows"][0]["workspace"]["panes"].map { |pane| pane.values.first }
  abort "agentic.yml: the leader lost its arguments file" unless panes.find { |pane| pane.include?("agent_runner.sh") }.end_with?("codex --args-file \x27/tmp/it\x27\\\x27\x27s/args\x27")
  abort "agentic.yml: the board pane does not run the board of the task" unless panes.last.end_with?("devagentics board team-1")
' "$ROOT/config/tmuxinator/agentic.yml"

if command -v luac >/dev/null 2>&1; then
  while IFS= read -r -d '' lua_file; do
    luac -p "$lua_file"
  done < <(find "$ROOT/config/nvim" -name '*.lua' -print0)
fi

# The agent team's program (agentic/), when cargo is there.
if [ -z "$quick" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
  if command -v cargo >/dev/null 2>&1; then
    (
      cd "$ROOT/agentic"
      cargo fmt --check
      cargo clippy --all-targets --quiet -- -D warnings
      cargo test --quiet
    )
    printf '✓ agentic (fmt, clippy, tests)\n'
  else
    printf '– agentic skipped: cargo not found\n'
  fi
fi

suites=()
for suite in "$ROOT"/scripts/test_*.sh; do
  name="$(basename "$suite" .sh)"
  if [ -n "$quick" ]; then
    case " $(printf '%s' "$quick_suites" | tr '\n' ' ') " in *" $name "*) ;; *) continue ;; esac
  fi
  suites+=("$suite")
done

logs="$(mktemp -d "${TMPDIR:-/tmp}/devws-check.XXXXXX")"
trap 'rm -rf "$logs"' EXIT
failed=0
retry=()

# A batch of suites at a time (bash 3.2 has no `wait -n`), each reported as
# it is collected.
for ((start = 0; start < ${#suites[@]}; start += parallel)); do
  pids=()
  for suite in "${suites[@]:start:parallel}"; do
    "$suite" > "$logs/$(basename "$suite" .sh).log" 2>&1 &
    pids+=("$!")
  done
  index=0
  for suite in "${suites[@]:start:parallel}"; do
    name="$(basename "$suite" .sh)"
    if wait "${pids[index]}"; then
      printf '✓ %s\n' "$name"
    else
      retry+=("$suite")
    fi
    index=$((index + 1))
  done
done

# A suite that failed alongside others runs again alone: several tmux servers
# at once can briefly run the machine out of terminals ("fork failed: Device
# not configured"). Passing alone is said, not hidden.
for suite in "${retry[@]+"${retry[@]}"}"; do
  name="$(basename "$suite" .sh)"
  if "$suite" > "$logs/$name.retry.log" 2>&1; then
    printf '✓ %s (failed beside the others, passed alone: %s)\n' "$name" \
      "$(grep -v '^\s*$' "$logs/$name.log" | tail -n 1)"
  else
    printf '✗ %s\n' "$name"
    sed 's/^/    /' "$logs/$name.retry.log"
    failed=1
  fi
done

if [ "$failed" = 1 ]; then
  printf 'Some checks failed.\n' >&2
  exit 1
fi
printf 'All checks passed.\n'
