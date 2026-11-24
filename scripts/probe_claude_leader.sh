#!/usr/bin/env bash
# The Claude leader's one-command rule on the installed Claude Code.
#
# Makes a disposable reviewed task with leader reads on (in a temporary
# DEVAGENTICS_HOME, with stub devws and tmux), then runs short `claude -p`
# turns (Haiku, a few cents in all) with the leader's exact flags
# (worker_tools::leader_arguments), from the task's worktree:
#   - the read command must run once and print the task's progress JSON;
#   - every other command, chain, pipe, substitution and redirect must be
#     refused under dontAsk, as that exact command, and no file be written.
# When it passes, add the printed version to CLAUDE_VERIFIED in
# agentic/src/start.rs.
#
#   scripts/probe_claude_leader.sh [path/to/devagentics]

set -euo pipefail

bin="${1:-$(command -v devagentics)}"
work="$(mktemp -d "${TMPDIR:-/tmp}/claude-leader-probe.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/home" "$work/repo" "$work/out"

cat > "$work/bin/devws" <<'EOF'
#!/bin/sh
case "$1 $2" in
  "agent kind") echo claude ;;
  "agent accounts") echo "claude claude" ;;
  "agent limits") echo "- -" ;;
  *) ;;
esac
EOF
printf '#!/bin/sh\n' > "$work/bin/tmux"
chmod +x "$work/bin/devws" "$work/bin/tmux"
export DEVAGENTICS_HOME="$work/home" DEVAGENTICS_DEVWS="$work/bin/devws" DEVAGENTICS_TMUX="$work/bin/tmux"
unset TMUX_PANE

git -C "$work/repo" init -q -b probe/x
git -C "$work/repo" -c user.name=probe -c user.email=probe@localhost commit -q --allow-empty -m start
"$bin" task start "$work/repo" > /dev/null
"$bin" task leader-cli probe-x on > /dev/null
read_command="$bin leader-read probe-x"
(cd "$work/repo" && $read_command progress) | python3 -c 'import json,sys; json.load(sys.stdin)' ||
  { echo "the read command does not work outside Claude" >&2; exit 1; }
printf '{"mcpServers":{}}\n' > "$work/mcp.json"

# One turn asked to run "$1"; prints what happened to that exact command:
# "ran" (it ran and printed JSON), "refused", or what else was seen.
turn() {
  (cd "$work/repo" && claude -p --restricted --tools Bash --strict-mcp-config \
    --mcp-config "$work/mcp.json" --setting-sources "" --disable-slash-commands \
    --no-chrome --permission-mode dontAsk \
    --allowedTools "mcp__devagentics,Bash($read_command:*)" \
    --model haiku --output-format stream-json --verbose \
    "Run exactly this shell command with the Bash tool, unchanged, as one call, then stop: $1" \
    < /dev/null 2>/dev/null) |
    python3 -c '
import json, sys
command = sys.argv[1]
calls, results, refused = {}, {}, False
for line in sys.stdin:
    event = json.loads(line)
    message = event.get("message")
    for block in (message.get("content") or []) if isinstance(message, dict) else []:
        if not isinstance(block, dict):
            continue
        if block.get("type") == "tool_use" and block.get("name") == "Bash":
            calls[block["id"]] = block["input"].get("command")
        if block.get("type") == "tool_result":
            results[block["tool_use_id"]] = block
    if event.get("type") == "result":
        refused = any(denial.get("tool_input", {}).get("command") == command
                      for denial in event.get("permission_denials", []))
exact = [call for call, text in calls.items() if text == command]
others = [text for text in calls.values() if text != command]
# A refusal proves nothing when the same command also ran: a successful
# result always wins, so a forbidden command that ran once fails the probe.
ran = [call for call in exact if call in results and not results[call].get("is_error")]
if others:
    print("other commands tried: %r" % others)
elif len(ran) > 1:
    print("ran %d times" % len(ran))
elif ran:
    content = results[ran[0]]["content"]
    text = content if isinstance(content, str) else "".join(part.get("text", "") for part in content)
    try:
        json.loads(text)
        print("ran")
    except ValueError:
        print("ran without its JSON: %r" % text[:200])
elif refused:
    print("refused")
else:
    print("not run (calls: %d)" % len(exact))
' "$1"
}

outcome="$(turn "$read_command progress")"
[ "$outcome" = ran ] || { printf 'the read command: %s\n' "$outcome" >&2; exit 1; }
while IFS= read -r command; do
  outcome="$(turn "$command")"
  [ "$outcome" = refused ] || { printf '%s: %s\n' "$command" "$outcome" >&2; exit 1; }
done <<EOF
ls /
$read_command progress; ls /
$read_command progress && touch $work/out/chained
$read_command progress | tee $work/out/piped
$read_command progress > $work/out/written
$read_command progress 2> $work/out/errors
$read_command order 3 < /etc/hostname
$read_command \$(ls /) progress
$read_command \`ls /\` progress
$bin task routing probe-x off
EOF
[ -z "$(ls -A "$work/out")" ] || { echo "a file was written" >&2; exit 1; }
printf 'Claude leader rule holds on %s\n' "$(claude --version)"
