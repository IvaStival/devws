---
name: spec-plan
description: >
  Spec Driven Development + Plan workflow for non-trivial tasks. Use this skill when the user asks to
  implement a feature, refactor, migration, or any non-trivial task that spans multiple files or steps.
  Also use it when the user mentions "spec", "plan", "specification", "let's plan", "create a plan", or
  when the task clearly needs more than 3 steps to complete. Do NOT use it for one-off fixes touching
  1-2 files, style tweaks, or tasks that take less than 5 minutes.
---

# Spec → Plan → Execute

A complete specification, planning, and execution workflow for non-trivial tasks.

## Flow Overview

```text
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  1. SPEC    │────▶│  2. PLAN    │────▶│  3. EXECUTE │────▶│  4. SAVE    │
│  Explore +  │     │  Detail     │     │  Implement  │     │  Document   │
│  Specify    │     │  the steps  │     │  the plan   │     │  in specs/  │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
       │                   │                   │                    │
   Ask the user       Ask the user        Execute step        Save spec +
   if it looks        if they want        by step with        plan + update
   good               to execute          checkpoints         README
```

---

## Phase 1 — Specification (Spec)

### Goal

Deeply understand WHAT needs to be done BEFORE planning HOW to do it.

### Procedure

1. **Explore the code** — Use Explore agents to understand the current state of the files involved, existing patterns, and dependencies
2. **Identify the scope** — List exactly what needs to change and what must NOT change
3. **Document requirements** — Write the spec with:
   - Context and motivation (why this needs to be done)
   - Functional requirements (what should happen)
   - Constraints and decisions (what NOT to do, chosen trade-offs)
   - Files involved (list with paths)
   - Success criteria (how to know it's done)
4. **Create the folder** in the format `specs/plans/YYYY-MM-DD-HHmmss_contextual-name/` (use `date "+%Y-%m-%d-%H%M%S"`)
5. **Save the spec** as `spec.md` inside the created folder

### Format of spec.md

```markdown
# Spec: {Descriptive title}

- **Date:** dd-mm-yyyy
- **Branch:** {current or suggested branch}
- **Status:** Specified

## Context

{Why is this change needed? What motivated it?}

## Requirements

1. {Functional requirement 1}
2. {Functional requirement 2}
...

## Constraints

- {What NOT to do}
- {Scope limits}

## Files Involved

| File | Action |
| ---- | ------ |
| `src/path/to/File` | Modify — {what} |
| `src/path/to/NewFile` | Create — {what} |

## Success Criteria

- [ ] {How to validate requirement 1 is met}
- [ ] {How to validate requirement 2 is met}
```

### Checkpoint

After writing the spec, **ask the user**:

> "The specification is ready. Would you like to review anything before moving on to the plan?"

Only advance to Phase 2 once the user approves.

---

## Phase 2 — Planning (Plan)

### Goal

Detail the implementation step-by-step with concrete, verifiable steps.

### Procedure

1. **Base it on the spec** — Each spec requirement should become one or more plan steps
2. **Order by dependency** — Steps that depend on others come later
3. **Include verification** — Each step must state how to verify it is correct
4. **Save the plan** as `plan.md` in the same folder as the spec

### Format of plan.md

```markdown
# Plan: {Descriptive title}

- **Date:** dd-mm-yyyy
- **Branch:** {branch}
- **Status:** Pending

## Context

{Summary of what will be done — reference to the spec}

## Step 1 — {Step name}

**Files:**
- `src/path/to/File`

**Actions:**
- {Concrete action 1}
- {Concrete action 2}

**Verification:**
- {How to confirm it's correct}

## Step 2 — {Step name}

...

## Final Verification

1. {Global check 1 — e.g. test suite passes with no new failures}
2. {Global check 2 — e.g. linter/formatter reports no errors}
3. {Global check 3 — e.g. build/type-check succeeds}
```

### Checkpoint

After writing the plan, **ask the user**:

> "The plan is ready with N steps. Would you like to execute it?"

Only advance to Phase 3 once the user confirms.

---

## Phase 3 — Execution

### Procedure

1. **Execute step by step** — Follow the plan in the defined order
2. **Verify each step** — Run the verification defined in the plan before moving on
3. **Report progress** — Inform the user when each step is completed
4. **Adapt when needed** — If a step reveals something unexpected, inform the user and adjust the plan before continuing

### Common verifications

Use whatever the project provides — for example its test runner, linter/formatter, type-checker, or build command. Discover the right commands from the project's config and docs (e.g. `package.json` scripts, `Makefile`, CI config) rather than assuming.

### When finishing

Run all the final verifications listed in the plan.

---

## Phase 4 — Save and Document

### Procedure

1. **Update status** — Change the status of `spec.md` and `plan.md` to "Executed"
2. **Add commits** — Reference the commit hashes in `plan.md`
3. **Update README** — Add an entry to the table in `specs/plans/README.md` (if it exists)

### Final folder structure

```text
specs/plans/YYYY-MM-DD-HHmmss_contextual-name/
├── spec.md       ← Specification (requirements, scope, motivation)
└── plan.md       ← Execution plan (steps, verifications, commits)
```

---

## Rules

- **Always save** the spec and plan in `specs/plans/` — even if the user doesn't explicitly ask
- **Never skip the spec** — The temptation to jump straight to the plan is strong, but the spec forces thinking about the "what" before the "how"
- **Never execute without approval** — Always ask before each phase transition
- **Language** — Write all content in English
- **Dates** — Use `dd-mm-yyyy` format in content and `YYYY-MM-DD-HHmmss` in folder names (for filesystem ordering)
- **Follow project conventions** — Respect the patterns documented in the project's AGENTS.md or CLAUDE.md and existing code (architecture, separation of concerns, naming)
