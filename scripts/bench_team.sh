#!/usr/bin/env bash
# Prepares one benchmark run of an agent team: a fresh git repo holding a
# fixture (agentic/bench/fixtures/<fixture>), its fixed goal, and the
# commands to start a legacy or a reviewed team on it. It starts no agent
# itself: the team runs on your accounts, so you start it.
#
#   bench_team.sh <fixture> <legacy|reviewed> [folder]
#
# A fixture is either a small project of its own, or a real project at a
# fixed commit (its source.commit) with acceptance tests laid over it (its
# tests/): BENCH_SOURCE names that project's repo, so no path or project
# name lives in devws.
#
# The folder defaults to ~/Projects/devws-bench/<fixture>-<mode>-<time>. When
# the team has stopped, `devagentics bench report <task>` measures the run;
# docs/user/devagentics-bench.md has the protocol.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURES="${BENCH_FIXTURES:-$ROOT/agentic/bench/fixtures}"

usage() {
  printf 'Usage: %s <fixture> <legacy|reviewed> [folder]\n' "$(basename "$0")" >&2
  printf 'Fixtures: %s\n' "$(ls "$FIXTURES" | tr '\n' ' ')" >&2
  exit 2
}

[[ $# -ge 2 && $# -le 3 ]] || usage
fixture="$1"
mode="$2"
source="$FIXTURES/$fixture"
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
if [[ -f "$source/source.commit" ]]; then
  commit="$(tr -d '[:space:]' < "$source/source.commit")"
  if [[ -z "${BENCH_SOURCE:-}" ]]; then
    printf '%s is a project at a fixed commit: set BENCH_SOURCE to its repo\n' "$fixture" >&2
    exit 1
  fi
  git -C "$BENCH_SOURCE" cat-file -e "$commit^{commit}" 2>/dev/null || {
    printf '%s has no commit %s\n' "$BENCH_SOURCE" "$commit" >&2
    exit 1
  }
  mkdir -p "$folder"
  git -C "$BENCH_SOURCE" archive "$commit" | tar -x -C "$folder"
  # A team's history in the project would make the run's task start from it.
  rm -rf "$folder/.devagentics"
  [[ -d "$source/tests" ]] && cp -R "$source/tests/." "$folder/"
  cp "$source/GOAL.md" "$folder/GOAL.md"
else
  cp -R "$source" "$folder"
  find "$folder" -name __pycache__ -prune -exec rm -rf {} +
  printf '__pycache__/\n' > "$folder/.gitignore"
fi
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
