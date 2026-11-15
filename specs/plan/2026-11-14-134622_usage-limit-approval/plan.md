# Plan: User-approved worker usage limit override

- **Date:** 14-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed
- **Specification:** [spec.md](spec.md), approved by the user.

## Step 1 — Model percentage blocking and task consent

**Files:** `agentic/src/worker.rs`, `agentic/src/state.rs`

- Add backward-compatible task consent states: not requested, pending, approved and denied. Revocation returns approved consent to denied.
- Represent percentage-blocked accounts with their windows and usage separately from actual provider-rejected accounts.
- Preserve automatic selection of below-threshold candidates. Require consent only after compatible candidates are exhausted because both providers exceed the percentage guard.
- An approved task bypasses the percentage guard only. Never bypass structured provider failures or reset their bounded retry history.
- Add tests alongside implementation for 85% boundaries, weekly/five-hour windows, mixed availability, consent and old-state deserialization.

**Verification:** Targeted worker and state tests pass; denied/default consent cannot select an over-threshold candidate.

## Step 2 — Persist requests and integrate launch/recovery

**Files:** `agentic/src/team.rs`, `agentic/src/watch.rs`

- Apply current task consent in the common worker-selection path used by spawn, startup fallback and resume.
- Persist a pending request and its percentage evidence transactionally before returning an actionable approval-required result; log the transition once.
- Record approval, denial and revocation through a team operation. Avoid prompting again for a denied request until the user asks to reconsider.
- After approval, use explicit resume to recheck percentage-blocked workers while preserving identity, role, lane and queued orders. Rebuild only the retry state needed for percentage blocking; retain genuine provider failure exclusions within a startup attempt.
- Keep capacity, plan approval, pause and writer ownership checks intact. Revocation governs subsequent launches rather than terminating running workers.
- Test approval followed by resume, dispatcher restart, revocation, task isolation, provider rejection and duplicate-worker prevention.

**Verification:** Targeted team/watch tests pass, including existing fallback/recovery tests.

## Step 3 — Expose consent to the leader

**Files:** `agentic/src/mcp.rs`, `agentic/src/status.rs`

- Add a distinct `set_usage_limit_approval` tool accepting an explicit boolean decision. Its contract requires explicit user consent; approved=false denies or revokes consent. Reject grants without an eligible pending request; allow a denied request to be reconsidered on explicit user instruction.
- Keep this project-local tool separate from exact-call external permission grants.
- Expose consent state, blocked accounts/windows/percentages and the next action in task status.
- Describe approval scope and instruct the leader to call `resume_task` after approval, then inspect status rather than spawning duplicate replacements.
- Test tool validation, request eligibility, approve/deny/revoke flow and status across task reloads.

**Verification:** MCP/status tests prove that ordinary plan approval cannot grant the override and that no worker starts before the separate consent decision.

## Step 4 — Document and verify

**Files:** `agentic/roles/leader.md`, `agentic/docs/usage.md`, `docs/user/devagentics-execution.md`, `docs/architecture/devagentics-execution.md`, this spec and plan.

- Document the prompt, task-wide duration, denial/reconsideration, revocation and remaining provider constraints. Follow applicable documentation skills when updating the docs.
- Preserve unrelated uncommitted team-history changes in shared files.
- Update spec and plan status after successful execution; record verification results. Do not create a commit unless requested.

**Final verification** (from `agentic/`):

1. `cargo fmt --check`
2. `cargo test`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo build`

Review the final diff for scope and preservation of existing work. Any pre-existing failures must be distinguished from regressions before completion.

## Execution results

- Implemented all four steps. Added state compatibility, task consent, preserved unavailable worker identity, explicit decision tooling, status guidance and documentation.
- Regression coverage includes denial/reconsideration/revocation, state reload, task isolation, provider rejection, plan safeguards and MCP input validation.
- `cargo test --offline`: 205 unit tests and 7 integration tests passed outside the sandbox. The sandbox denies process inspection in the existing timeout cleanup test.
- `cargo build --offline`: passed.
- `cargo fmt --check` and `cargo clippy --all-targets --offline -- -D warnings`: passed.
- No commit created; existing team-history work preserved.
- Separate safe-communication specification prepared at the user's request; terminal message delivery is unchanged in this feature.
