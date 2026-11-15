# Spec: Reliable orders and isolated checks in devAgentics

- **Date:** 12-11-2026
- **Branch:** `feat/devagentics-reliable-execution`
- **Status:** Executed — 12-11-2026

## Context

The leader's session report describes stale completion attribution, conversational file approvals that do not reach the checker, interrupted agents that never resume, and database environment settings leaking between checks. These failures consume agent turns and make review unreliable. Correct order identity and recoverable execution come before context compression.

Code inspection confirms that `orders::report` selects the pane agent's open order without an explicit ID, `Team::pause` only sets a flag, `Team::resume` restarts lost or gone panes only, and `checks::run_check` inherits the process environment. Orders already retain a base commit and delivery flag; changes should extend these concepts.

## Requirements — first implementation batch

### 1. Order-bound reports

- Require `report done --order <id> <summary>` and the same order binding for blocked reports. Never infer the order from the latest assignment.
- Validate task membership, reporting agent ownership, open status, delivery, and that the order is still authoritative. Reject stale, closed, superseded, undelivered, and foreign orders without state changes.
- Validate again in the final transaction after checks, so reassignment or amendment during a check cannot close the wrong assignment or certify an obsolete revision.
- Bind check evidence to order ID, revision, base and checked HEAD. Reject results if those inputs change during execution.
- Update generated order messages, agent instructions, CLI help, fixtures and integration tests together. Existing stored tasks must still deserialize.

### 2. Explicit order amendments

- Add `order amend <id> --allow <path-or-glob>` in the task context and a corresponding leader MCP tool. Support repeated `--allow` and require an amendment reason.
- Amend an open authoritative order in place: preserve ID, original base, step, agent and attempt count. Record a monotonic revision and durable amendment history.
- Validate globs before mutation. Preserve forbidden rules and lane boundaries; amendments must not silently widen lane ownership. Repeated identical allow additions must not duplicate state.
- Queue the updated authoritative order for the agent, replacing stale queued copies. Invalidate earlier check evidence; never require reissuing an order to widen its allowed files.
- Existing unrestricted allowlists remain unrestricted; adding an allow entry must not accidentally narrow them. Return an explicit no-op result in that case.

### 3. Recoverable pause and resume

- Preserve the existing soft pause and add `task pause <task> --interrupt` with equivalent MCP support.
- Persist pause before attempting interrupts; prevent new deliveries and launches throughout the pause. Interrupt active team turns without deleting panes, worktrees or edits.
- Track which agents require resumption and per-agent interrupt outcomes. Include the leader when managed and active; do not interrupt the invoking control process. Surface partial failures and keep the task paused.
- `task resume <task>` must resume interrupted agents in existing panes with their authoritative current orders, while preserving the existing recovery for lost panes. Agents without current orders receive no invented assignment.
- Queue recovery messages before enabling delivery. Repeated pause/resume calls must not duplicate orders or start parallel turns. Retain failed recovery work for retry and expose a concise outcome.
- Use a dedicated Devws interrupt capability rather than treating pane destruction as interruption. Verify supported agent programs and report unsupported interruption explicitly.

### 4. Named, isolated check profiles

- Add repository-scoped named check and environment profiles in task configuration, plus leader configuration support. A check profile selects a command, environment, timeout, and declared requirements. Do not commit secrets in profile definitions or output.
- Add `check run --order <id>` to execute the order's configured profiles in its authoritative worktree, with equivalent MCP support. Report completion must reuse valid evidence or execute the same runner.
- Launch every phase with a fresh explicitly constructed environment. Preserve only documented baseline variables needed to locate tools and use temporary storage, then apply its selected environment. Database variables from the parent process or another phase must not leak.
- Keep legacy string checks working with their current semantics during migration; named profiles use isolation. Reject ambiguous mixed configuration and unknown profiles before execution.
- Return structured phase outcomes: passed, test failure, required tests skipped, environment failure, file-policy failure, timeout, or runner failure. Use explicit preflight and machine-readable test evidence or configured adapters; do not guess failure classes from arbitrary log text. Missing required evidence fails closed.
- Return order/revision, profile, outcome, exit code, duration, short diagnostic and full-log path. Bound inline output, retain full logs under unique run IDs, and do not put secret environment values in diagnostics.
- Kill and reap the check process group on timeout, including shell descendants. Persist evidence independently of the concise response.

## Follow-up scope

Keep these as a subsequent batch, after the correctness foundation is verified:

1. `task status <task> --brief`: current orders, blockers, pending reviews, pause/recovery state and limits; omit historical plans, completed work and gone agents by default.
2. `review bundle <step>`: original implementation base through current HEAD, full diff artifact, acceptance criteria, amendments and valid check evidence, including across reissued orders.
3. `agent context <agent>`: authoritative order, relevant plan sections, decisions, unanswered questions and unfinished edits with paths to longer artifacts.
4. Order-bound questions using existing senior routing; scope-changing answers explicitly create amendments rather than relying on prose.
5. `team events --since <cursor>`: stable incremental cursors and explicit acknowledgement so handled notifications are not repeatedly delivered.

## Constraints

- Extend existing Rust CLI, task transactions, JSON state, dispatcher and MCP APIs; do not introduce a second coordinator or state store.
- Keep terminal and board behavior compatible; no model or role changes in this batch.
- Preserve existing work and unrelated local changes. No live team interruption or project-specific PHP/database execution during development validation.
- All new state fields must support old task files. Fail with actionable errors when old commands omit newly required order identity.
- Maintain bounded agent-facing results with full evidence available as artifacts. Do not claim token savings without measurement.
- No implementation until specification and subsequent implementation plan approvals, following project workflow.

## Files Involved

| File | Expected change |
| ---- | --------------- |
| `agentic/src/main.rs` | Order/check CLI commands and required report identity |
| `agentic/src/orders.rs` | Identity validation, amendments, revision-aware reporting |
| `agentic/src/state.rs` | Backward-compatible revisions, amendments, recovery and evidence references |
| `agentic/src/task.rs` | Named check/environment configuration |
| `agentic/src/checks.rs` | Isolated runner, structured outcomes and process cleanup |
| `agentic/src/team.rs` | Recoverable interruption and resume |
| `agentic/src/devws.rs` | Interrupt abstraction, real adapter and fake |
| `agentic/src/watch.rs` | Pause-safe delivery and recovery handling |
| `agentic/src/mcp.rs` | Leader tools and schemas |
| `agentic/src/agent_args.rs`, `agentic/src/roles.rs` | Agent instruction updates where applicable |
| `agentic/src/status.rs` | Recovery/check state visibility |
| `agentic/tests/flow.rs` | CLI/MCP regression coverage |
| `agentic/docs/usage.md`, `agentic/docs/design.md` | Command migration and state/runner design |

Exact supporting files and any process-management dependency will be selected during planning after confirming existing devws interrupt primitives.

## Success Criteria

- [x] A report for an older order cannot complete a newer one; unauthorized and undelivered reports leave state unchanged.
- [x] An amendment permits an authorized test file without changing order identity or review base; forbidden and lane rules remain enforced.
- [x] A concurrent amendment/reassignment/HEAD change invalidates in-flight evidence.
- [x] Pause blocks delivery before interruptions; resume wakes interrupted existing panes exactly once and survives partial failures/restarts.
- [x] SQLite and PostgreSQL fixture checks cannot observe each other's variables or inherited database settings.
- [x] Required skipped tests, unmet environment requirements, policy rejection, test failure and timeout produce distinct structured outcomes.
- [x] Timeout leaves no child check processes; logs remain available and inline diagnostics are bounded.
- [x] Old stored tasks and legacy check strings remain usable; updated instructions always provide an explicit report order ID.
- [x] Regression tests precede or accompany implementation; `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and `cargo build --locked` pass in `agentic/`.

Implementation and validation results are recorded in [plan.md](plan.md). Terminal delivery is not transactional with disk: recovery coalesces ordinary retries, while a crash after terminal acceptance but before acknowledgement requires pane inspection, as documented in the execution guide.
