# Plan: OpenCode workers in reviewed teams

- **Date:** 17-11-2026
- **Status:** Executed; verification limitations recorded below
- **Approval:** Specification and implementation plan approved in conversation.

## Steps

1. Establish isolated configuration for installed OpenCode 1.19.6, keeping account
   authentication/data and allowing only authenticated worker MCP. Verify resolved
   configuration and inherited configuration exclusion before readiness.
2. Add exhaustive provider dispatch and managed JSON turns with exact session
   continuation; retain claimed failures for explicit recovery.
3. Normalize events, usage, model and database/session metadata. Enable reviewed
   eligibility and capability reporting after verification. Preserve routing.
4. Update user/architecture docs and run regression, live adapter and reviewed
   lifecycle verification, then make check. Reproduce baseline failures separately.

## Defaults

Workers only; no weaker isolation, automatic worker replacement, commit or push.
A missing authenticated provider account must be reported as a live smoke-test
limitation rather than represented as successful paid-provider verification.

## Verification

- Rust: 373 unit tests and 9 integration tests passed; 6 optional runtime tests ignored by default.
- Installed OpenCode 1.19.6 hostile-project isolation probe passed; effective native tools denied, sole worker MCP allowed.
- Local model fixture with actual OpenCode passed: sole worker tool exposure, worker invocation, exact session resume. No registered/authenticated external OpenCode account was available for a paid-provider smoke test.
- Optional real rootless Docker lifecycle passed with immutable local Alpine image: OpenCode-selected floor author and senior reviewer, isolated execution, checkpoint, checks, independent review, merge and milestone readiness.
- Clippy, formatting and diff whitespace checks passed.
- OpenCode plugin and menu shell suites passed. Usage suite skipped because sqlite3 is unavailable.
- `make check` attempted; stops at scripts/check.sh:49 because Ruby is unavailable. Earlier reported main failures could not be reproduced beyond this environment prerequisite.

The verified reviewed adapter currently supports Linux/OpenCode 1.19.6 only. Other versions/platforms fail closed before readiness and retain compatible fallback. No commits or pushes were made.
