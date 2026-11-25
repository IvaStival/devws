#!/usr/bin/env bash
# Seed project-level instructions and skills for the selected devws agent.
# Existing instruction files and skills are never overwritten.

set -u

project_root="${1:-}"
agent="${2:-claude}"
template_root="$HOME/.tmuxinator/project-template"

if [ ! -d "$project_root" ]; then
  printf 'Project folder not found: %s\n' "$project_root" >&2
  exit 2
fi
if [ ! -r "$template_root/CLAUDE.md" ]; then
  printf 'Project agent template is not deployed\n' >&2
  exit 2
fi

# The program's module seeds the project (init_project): "created:<file>" or
# "skipped:<why>".
# shellcheck source=agents.sh
. "$(cd "$(dirname "$0")" && pwd)/agents.sh"
created=""
skipped=""
outcome="$(agent_hook "$agent" init_project "$project_root" "$template_root")" || exit 2
case "$outcome" in
  created:*) created="${outcome#created:}" ;;
  skipped:*) skipped="${outcome#skipped:}" ;;
esac

if [ -n "$created" ]; then
  printf 'Initialized %s with default skills' "$created"
else
  printf '%s; added only missing skills' "$skipped"
fi
