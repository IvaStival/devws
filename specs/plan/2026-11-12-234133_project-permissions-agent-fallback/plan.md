# Plan: Project-scoped autonomy and agent model fallback

- **Date:** 12-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed — one existing verification blocked by sandbox

## Context

Implement the approved requirements in `spec.md`. Preserve unrelated work in the current tree. Current worker startup refuses exhausted accounts before launch, role parsing forbids worker fallback, and pane creation does not prove model readiness.

## Step 1 — Align permissions with project boundaries

**Progress:** Completed.

**Files:** `agentic/src/mcp.rs`, `agentic/roles/leader.md`.

- Write regression tests proving task-owned agent stop/start/replacement needs no per-call approval, while adding another repository and opening remote pull requests still require exact one-use authorization.
- Classify stopping a task-owned agent as system; retain ownership validation before affecting a pane. Remove destructive classification as a reason to request approval for this project-local operation.
- Update tool descriptions and leader instructions to explain the project boundary. Keep the existing overall-plan approval workflow.
- Test invalid and foreign agent identifiers, ensuring they cannot affect an unrelated pane.

**Verification:** MCP and team permission tests; exact-call, denied-call and consumed-authorization regressions for the remaining external actions.

## Step 2 — Define compatible ordered worker candidates

**Progress:** Completed.

**Files:** `agentic/src/roles.rs`, `agentic/src/agent_args.rs`, `agentic/roles/samples/` and a focused worker-selection module if needed.

- Write parsing and launch-argument tests first.
- Allow existing `[fallback]` configuration for worker roles without changing its meaning for the leader; support an ordered list of further worker alternatives with explicit provider/model settings.
- For existing Claude workers without configured alternatives, supply a Codex candidate using the project's existing Codex model convention. Supply the reverse compatible default for Codex workers. Explicit alternatives take precedence over defaults.
- Deduplicate candidates, retain role instructions/group/write scope, and apply each candidate's own provider-compatible model/effort/approval settings. Never copy a Claude model identifier into a Codex invocation.
- Keep permission ceilings independent of model selection. Show available candidates in role status.

**Verification:** worker and legacy leader role parsing; explicit/default ordering; duplicate removal; read-only and writer argument checks on both providers.

## Step 3 — Select available candidates before reserving a worker

**Progress:** Completed.

**Files:** `agentic/src/team.rs`, `agentic/src/devws.rs`, worker-selection module, `agentic/src/state.rs`, `agentic/src/status.rs`.

- Write behavior tests for Claude exhaustion followed by Codex success, same-provider alternative models, both providers exhausted and unknown usage.
- Evaluate preferred candidate then alternatives, checking provider availability and the existing 85% week/five-hour thresholds. Account exhaustion skips other models sharing that account; model-specific failure permits another model on that provider.
- Preserve paused-task, plan approval, capacity and one-writer checks. Reserve one logical worker and prevent concurrent launches from bypassing these checks.
- Persist selected provider/model/effort and fallback reasons with backward-compatible defaults for existing task state. Expose actual selection in status and logs.
- Return all candidate rejection reasons when none can run; leave no live reservation.

**Verification:** team, state compatibility and status tests, including lane and capacity regressions.

## Step 4 — Handle unavailability discovered during startup

**Progress:** Completed.

**Files:** `agentic/src/devws.rs`, `agentic/src/team.rs`, `agentic/src/watch.rs`, `agentic/src/recovery.rs`, `config/tmuxinator/agent_cli.sh`, provider event integration as needed.

- Establish typed startup evidence for provider exhaustion, model unavailability and ordinary launch failures. Use structured provider events or explicit supported error codes/messages; do not classify arbitrary pane text as a retry instruction.
- Handle synchronous launch rejection and asynchronous startup rejection before the worker becomes ready. Pane creation alone must not count as model availability.
- On explicit unavailability, try the next unattempted candidate at most once, retaining the logical worker identity and lane. Clean up only the failed task-owned process/pane before launching another candidate.
- Preserve queued orders and authoritative recovery context; do not replay an already completed order or automatically interrupt a healthy working worker.
- Propagate unrelated configuration, filesystem and task errors immediately. On exhaustion of all candidates, mark the worker unavailable and notify the leader with reasons.

**Verification:** fake provider events and dispatcher ticks covering quota rejection, model rejection, duplicate events, successful readiness, unrelated errors, queued work and exhaustion cleanup. Run relevant shell tests if the bridge changes.

## Step 5 — Integrate, document and verify

**Progress:** Completed.

**Files:** `agentic/tests/flow.rs`, `agentic/docs/usage.md`, `docs/guide/agentic.md`, `docs/architecture/devagentics-execution.md`, affected user docs.

- Add an integration scenario where exhausted Claude results in a Codex worker taking the same role/lane without a permission request.
- Cover external permission boundaries and fallback failure without phantom workers.
- Update architecture and user documentation using the applicable documentation skills; document candidate configuration, defaults, reasons in status and all-candidates-unavailable behavior.
- Update spec/plan completion status only after implementation and checks pass. No commit or changes to live sessions are included in this plan.

**Verification:** integration tests and the final checks below.

## Final verification

1. `cargo test --manifest-path agentic/Cargo.toml`
2. `cargo fmt --manifest-path agentic/Cargo.toml --check`
3. `cargo clippy --manifest-path agentic/Cargo.toml --all-targets -- -D warnings`
4. `cargo build --manifest-path agentic/Cargo.toml`
5. Relevant shell bridge tests discovered from changed scripts; run the existing agent CLI suite when its interface changes.
6. Review the final diff for unrelated changes, leaked provider-specific settings, lost orders and weakened project boundaries.

## Results

- Project-local start, stop and replacement are system actions; foreign IDs
  cannot control a pane. Cross-project additions and remote PR publication
  retain exact one-use authorization.
- Worker roles support `[fallback]` and ordered `[[alternatives]]`; legacy
  roles receive compatible cross-provider defaults. No role-file edits are
  needed to activate those defaults.
- Startup selection persists attempts and preserves scope, identity, lanes
  and queued order IDs. Model rejection, account rejection, unknown errors,
  cleanup failure, explicit resume and stale former-pane evidence are covered.
- Explicit resume picks up corrected provider settings while keeping the
  worker's snapshotted instructions and write scope.
- Architecture and user docs are updated.

Validation:

```bash
cargo test --manifest-path agentic/Cargo.toml --quiet -- --skip check_profiles::tests::timeout_terminates_the_shell_and_its_background_child
cargo fmt --manifest-path agentic/Cargo.toml --check
cargo clippy --manifest-path agentic/Cargo.toml --all-targets -- -D warnings
cargo build --manifest-path agentic/Cargo.toml
python3 scripts/test_agent_model.py
bash scripts/test_agent_cli.sh
bash -n config/tmuxinator/agent_runner.sh config/tmuxinator/agent_state.sh
git diff --check
```

Results: 185 unit tests + 7 integration tests passed; 10 Python hook/runner
tests and the agent CLI suite passed. Formatting, Clippy, build, shell syntax,
documentation links and diff checks passed. The full Rust suite was attempted;
one existing process-inspection test cannot execute `ps` in this sandbox.
Escalation requests were interrupted by daemon restarts. This is the only
remaining verification exception; it was not removed or weakened.

The updated binary is built under `agentic/target/debug/`. Installation and
restarting live teams are outside this approved implementation plan. No commit
was requested or created.
