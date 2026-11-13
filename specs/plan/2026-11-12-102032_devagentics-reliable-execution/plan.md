# Plan: Reliable orders and isolated checks in devAgentics

- **Date:** 12-11-2026
- **Branch:** `feat/devagentics-reliable-execution`
- **Status:** Executed — 12-11-2026
- **Specification:** [spec.md](spec.md), approved 12-11-2026

## Context

Implement the approved first batch: explicit report identity, audited order amendments, recoverable interrupt/resume, and named isolated check profiles. Preserve legacy task loading and raw command checks. Compact status, review bundles, context bundles, question amendments and incremental events remain follow-up work.

The existing pause is a state flag, resume recovers gone panes, and `devws agent stop` sends two interrupts before potentially killing a pane. A separate non-destructive interruption primitive is required. Existing task transactions provide state locking, but external checks and deliveries need explicit revision and recovery validation around those operations.

## Step 1 — Establish compatible state and operation boundaries

**Status:** Completed

**Files:** `agentic/src/state.rs`, `task.rs`, `orders.rs`, `watch.rs`, `log.rs`; new focused check configuration/evidence module if needed.

**Actions:**

- Create the implementation branch without disturbing existing local changes. Record baseline test/lint results before implementation.
- Add behavior tests for loading old state/config files and serialization of new state. Default legacy order revisions to zero.
- Define order revisions, structured amendment history, revision-aware queued order messages, check evidence identity, and durable per-agent interrupt/recovery states. Distinguish notification delivery from whether the current order revision was received.
- Introduce shared authoritative-order validation for task, agent, status, delivery and revision. New order attempts still receive new IDs; amendments retain the original ID and base.
- Define short operation reservations in task state for report/check and delivery/recovery coordination. Never hold the general task lock throughout a shell check or slow terminal action. Reject conflicting completion operations explicitly and make interrupted reservations recoverable.
- Keep new fields backward compatible using defaults. Keep long evidence/logs as task artifacts with state references.

**Verification:** Old task/config fixtures load unchanged; round trips preserve new fields; stale queued revision cannot mark a newer revision delivered; operation conflicts produce errors without unrelated state changes.

## Step 2 — Bind reports to exact orders

**Status:** Completed

**Files:** `agentic/src/main.rs`, `orders.rs`, `agent_args.rs`, `roles.rs` where instructions exist, `tests/flow.rs` and inline tests.

**Actions:**

- Write regression tests for the old-report/new-assignment failure before changing reporting behavior.
- Require `--order <id>` for done and blocked. Resolve the exact order and validate ownership, open/current status and delivery before inspecting work or running a check.
- Capture order revision, original base and HEAD; revalidate identity and inputs in the transaction that records completion. Preserve valid newer assignments when an obsolete report is rejected.
- Update generated order messages and all agent instruction sources, CLI fixtures and integration tests to use the exact order ID.
- Return concise errors identifying the submitted and authoritative order when appropriate. Do not close or certify work after concurrent reassignment/amendment.

**Verification:** Done and blocked reject stale, foreign, closed and undelivered orders; missing ID fails at CLI parsing; concurrent state/HEAD changes reject completion; normal reports retain existing review behavior.

## Step 3 — Amend orders without losing identity

**Status:** Completed

**Files:** `agentic/src/orders.rs`, `main.rs`, `mcp.rs`, `state.rs`, `watch.rs`, `lanes.rs` and regression tests.

**Actions:**

- Add shared amendment operation, CLI `order amend <id> --allow <glob> --reason <text>` with repeatable allow entries and explicit/inferred task selection following existing task-context conventions, plus leader MCP tool `amend_order`.
- Validate all additions before mutation; deduplicate additions; preserve unrestricted semantics, forbidden rules and original base/agent/attempt count.
- For lanes, retain lane constraints as an independent policy ceiling during inspection. Reject exact outside paths; for globs require a conservatively provable lane-contained pattern, returning a clear error for unprovable widening rather than attempting general glob inclusion.
- Increment revision only for effective changes; append reason and authorization source to amendment history, invalidate prior evidence, and replace stale queued order copies with the updated authoritative message.
- Serialize amendment versus delivery so a message for an older revision cannot certify delivery of a newer one.

**Verification:** Authorized test-file additions pass policy checks while retaining base/ID; duplicate/unrestricted additions are no-ops; malformed globs and forbidden/lane escapes fail safely; amendment during a check invalidates completion; queued and already-delivered amendment paths behave correctly.

## Step 4 — Add non-destructive interruption and durable recovery

**Status:** Completed

**Files:** `config/tmuxinator/agent_cli.sh`, `agentic/src/devws.rs`, `team.rs`, `watch.rs`, `main.rs`, `mcp.rs`, `status.rs`, shell-test location discovered from existing test conventions.

**Actions:**

- Add `devws agent interrupt <pane>` separately from stop. Validate supported program and active pane state, send a single interruption request, and wait with a bounded timeout for observed completion; never kill a pane or send a second quit signal. Unsupported or unacknowledged interruption returns an explicit outcome.
- Extend Devws real and fake adapters with structured interruption results. Test tmux interaction through a stub rather than interrupting live sessions.
- Add `task pause <task> --interrupt` and leader `pause_task`/`resume_task` tools, preserving existing soft-pause behavior.
- Persist pause and per-agent interruption intent before external effects. Include the managed leader when safe, exempt the invoking control pane, record partial outcomes, and retain paused state on failures.
- Coordinate delivery reservations with pause so no delivery starts after pause is committed; an already-issued terminal action must be observed and included in recovery before pause reports success. Suppress intentional-interruption stall warnings.
- Resume surviving interrupted panes with the current authoritative order; retain existing lost-pane recovery. Queue recovery before unpausing, avoid obsolete order copies, and preserve pending recovery when a pane stays busy or an external action fails.
- Make repeated calls and restart recovery idempotent; expose concise pending/failed recovery information in existing status output. Agents with no current order receive no fabricated work.

**Verification:** Fake adapter tests cover pause/delivery races, leader/self handling, partial interruption, persistent restart, busy/gone panes, amendment while paused, no-order agents and repeated resume. Stub shell tests confirm one interrupt and no pane destruction.

## Step 5 — Configure and execute isolated named checks

**Status:** Completed

**Files:** `agentic/src/task.rs`, `checks.rs`, new `check_profiles.rs`/`check_runs.rs` modules as responsibility requires, `main.rs`, `mcp.rs`, `team.rs`, `orders.rs`, `Cargo.toml`/`Cargo.lock` only for process management if necessary.

**Actions:**

- Define repository-scoped named checks and environments in task TOML: command, selected environment, timeout, declared preflight requirements and optional machine-readable result adapter. Store no actual secret values; resolve explicitly named secret environment sources only at launch.
- Expose validated profile configuration through leader tools and profile selection on orders, mutually exclusive with a legacy raw command override. Keep old string checks on the legacy path.
- For named phases, clear the environment and add a documented minimal execution baseline (PATH and required temporary-directory settings), explicit non-secret values and explicitly referenced secret inputs. Do not implicitly inherit HOME or database settings; profiles must opt into additional tool settings.
- Use fresh per-phase working/output directories where needed. Run declared prerequisites before tests and distinguish unmet requirements from test assertions.
- Define a generic JSON result contract for test counts and required skipped tests, plus fixture producer examples. Treat absent/malformed required output as runner failure; do not introduce speculative Pest log parsing. Add a dedicated adapter only if the target runner's supported output is verified during implementation.
- Launch each shell in its own process group; timeout/cancellation terminates and reaps descendants. Use the smallest appropriate Unix process API dependency if standard APIs are insufficient for safe signaling.
- Return typed phase results with bounded diagnostics and unique full-log artifacts. Redact configured secret values from returned diagnostics and retained logs without changing child execution.

**Verification:** Fixture commands prove parent and prior-phase database variables cannot leak; explicit tool path/env works; unknown/ambiguous config fails early; missing prerequisites, nonzero tests, required skips, malformed evidence, timeout and spawn failure classify correctly; descendant process exits after timeout; secrets are absent from diagnostics/logs.

## Step 6 — Integrate check evidence with reporting and leader tools

**Status:** Completed

**Files:** `agentic/src/main.rs`, `orders.rs`, `checks.rs` and new runner modules, `mcp.rs`, `status.rs`, `tests/flow.rs`.

**Actions:**

- Add `check run --order <id>` with task selection matching order amendment and MCP `run_check`; enforce caller/task/order ownership as appropriate to agent CLI versus leader tools.
- Run file policy before commands. Emit structured policy rejection without launching tests. Persist unique evidence tied to order ID/revision/base/HEAD and selected configuration fingerprint.
- Use one runner from explicit check execution and done reports. Reuse evidence only when HEAD, clean worktree, order revision, profile configuration, and declared environment input fingerprints still match; never persist raw secret inputs. Otherwise rerun checks.
- Finalize reports only after transaction revalidation. Keep the order open when concurrency makes evidence stale; ordinary valid test/policy failure retains existing failed-report semantics.
- Return compact structured results from check APIs and short report messages with evidence/log paths; retain complete output only in artifacts. Keep legacy check outcomes compatible.

**Verification:** Binary/MCP tests cover a successful explicit check reused by report, all invalidation inputs, failure classifications, foreign/stale order rejection, policy-before-command behavior, unique logs on retries and bounded output. Confirm explicit checks never close an order themselves.

## Step 7 — Document migration and run final validation

**Status:** Completed

**Files:** `agentic/docs/usage.md`, `agentic/docs/design.md`, `agentic/README.md` where needed, repository architecture/user docs according to applicable documentation skills, this spec and plan.

**Actions:**

- Apply the user documentation and architecture documentation skills for the new visible commands and state/runner design. Match repository conventions and update existing canonical docs before adding parallel documentation.
- Document required report IDs, amendment authorization, pause/interrupt/resume outcomes, profile/environment examples, machine-readable test contract, legacy migration and secret handling.
- Add an end-to-end stub flow: delivered order → amendment → interrupt/pause → resume → isolated check → exact-order report → leader review. No real team interruption, external databases or project-specific PHP suite is needed.
- Run final checks, inspect the complete diff for scope and update plan/spec statuses only after successful verification. Record actual commands/results and any limitations; do not create a commit unless requested.

**Verification:** Run commands below; verify CLI help and MCP schemas match docs and stub flow. No claim of measured token reduction in this batch.

## Final Verification

From `agentic/`:

1. `cargo test`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo fmt --check`
4. `cargo build --locked`

For changed shell code: run `bash -n config/tmuxinator/agent_cli.sh` from the repository root and the existing relevant shell regression/lint commands discovered before editing. Use stub tmux/devws commands for behavior verification. Compare any baseline failures explicitly rather than reporting them as passing.

## Execution Checkpoint

Specification and execution approved. All seven steps completed and verified.


## Validation Results — 12-11-2026

- `cargo test --quiet`: 157 unit tests and 6 integration tests passed.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo fmt --check`: passed.
- `cargo build --locked`: passed.
- `bash scripts/test_agent_interrupt.sh`: passed with stub terminals.
- `bash scripts/test_agent_cli.sh`: passed using its isolated tmux server.
- `bash -n` for both changed shell scripts: passed.
- New documentation links resolve; `git diff --check` passed.
- Process inspection in the timeout test and the existing isolated tmux suite required sandbox escalation. Neither validation touched live agent panes.

## Implementation Decisions and Limits

- Execution metadata lives in defaulted State maps keyed by order/agent, keeping old Order records compatible.
- Named profile selections bind to an order at issuance. Current definitions participate in evidence validation. Source-secret profiles rerun instead of persisting a secret-value fingerprint; legacy commands also rerun.
- Required skip classification uses an explicit JSON count contract. No project-specific Pest adapter or real database execution was introduced.
- Delivery acknowledgement cannot be atomic with a terminal action. Persisted intent supports recovery, and repeated acknowledged recovery is coalesced; operators inspect uncertain crash-window actions before retrying.
- Documentation added under root `docs/user/` and `docs/architecture/`, with links from existing agentic docs. Existing documentation was not relocated.
- No installed binary or live task was upgraded; the source was built and tested. Installation remains the existing project installation workflow.
- No commit was requested or created. Pre-existing `.env` changes and the unrelated deployment specification were left untouched.
