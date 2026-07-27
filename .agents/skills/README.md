# Default reusable skills

These are the default agent skills seeded into new projects. Deploy them for
Claude, Codex, or both with:

```bash
make scaffold-project DIR=/path/to/project SKILLS=1
make scaffold-project DIR=/path/to/project AGENT=codex SKILLS=1
make scaffold-project DIR=/path/to/project AGENT=both SKILLS=1
```

That creates the matching `CLAUDE.md` and/or `AGENTS.md` instructions and
copies these skills into `.claude/skills` and/or `.agents/skills`.

## Included skills

| Skill | Purpose |
| ----- | ------- |
| `commit` | Create commits following the standard message format (no AI credits). |
| `doc-generator` | Generate/update architectural docs after significant changes. |
| `spec-plan` | Spec → Plan → Execute workflow for non-trivial tasks. |

## Skill format

Each skill is a directory containing a `SKILL.md` with YAML frontmatter:

```markdown
---
name: my-skill
description: When to use this skill — be specific about triggers so the model
  knows when to invoke it. Include the phrases a user might say.
---

# My Skill

Step-by-step instructions the model follows when the skill is invoked.
```

To add a new default skill: create `templates/claude/skills/<name>/SKILL.md`
following the format above, then re-run `make scaffold-project ... SKILLS=1`.
