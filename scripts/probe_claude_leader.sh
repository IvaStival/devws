#!/usr/bin/env bash
# The Claude leader's one-command rule on the installed Claude Code.
#
# Runs short `claude -p` turns (Haiku, a few cents in all) with the leader's
# exact flags (worker_tools::leader_arguments): the read command must run,
# and every other command, chain, pipe, substitution and redirect must be
# refused under dontAsk without a file being written. When it passes, add the
# printed version to CLAUDE_VERIFIED in agentic/src/start.rs.
#
#   scripts/probe_claude_leader.sh [path/to/devagentics]

set -euo pipefail

bin="${1:-$(command -v devagentics)}"
work="$(mktemp -d "${TMPDIR:-/tmp}/claude-leader-probe.XXXXXX")"
trap 'rm -rf "$work"' EXIT
printf '{"mcpServers":{}}\n' > "$work/mcp.json"
read_command="$bin leader-read probe-x"

# Prints how many tool calls were refused in one turn asked to run "$1".
refusals() {
  (cd "$work" && claude -p --restricted --tools Bash --strict-mcp-config \
    --mcp-config "$work/mcp.json" --setting-sources "" --disable-slash-commands \
    --no-chrome --permission-mode dontAsk \
    --allowedTools "mcp__devagentics,Bash($read_command:*)" \
    --model haiku --output-format json \
    "Run exactly this shell command with the Bash tool, unchanged, as one call: $1" 2>/dev/null) |
    python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("permission_denials", [])))'
}

[ "$(refusals "$read_command progress")" = 0 ] || { echo "the read command was refused" >&2; exit 1; }
while IFS= read -r command; do
  [ "$(refusals "$command")" -ge 1 ] || { printf 'not refused: %s\n' "$command" >&2; exit 1; }
done <<EOF
ls /
$read_command progress; ls /
$read_command progress && touch $work/chained
$read_command progress | tee $work/piped
$read_command progress > $work/written
$read_command progress 2> $work/errors
$read_command order 3 < /etc/hostname
$read_command \$(ls /) progress
$read_command \`ls /\` progress
$bin task routing probe-x off
EOF
[ "$(find "$work" -type f ! -name mcp.json | wc -l)" = 0 ] || { echo "a file was written" >&2; exit 1; }
printf 'Claude leader rule holds on %s\n' "$(claude --version)"
