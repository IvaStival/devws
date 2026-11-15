#!/usr/bin/env bash
# Prepares one benchmark run of an agent team: a fresh git repo holding a
# fixture (agentic/bench/fixtures/<fixture>), its fixed goal, and the
# commands to start a legacy or a reviewed team on it. It starts no agent
# itself: the team runs on your accounts, so you start it.
#
#   bench_team.sh <fixture> <legacy|reviewed> [folder]
#
# The folder defaults to ~/Projects/devws-bench/<fixture>-<mode>-<time>. When
# the team has stopped, `devagentics bench report <task>` measures the run;
# docs/user/devagentics-bench.md has the protocol.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  printf 'Usage: %s <fixture> <legacy|reviewed> [folder]\n' "$(basename "$0")" >&2
  printf 'Fixtures: %s\n' "$(ls "$ROOT/agentic/bench/fixtures" | tr '\n' ' ')" >&2
  exit 2
}

[[ $# -ge 2 && $# -le 3 ]] || usage
fixture="$1"
mode="$2"
source="$ROOT/agentic/bench/fixtures/$fixture"
[[ -d "$source" ]] || { printf 'No fixture %s\n' "$fixture" >&2; usage; }
case "$mode" in
  legacy|reviewed) ;;
  *) printf 'The mode is legacy or reviewed, not %s\n' "$mode" >&2; usage ;;
esac
stamp="$(date +%Y%m%d-%H%M%S)"
folder="${3:-$HOME/Projects/devws-bench/$fixture-$mode-$stamp}"
if [[ -e "$folder" ]]; then
  printf '%s exists already: each run starts from a fresh copy\n' "$folder" >&2
  exit 1
fi

mkdir -p "$(dirname "$folder")"
cp -R "$source" "$folder"
find "$folder" -name __pycache__ -prune -exec rm -rf {} +
printf '__pycache__/\n' > "$folder/.gitignore"
git -C "$folder" init -q -b main
# The team commits there too (its history): a machine without a git identity
# of its own gets a neutral one for this repo only.
if ! git -C "$folder" config user.name >/dev/null; then
  git -C "$folder" config user.name "devws bench"
  git -C "$folder" config user.email "bench@devws.invalid"
fi
git -C "$folder" add -A
git -C "$folder" commit -q -m "bench: $fixture fixture"
git -C "$folder" switch -q -c "bench/$mode-$stamp"

cat <<EOF
Prepared $fixture for a $mode run in
  $folder  (branch bench/$mode-$stamp)

Start the team:
EOF
if [[ "$mode" == legacy ]]; then
  cat <<EOF
  devagentics task start "$folder" --legacy
  devws agentic "$folder"
EOF
else
  cat <<EOF
  devws agentic "$folder"
EOF
fi
cat <<EOF

Give the leader the goal in $folder/GOAL.md, unchanged, and answer its
questions as you would on every run. The check is
  python3 -m unittest discover -s tests -t .
with the acceptance tests in tests/acceptance/ protected.

When the team has stopped:
  devagentics bench report <task> --json > <runs folder>/$fixture-$mode-$stamp.json
EOF
