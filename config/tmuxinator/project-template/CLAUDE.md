# CLAUDE.md

> Per-project instructions for Claude Code. Copy this file to a project root and
> fill in the `{{PLACEHOLDERS}}`. Keep it short and specific — it is loaded into
> context every session, so every line should earn its place.

## Project overview

- **Name:** {{PROJECT_NAME}}
- **What it does:** {{ONE_OR_TWO_SENTENCES}}
- **Stack:** {{LANGUAGES_FRAMEWORKS}}
- **Entry points:** {{MAIN_FILES_OR_DIRS}}

## How to run / test / build

```bash
{{INSTALL_CMD}}     # install dependencies
{{RUN_CMD}}         # run locally
{{TEST_CMD}}        # run the test suite
{{LINT_CMD}}        # lint / format
{{BUILD_CMD}}       # build / type-check
```

Always run the test and lint commands above before considering a change done.

## Code rules

- **TDD.** Write tests before or alongside implementation. No feature ships
  without a passing test; every bug fix gets a regression test.
- **SOLID.** Give each class, service, component, and module one clear
  responsibility. Depend on stable interfaces or types instead of concrete
  implementation details. Prefer extension over modifying unrelated behavior.
- **Simplicity first.** Clear, direct code beats clever abstractions. Do not
  design for hypothetical future requirements.
- **Match the surrounding code.** Follow existing naming, structure, and idioms
  before introducing new patterns. Consistency beats personal preference.
- **Small, focused units.** Prefer short functions with a single responsibility;
  use early returns instead of deeply nested logic. Extract shared concepts,
  but keep one-off helpers inline when extraction adds indirection without reuse.
- **Full, descriptive names.** Do not use single-letter or abbreviated names
  (`patient`, not `p`; `appointment`, not `apt`). Booleans read as predicates
  (`isReady`, `hasAccess`).
- **Handle errors explicitly.** No silent catches. Fail fast with useful
  messages; validate inputs at boundaries.
- **No dead code.** Don't leave commented-out blocks, unused vars, or TODO
  dumps. Delete what you replace.
- **Comment the "why", not the "what".** The code says what; comments explain
  intent, trade-offs, and non-obvious decisions.
- **Keep changes scoped.** Don't reformat or refactor unrelated code in the same
  change. One concern per change.
- **Tests describe behavior.** Prefer observable outcomes over implementation
  details. Run the relevant test, lint, type-check, and build commands.
- **No secrets in code.** Config and secrets come from env/config files that are
  gitignored.
- **Reuse before creating.** Search for existing services, components, hooks,
  schemas, constants, and utilities before introducing another implementation.

## Architecture rules

- **Backend:** {{BACKEND_ARCHITECTURE_RULES}}
- **Frontend:** {{FRONTEND_ARCHITECTURE_RULES}}
- **Data and tenancy:** {{DATA_ACCESS_AND_TENANCY_RULES}}
- **Reusable component index:** {{REUSABLE_COMPONENTS_DOCUMENT_OR_NONE}}

## Workflow

- For non-trivial work (multiple files or > ~3 steps), use the **spec-plan**
  skill: Spec → Plan → Execute with approval between phases.
- Save plans under `specs/plan/` and keep their status current as implementation
  progresses.
- For commits, use the **commit** skill — it enforces the message standard and
  **never** adds AI/authorship credits to commit messages.
- Verify changes by actually running the relevant command, not by assuming.

## Conventions specific to this project

- {{PROJECT_SPECIFIC_RULE_1}}
- {{PROJECT_SPECIFIC_RULE_2}}

## Do NOT

- {{THINGS_TO_AVOID — e.g. "don't touch generated files under /gen"}}
