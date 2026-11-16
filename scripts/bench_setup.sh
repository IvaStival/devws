#!/usr/bin/env bash
# Sets up a prepared benchmark run (bench_team.sh) the same way every time,
# before its leader opens: makes the reviewed task, configures its runner and
# checks from the fixture (runner.json, checks.json) and turns the code graph
# on or off. Prints the task id.
#
#   bench_setup.sh <folder> <fixture> <graph on|off>

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURES="${BENCH_FIXTURES:-$ROOT/agentic/bench/fixtures}"
DEVAGENTICS="${DEVAGENTICS:-devagentics}"

[[ $# -eq 3 && ( "$3" == on || "$3" == off ) ]] || {
  printf 'Usage: %s <folder> <fixture> <on|off>\n' "$(basename "$0")" >&2
  exit 2
}
folder="$1" fixture="$FIXTURES/$2" graph="$3"
[[ -f "$fixture/runner.json" && -f "$fixture/checks.json" ]] ||
  { printf '%s has no runner.json and checks.json\n' "$fixture" >&2; exit 1; }

task="$("$DEVAGENTICS" task start "$folder" | sed -n 1p)"
repo="$("$DEVAGENTICS" task status "$task" | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["repos"][0]["name"])')"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# The runner takes the image by its immutable id, found from the fixture's tag.
tag="$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["image_tag"])' "$fixture/runner.json")"
image="$(docker image inspect "$tag" --format '{{.Id}}')"
python3 -I - "$fixture/runner.json" "$image" > "$scratch/runner.json" <<'PY'
import json, sys
runner = json.load(open(sys.argv[1]))
runner["configuration"]["image"] = sys.argv[2]
print(json.dumps({"configuration": runner["configuration"]}))
PY
"$DEVAGENTICS" workflow "$task" runner_configure --input-file "$scratch/runner.json" >/dev/null

# Check profiles go through the leader's own tool, as a leader would set them.
python3 -I - "$fixture/checks.json" "$repo" > "$scratch/mcp.jsonl" <<'PY'
import json, sys
checks = json.load(open(sys.argv[1]))
for message in [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "bench", "version": "0"}}},
    {"jsonrpc": "2.0", "method": "notifications/initialized"},
    {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "configure_checks", "arguments": {"repo": sys.argv[2], "configuration": checks}}},
]:
    print(json.dumps(message))
PY
"$DEVAGENTICS" mcp "$task" < "$scratch/mcp.jsonl" | grep -q '"isError":false' ||
  { printf 'configuring the checks of %s failed\n' "$task" >&2; exit 1; }

"$DEVAGENTICS" task graph "$task" "$graph" >/dev/null
printf '%s\n' "$task"
