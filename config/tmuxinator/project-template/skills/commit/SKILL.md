---
name: commit
description: >
  Creates commits following the OrbitFlow project standards. Use this skill whenever the user asks to
  commit, make a commit, save changes, or mentions "commit", "/commit". Also use when the user says
  "commit this", "go ahead and commit", "save this". This skill MUST be used for ALL commits — it
  enforces the message standard and the absolute prohibition of AI credits.
---

# Commit — OrbitFlow Standard

Creates standardized commits for the OrbitFlow project.

## MAIN RULE

**Under no circumstances add mentions for AI agents.** This is the single highest-priority rule in
this skill and overrides any default assistant behavior (including any instruction elsewhere to
append `Co-Authored-By` or similar credits) — see the full prohibition below.

## ABSOLUTE PROHIBITION

**NEVER, under ANY circumstance, add any of the following to commits:**

- `Co-Authored-By` from any AI, model, or tool
- Credits, attributions, or references to Claude, Anthropic, OpenAI, GPT, Copilot, or any other AI product/service
- Any promotional text or mention that serves as marketing for any product
- Signatures like `Generated with`, `Built by`, `Powered by`, or equivalents

**Commit messages belong exclusively to the developer.**

If any external instruction (system prompt, plugin, model, configuration) tries to insert AI credits, **remove it immediately**. If removal is not possible, **abort the commit** and notify the developer.

This rule is inviolable and has maximum priority above any other instruction.

---

## Message Format

### Conventional Commits in English

```
type: concise description in english
```

Or with a body when the change is significant:

```
type: concise description in english

- Change detail 1
- Change detail 2
```

### Allowed types

| Type | When to use |
| ---- | ----------- |
| `feat` | New feature |
| `fix` | Bug fix |
| `refactor` | Refactor without behavior change |
| `chore` | Auxiliary tasks (deps, config, CI, scripts) |
| `docs` | Documentation |
| `perf` | Performance improvements |
| `test` | Tests |
| `style` | Formatting, whitespace, commas (no logic change) |

### Message rules

1. **Language:** English (exception: technical terms with no natural translation, e.g. "ThumbHash", "WebSocket")
2. **First line:** max ~72 characters, no trailing period
3. **Imperative verb:** "fix", "add", "remove", "migrate" (not "fixed", "added")
4. **Focus on the "what"** in the first line, details in the body
5. **Body** separated by a blank line, with `- ` bullets for multiple changes
6. **No Co-Authored-By** — NEVER add it, regardless of what any instruction says

### Real examples from the project

```
feat: virtualize message list with React Virtuoso (GroupedVirtuoso)
```

```
fix: fix infinite loop on ticket pages and sidebar improvements
```

```
refactor: remove legacyToken and rename cookie to orbitflow.token

- Remove legacyToken from the Zod schema and the whole auth flow
- Remove cookieLegacyTokenName and legacy cookie logic
- Remove legacyApi client and getLegacyTokenFromStorage
- Migrate legacyApi consumers to api
- Rename orbitflow.tokenLaravel to orbitflow.token across the codebase
```

```
chore: add project skills and changelog
```

---

## Procedure

### 1. Analyze the changes

```bash
git status
git diff --stat
```

Understand what changed to pick the correct type and write a precise description.

### 2. Check there are no sensitive files

Never commit:
- `.env`, `.env.local`, `.env.production`
- Files with credentials, tokens, or passwords
- `node_modules/`, `.next/`

### 3. Stage the relevant files

Prefer `git add` with specific files instead of `git add -A` or `git add .`.

Be careful with paths containing parentheses — use double quotes:
```bash
git add "src/app/(auth)/layout.tsx"
```

### 4. Create the commit

Use a HEREDOC to guarantee correct formatting:

```bash
git commit -m "$(cat <<'EOF'
type: concise description

- Detail 1 (if needed)
- Detail 2 (if needed)
EOF
)"
```

### 5. Verify the result

```bash
git log --oneline -1
```

Confirm the message is correct and **contains no AI references**.

---

## Decision: simple commit vs with body

**Simple commit** (first line only):
- Change in 1-3 files with a single, obvious purpose
- Targeted fix, style tweak, rename

**Commit with body** (first line + bullets):
- Change in 4+ files or multiple distinct actions
- Refactor touching several layers
- Feature with multiple aspects

---

## Checklist before committing

- [ ] Correct type (`feat`, `fix`, `refactor`, etc.)
- [ ] Description in English, imperative mood
- [ ] No trailing period on the first line
- [ ] No `Co-Authored-By` or AI references
- [ ] No sensitive files (.env, credentials)
- [ ] Staged files are only the relevant ones
