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

created=""
skipped=""
case "$agent" in
  claude)
    if [ -e "$project_root/CLAUDE.md" ]; then
      skipped="CLAUDE.md already exists"
    else
      cp "$template_root/CLAUDE.md" "$project_root/CLAUDE.md"
      created="CLAUDE.md"
    fi
    mkdir -p "$project_root/.claude/skills"
    cp -Rn "$template_root/skills/." "$project_root/.claude/skills/"
    ;;
  codex)
    if [ -e "$project_root/AGENTS.md" ]; then
      skipped="AGENTS.md already exists"
    else
      sed 's/CLAUDE\.md/AGENTS.md/g; s/Claude Code/Codex/g' \
        "$template_root/CLAUDE.md" > "$project_root/AGENTS.md"
      created="AGENTS.md"
    fi
    mkdir -p "$project_root/.agents/skills"
    cp -Rn "$template_root/skills/." "$project_root/.agents/skills/"
    ;;
  *)
    printf 'Unsupported devws agent: %s\n' "$agent" >&2
    exit 2
    ;;
esac

if [ -n "$created" ]; then
  printf 'Initialized %s with default skills' "$created"
else
  printf '%s; added only missing skills' "$skipped"
fi
