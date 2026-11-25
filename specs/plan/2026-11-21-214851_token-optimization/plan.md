# DevAgentics token optimization implementation plan

- **Date** 21-11-2026
- **Source** `feat/opencode-workers` at `1b9e06f`, including pre-existing working-tree changes
- **Status** Compact MCP and the opt-in Codex Leader CLI pilot validated through completed Notes deliveries. Broader CLI coverage and general cost claims remain open.
- **Scope** [Specification](spec.md). Reviewed teams are the initial trial population. Preserve legacy compatibility.

## Repository audit

Paths in the audit and phases are relative to the DevWS root.

| Area | Verified behavior and implications |
| --- | --- |
| Leader | `agentic/src/start.rs::write_leader_args`, `leader.rs::reviewed_leader_allowed`, and `worker_tools.rs::leader_arguments` configure the tmux Leader and provider restrictions. Reviewed Leader support is provider- and policy-dependent. Codex has a reviewed-team opt-in. OpenCode worker support alone says nothing about Lead eligibility. `mcp.rs::Server::serve/handle` serves newline-delimited JSON-RPC and can relay individual requests to a replaced binary, signaling tool-list changes. |
| Authorization | `mcp.rs::Server::call` runs required-field checks, classifies effects, and gates exact calls through `authorized_call` and `authorize_action`. Dynamic checks include project-external workflow effects. `call_inner` assumes these checks have passed. `main.rs::run` already exposes operator commands including workflow, task status, order amend, check run and report. The generic operator workflow calls `workflow::call` directly under different trust assumptions. |
| Dispatch | `main.rs` uses a one-second dispatcher cadence. Reviewed dispatch in `watch.rs` sends a fixed Leader nudge through `devws.rs::CommandDevws::send` with text on stdin. It tracks `leader_nudged`, separately from consumption. Legacy `tell_the_leader` injects message text and consumes it after successful delivery. Managed workers claim messages into `worker_turns` in the supervisor. |
| State | `home.rs::Home` resolves `DEVAGENTICS_HOME` or `~/.devagentics`. `task.rs::Task::transaction` takes exclusive `state.lock`, replaces `state.json` by rename, then appends `log.jsonl` under the same lock. The append can fail after state replacement. The writer has no explicit file/directory fsync. Reads take a shared lock. `control_lock` serializes selected effects. `state.rs` allocates sparse monotonic IDs across entities, including messages. Application and order revisions have separate domain meanings. |
| Inbox | `workflow.rs::leader_inbox` returns pending events, roster and ready work. Existing `input.ack` combines acknowledgment with the next inbox call, using separate write and read snapshots. `acknowledge` removes IDs and can reject a repeated acknowledgment. Pending messages are authoritative. The audit log is unsuitable as a lossless replay stream. |
| Worker | `worker_tools.rs::panel` launches managed subprocess turns and retains raw stream output with rotation. `worker_tools.rs::mcp` authenticates at startup and on each call. `call_in` adds memory and report/review side effects around `call`, which enforces live agent, role and probe restrictions and dispatches the capability catalog. |
| Execution and integration | `runner.rs::execute/run_container` uses scoped Docker snapshots. `runner.rs::checkpoint` validates the current bound order and allowed paths before transferring changes through Git. `check_profiles.rs::run` records evidence and uses an order operation lock. `orders.rs::report/report_findings`, `reviews.rs` and `integration.rs` enforce role-specific reporting, verification, review and candidate checks. Keep these service paths and their evidence semantics. |
| Memory | `agentic/src/memory.rs` wraps the external ai-memory CLI. Its backend storage is outside this crate. `notes.rs::about_step/knowledge_for` already targets step and lane/path. `notes.rs::search` asks for project-wide candidates, filters by task path afterward and caps results at five. `mcp.rs::call_inner` injects step and area notes into orders. `start.rs::with_memory` and `leader.rs::handed_memory` include handoff context. |

### Measurements and evidence gaps

A fresh offline build from the audited working tree served real `tools/list` and `tools/call` requests against a temporary `DEVAGENTICS_HOME`. The fixture had one live worker, one short pending Leader message and no real provider, Docker workload, or application history. Sizes below are UTF-8 bytes. Catalogs use compact JSON for the tool array. Input schemas sum compact serialization of each `inputSchema`. Tool content sizes refer to the decoded text. Wire sizes exclude the trailing newline.

| Surface | Count | Catalog bytes | Input schema bytes |
| --- | ---: | ---: | ---: |
| Leader | 36 | 21,901 | 10,854 |
| Worker | 1 | 1,640 | 107 |

The largest Leader entries by compact bytes were `send_order` 3,049, `application_workflow` 1,709 and `spawn_agent` 1,188. The single Worker entry is 1,638 bytes, much of it the description of multiple actions. The Leader catalog is 13.4 times the size of the Worker catalog on this measure.

| Fixture call | Content bytes | Wire bytes |
| --- | ---: | ---: |
| Leader workflow inbox | 219 | 332 |
| Leader workflow status outline | 1,004 | 1,185 |
| Worker status | 205 | 326 |

The fixture lacked a real pane-state responder, so `task_status` produced an error and was excluded. Reproduce by building with `cargo build --offline --manifest-path agentic/Cargo.toml`, launching `mcp fixture` and `worker-mcp fixture worker-1 --identity <fixture path>`, and sending JSON-RPC `tools/list` and the corresponding `tools/call` requests. Use a fake `DEVAGENTICS_DEVWS` for repeatable full status measurements.

Other confirmed size drivers are 2,512 bytes of source in `agentic/roles/standards/shared.md` and 22,159 bytes in `agentic/roles/leader.md`. Managed ordinary prompts rebuild role text, shared standards, provider tool rules, optional graph instructions and the message even on resumed turns. Exact prompt sizes vary. `runner.rs` retains head and tail up to about 64 KiB per output stream. `mcp.rs::fitting` rejects Leader successes above 80,000 characters. Worker MCP pretty-prints JSON as text. `check_logs.rs::read` already offers bounded access to persisted check logs. Broad state reads, candidate diffs, build output, memory injections and retries are candidates for workload measurement. Their production frequency and ranking are unknown.

The earlier [Leader token-savings plan](../2026-11-19-090000_leader-token-savings/plan.md) recorded roughly 210 calls in one session, including about 34 replacing workers after account errors, 25 reading or acknowledging inboxes and six guessing order IDs. It already introduced combined inbox acknowledgment and account-failure recovery. Treat that sample as historical evidence and remeasure against the current tree.

Local byte counts establish catalog asymmetry and repeated context sources. They cannot establish how often each provider includes schemas in upstream requests, provider token attribution by role, typical result distributions, context growth, or live cost and reliability differences. Phase 1 establishes those baselines. An MCP `tools/list` trace measures client discovery only. Use authorized provider metadata when available and controlled A/B deltas otherwise. Per-component token counts derived locally remain estimates.

### Provider sessions and accounting

| Runtime | Verified managed worker lifecycle | Phase 1 validation |
| --- | --- | --- |
| Claude Code | `runner.rs::prepare/authenticate/bind_order` creates a generation-specific `workers/<agent>/identity-<nonce-prefix>`. `worker_tools.rs::panel/claude_arguments` derives the UUID from its nonce, starts with `--session-id`, then uses `--resume` after a successful turn. `resumed` is panel-local. Rebinding an order can preserve identity. | Restart with a preserved identity and existing transcript. Verify first-turn interruption, repeated orders, fallback and account isolation. |
| Codex | `codex_turn/thread_id` collects `thread.started` and stores it under worker ID in `state.worker_sessions` after output drain and process wait. Later calls use `codex exec resume`. | Kill after session start and before persistence, then verify orphan/lost-thread behavior and recovery. Verify worker replacement and account changes. |
| OpenCode | `opencode_turn` learns session ID from events, persists under `opencode/<worker>` during streaming, rejects unexpected IDs and passes `--session` later. `opencode_worker.rs` supplies the controlled adapter configuration and transcript pointer. | Check pinned runtime, interrupted first output, fallback across providers and separate account data directories. |

`start.rs::start_task_with_workflow` derives task ID from branch slug and handles collisions by comparing canonical worktrees and allocating a suffix. Session maps inherit task-directory scope. Verify same branch across repositories, concurrent starts, task reopen, agent recreation, role/account changes and crash replay. Preserve session keys, JSON records and provider arguments during transport trials. Provider history continues across orders by design. Measure whether that continuity becomes expensive before considering rotation.

`panel_view.rs` parses worker stream usage for display. Codex completed-turn usage has inclusive input and a cache-read subset. Claude display adds regular input and cache reads and omits cache creation. OpenCode accumulates deduplicated step usage. `worker_tools.rs::show_turn` displays summaries and keeps raw output, without a normalized turn ledger. `board_usage.rs::UsageReader` reads `devws agent usage` and archived usage. Provider hooks under `config/tmuxinator/agents/` expose richer transcript data. `bench.rs::report/build` already reports delivery metrics and estimated API cost, with limited per-turn attribution and coverage. Reuse this plumbing and verify definitions against fixtures for pinned provider versions.

## Architecture decision

Keep transport-neutral application operations inside the existing coordinator. Extract small typed operations where MCP currently combines validation, enrichment, authorization and rendering. Both adapters invoke the same guard and service. The supervisor, dispatcher and state bus retain ownership of sessions, wake-ups and delivery. Use a trusted local launcher or broker only if required to present a constrained callable capability. Keep task state and privileged paths inaccessible to the model-facing execution environment.

| Variant | Configuration | Reason to test | Primary trade-off |
| --- | --- | --- | --- |
| A | Current MCP for Leader and workers | Baseline and rollback. Also test compact MCP with the same data contract as B. | Larger Leader catalog. Existing capability enforcement and diagnostics paths. |
| B | CLI for high-frequency Leader operations; worker MCP retained | Largest measured catalog opportunity with the worker trust boundary intact. Use scoped MCP for uncovered Leader capabilities during pilot, and measure its residual catalog. | CLI discovery, shell grammar, subprocess overhead and capability mediation. Full Leader CLI only after parity and verified isolation. |
| C | CLI for Leader and workers | Tests incremental gain over B, including command discovery and round trips. | Worker already has one compact schema. Separate provider-specific confinement work and larger parity surface. |

Start implementation with telemetry for A. Run compaction and transport experiments independently so smaller results cannot be credited to CLI. Prefer B for a pilot when confinement is proven and A/B evidence warrants it. Keep MCP for any provider or role that cannot safely invoke a narrow CLI capability. A smaller MCP catalog or compact MCP alone remains a viable outcome.

## Proposed command contracts

These are model-facing commands behind the constrained launcher. Bind task, role, agent/account and worker generation to the launch context. Resolve requested order IDs against that principal. Operator CLI remains separate. Use compact JSON by default in agent mode, with `--json` explicit and `--detail` or bounded selectors for drill-down. JSON and command payloads go over stdin to avoid shell interpolation and accidental argv disclosure.

| Command | Input and default result | Application operation |
| --- | --- | --- |
| `order issue --input - --request-id KEY` | JSON required `agent,step,title,text`. Optional `allowed,read,targets,forbidden,profiles,check,plan,spec,diagnosis`. Diagnosis is `cause,evidence,change`; preserve current enum, profile/check exclusivity, plan and retry rules. Returns `order_id,step,status`. | Typed extraction of `mcp.rs` send-order path, including memory enrichment and `orders::send_order`. |
| `order get ID [--detail]` | Returns order/step IDs, assignee, status and relevant contract/evidence references. Details expose only authorized fields. | Task-scoped projection of `state.orders` and bound contract. |
| `inbox list [--limit N]` | Pending page, opaque cursor, `has_more`, optional roster and ready-work summary. | `workflow.rs` inbox logic over one snapshot. |
| `inbox changes --since CURSOR [--limit N] [--ack ID]...` | Returns new events in order, next cursor, pagination and acknowledgment results. Processing and acknowledgment are explicit. | Shared inbox operation described below. |
| `task status [--section NAME]`, `state summary`, `task context [STEP]` | Compact counts, blockers, ready work and scoped contract/plan/evidence references. Section/detail supports targeted expansion. Worker defaults to its assignment and permitted review scope. | `status::summary`, `workflow::compact/brief`, contract and plan readers. |
| `worker status` | Identity, role, active order, revisions, blockers and authorized references. | `worker_tools::call_in/call`. |
| `worker execute --input -` | JSON `command` for the container only. Returns execution status, exit code, diagnostics and `result_id`. | `runner::execute/run_container`. |
| `worker checkpoint --input -` | JSON `summary`. Returns changed paths and checkpoint commit or evidence reference. | `runner::checkpoint`. |
| `worker check` | Current writing order and configured check selection. Returns profile verdicts, evidence and result references. | `check_profiles::run`. |
| `worker report --input - --request-id KEY` | JSON `outcome` one of `done,blocked`, and `summary`. Validate current assignment and revision. Returns recorded outcome and evidence ID. Independent reviewer submits a review through its separate capability. | `call_in`, `orders::report/report_findings` and report-memory write. |
| `result show ID [--tail N | --offset N --limit N]` | Authorized bounded excerpt with byte/line range, status and continuation. Human full export streams separately from agent-facing limits. | Shared result resolver, seeded by `check_logs::read` and new execution artifacts. |

Success example after Phase 4:

```json
{"ok":true,"summary":"42 tests passed","result_id":"run:872","state_revision":148}
```

Failure example:

```json
{"ok":false,"error":{"code":"CHECK_FAILED","message":"checkout_total expected 42, got 41","retryable":false},"result_id":"run:872"}
```

Version the envelope. Exit 0 for successful operations, 2 for invalid input, 3 for denied or missing scope, 4 for revision/cursor/idempotency conflicts, 5 for failed work/check and 1 for infrastructure errors. Include a machine-readable code such as `PERMISSION_REQUIRED`, `INVALID_INPUT`, `STALE_ORDER`, `CURSOR_EXPIRED` or `CHECK_FAILED`. Keep JSON on stdout and human diagnostics on stderr. Bound input/output and distinguish a rejected operation, a completed command with nonzero exit and an uncertain write outcome. Keep references resolvable under the existing permissions even when commands fail.

## State, security and concurrency design

- Begin inbox changes with existing monotonic message IDs as event positions. If measured need warrants a broader state revision, increment a serde-defaulted counter on real mutations under `Task::transaction`. Keep application/order revisions independent. Cursor encodes task incarnation, recipient, schema and page position. Limit reads to a captured high water and never advance a cursor past an unreturned eligible event. Invalid, expired or wrong-incarnation cursors return a reset condition. Use a bounded pending snapshot through the same high water, then resume deltas. Retain unacknowledged messages until acknowledgment. Holes in IDs are normal.
- Use per-recipient acknowledgment receipts/tombstones within replay retention if needed for stable retry. Batch validation, ack, receipt write and pending projection in one state transaction. Reject unauthorized or unknown IDs. An acknowledged ID reused within retention succeeds idempotently. Consumers acknowledge after handling, persist their checkpoint or recover through pending replay, and tolerate duplicate delivery. Preserve the nudge high water independently. Store event/ack metadata with state changes. An audit-log append failure after rename must not trigger a second mutation.
- For issue and report, scope idempotency keys to task incarnation, principal, operation and key. Hash canonical validated user input before enrichment. Replay same request with the prior result; reject key reuse with a different input. On report, bind the in-flight key to the claimed order and revision, and look up committed receipts even after the order closes. Commit receipt and mutation together. For calls with external work, reuse the order lock and existing evidence/integration journal, record in-progress state, do expensive work outside the state lock, and revalidate order, permissions and generation at commit. Reconcile interrupted work before retry. Avoid unconditional retries of execute/checkpoint where an effect may already have occurred. Preserve existing lock order and add multiprocess regression tests for rebind versus execute/checkpoint/report.
- Preserve exact-call approval and the separation of product approval from external-action authorization. Do not hand the model the generic operator CLI. Bind capabilities in the launcher, reject caller overrides, scrub inherited privileged environment, avoid shell interpolation, validate paths and enums, and apply redaction and artifact quotas. Keep task state, Git administration paths, host credentials and Docker socket outside model reach. Worker authentication currently checks the expected registered path, current live agent and 64 lowercase or uppercase hex characters; it relies on possession of a restricted file path and trust in the caller's environment. A model with arbitrary same-user host shell undermines that boundary.
- Claude disables native tools in reviewed configurations, OpenCode enforces an allowlisted provider configuration, and Codex uses provider sandbox modes and restricted configuration. Verify actual capability confinement on each pinned provider. A role instruction or a launcher in an unrestricted shell is insufficient. Keep structured MCP doorkeeping when the only way to invoke CLI would expand shell access.
- Keep full execution stdout/stderr out of model context by default, stream to task-owned artifacts with size/retention policy, and disclose truncation or unavailable artifacts. Retain first actionable errors, file/line, failed assertions, stack frames and tail, with pagination when diagnostics exceed budget. Keep check evidence and reviewed candidate diffs bound to their order/revision, and protect binary data, secrets and untrusted log text. Ensure crash-safe artifact publication and clean orphaned partial artifacts. Preserve legacy responses behind a flag during migration.
- Add state fields with serde defaults and avoid recreating task or session identity. Plan downgrades explicitly. Older writers can deserialize past unknown fields and erase them on write. Disable new readers and drain in-flight operations before replacing binaries. Keep the upgraded binary for normal transport rollback. If downgrade is required, archive state and reconcile receipts and cursors before cutover.

## Prioritized implementation phases

| Priority and dependency | Work and source locations | Acceptance criteria |
| --- | --- | --- |
| P0. Phase 1, first | Instrument `mcp.rs::Server::handle`, `worker_tools.rs::mcp/panel/show_turn`, provider hooks and `panel_view.rs`. Extend `bench.rs` and `board_usage.rs` with provenance, coverage and role breakdown. Run current MCP on representative workloads before changing architecture. | Replay fixtures reconcile stream and transcript usage without duplicate counting. Publish a baseline with schema-discovery frequency, prompt segment sizes, operation payload percentiles, cost, retries, repair and integration success. Rank bottlenecks with uncertainty. |
| P1. Phase 2, after Phase 1 | Extract typed command and guard paths from `mcp.rs::Server::call/call_inner` only where needed, using `orders.rs`, `workflow.rs`, `status.rs` and `notes.rs`. Reuse authenticated `worker_tools::call_in` for worker actions. | Existing MCP contracts, permissions, side effects, replacement behavior and evidence pass parity tests. Both adapters share validation and authorization. |
| P1. Phase 3, after Phase 2 and provider confinement proof | Add Leader read commands in `main.rs`, capability wiring in `start.rs`, `leader.rs` and `worker_tools.rs`, then add order issue with request receipts. Provide a short command manifest and bounded help. Keep unported actions available through scoped MCP. | CLI cannot escape task, role, capability or generation. Benchmark B with equivalent payloads to A. Measure actual residual MCP schema size, CLI startup and discovery calls. |
| P1. Phase 4, after Phase 1 and shared contracts from Phase 2 | Optimize measured large outputs in `runner.rs`, `check_profiles.rs`, `check_logs.rs`, `status.rs` and `workflow.rs`. Add cursor semantics where warranted in `state.rs`, `task.rs` and `watch.rs`. | Compact versus original responses preserve repair and integration success. Complete authorized logs remain accessible. Stress tests pass for concurrent enqueue/page/ack, timeout retry, legacy state and restart. Transport and output switches are independent. |
| P2. Phase 5, after Phases 3–4 | Test C using `worker_tools.rs`, `runner.rs::authenticate`, `opencode_worker.rs` and provider launch settings. Prove constrained invocation for each role in the trial, including planners, reviewers and infra roles where applicable. | Worker parity and cross-scope denial tests pass before live trials. Compare with B at equal output formats. Keep MCP when incremental value or confinement falls short. |
| P2. Phase 6, after Phase 1 | Measure and experiment in `worker_tools.rs::panel`, `start.rs::with_memory`, `leader.rs::handed_memory`, `notes.rs` and `memory.rs`. Try task-scoped synopsis, snippets, stable refs and deduplication by note revision. Evaluate scoped retrieval before candidate limits where supported. | Test recall under cross-task noise, trust markers, no-memory mode and handoff/restart recovery. Shorten repeated instructions only after refresh tests. Rotate sessions only if measured growth dominates and continuation, recovery and isolation tests pass. |
| P1. Phase 7, after selected trials | Compare eligible A/B/C configurations, publish exact settings and canary per task and provider. | Meet the release gates below, demonstrate rollback through restart and successfully integrate a real multi-agent task. |

### Phase 1 measurement protocol

1. Record each turn with run, task incarnation, order/step, agent, role, provider, model, account alias, session and attempt, plus provider response IDs when available. Record duration, retries by cause, tool counts and request/result sizes per operation, execution times and delivery/evidence outcome. Capture sizes of prompt segments and catalog discovery calls. Use local pseudonyms in exports and omit raw prompts, tool arguments, credentials and log text by default.
2. Collect native counters with provenance and parser version. Canonicalize disjoint uncached input, cache read, cache write and output where available. Subtract the cached subset from inclusive Codex input once. Track Claude cache creation, including TTL classes where available. Deduplicate OpenCode by native message/part identity. Preserve native reasoning counters and avoid adding them to already inclusive output. Reconcile streams, transcripts and pane totals without combining overlapping records. Missing counters stay unknown. Price using a versioned provider/model rate table. Separate actual billing, equivalent estimated price and unknown cost. Report subscription capacity separately.
3. Correlate interactive Leader transcripts/hooks with turn boundaries and worker streams with supervisor claims, including interrupted attempts. Measure per-request effective input by successive turn ordinal, fixed prompt size and compaction markers when exposed. Controlled discovery/content trials with equal capabilities and response budgets estimate the upstream impact of schema and history where exact provider metadata is unavailable.
4. Publish p50/p95 payload size and total bytes by operation and role, context growth, cache ratios, failure recovery costs, and cost coverage. Compare compiler output, reviews, broad status, inbox calls, retrieved memory, repeated instructions and discovery costs. Carry the local JSON-RPC size assay forward as a regression fixture.

### Comparative benchmark and release gates

Extend `agentic/bench/fixtures`, `scripts/bench_team.sh`, `docs/user/devagentics-bench.md` and the existing `devagentics bench report --json` / `bench compare`. Use paired randomized A/B/C order with identical commit, task goal, provider/model/effort, image, tool capability, integration checks and task timeout/budget. Isolate fresh worktrees, session stores and test memory per run. Stratify cold and warm caches and record versions and load. Keep result shape and context policies fixed in the transport trial. Add a compact MCP control and a compact CLI arm to isolate compaction effects.

Cover a single-file change, multi-file backend feature, frontend/backend integration, failed test repair, multi-agent handoffs and an interrupted worker. Include noisy build output and competing task memory in fixtures. Start with at least ten paired repeats per scenario and eligible provider, then size the comparison using pilot variance and the reliability margin. Verify acceptance tests on the integrated branch, milestones and required review. Record interventions, fallbacks and spend on failed/interrupted attempts. Follow fixtures with real opt-in tasks.

The primary metric divides total attributable provider spend across **all** attempted runs, including failures and retries, by the count of successfully integrated tasks. Zero successes is a failed arm. Publish role-level token and cache usage, estimated and actual cost coverage, completion, first-pass and test/integration outcomes, latency p50/p95, retries, tool calls and interventions. Use pair-aware intervals grouped by workload and provider and report strata before pooled results.

Suggested preregistered gates, subject to baseline variance and stakeholder review:

- B improves cost per success by at least 10% over A, with a 95% interval supporting some improvement. C improves by at least a further 5% over B, with equivalent statistical support. Aim for priced coverage on at least 95% of observed turns and comparable coverage across arms. Report missing coverage and sensitivity to it.
- Require completion and quality non-inferiority at an agreed margin. Start with a five-percentage-point completion margin, no new class of acceptance failure and no increase in required manual intervention. Flag p95 latency regressions beyond 10%. Treat underpowered comparisons as inconclusive.
- Any authorization, isolation, lost committed message, duplicate mutation or validation-integrity failure blocks rollout of the affected configuration.

### Verification and rollback

Add unit/replay coverage for usage formats and deduplication, response budgets, diagnostic extraction, permission and input validation, idempotency, cursor expiry and legacy state. Add adapter contract tests for equivalent permissions, state, evidence and filesystem effects. Use multiprocess tests with injected interruption around state rename, audit append, session capture, ack and artifact publication. Cover wrong scope/generation, excluded paths, container enforcement, pause and stale order races, review and validation failures, and dispatcher nudge parity. Run `make check` and the focused Docker/worker/provider matrix before rollout.

Ship telemetry, output controls and transport switches independently. Canary B by provider/task. Disable a failed feature, drain or reconcile in-flight calls and restart the approved adapter in the same sessions where compatible. Replay pending inbox messages and preserve idempotency receipts, artifacts and evidence. Use the upgraded binary for MCP rollback. Continue with compact MCP where CLI savings are uncertain, discovery offsets catalog savings or CLI requires broader access.

### Audit validation

The plan used the fresh offline build and real local fixture responses. Live provider token attribution, billing, typical result sizes and comparative cost and success remain Phase 1 and Phase 7 deliverables.

The audited DevAgentics crate passed formatting, Clippy with warnings denied, and 481 tests with six ignored. The Docker and live-provider benchmark matrix was outside this planning run.

### Phase 1 implementation progress

- Added opt-in task-local MCP payload and managed-worker turn telemetry in
  `telemetry.rs`, `mcp.rs` and `worker_tools.rs`. The existing JSON benchmark
  report summarizes call counts, failures, byte totals, response p50/p95 and
  worker-turn coverage. Missing team usage remains unknown.
- Verified unchanged MCP responses with telemetry on and off against a
  temporary fixture, including Leader and worker discovery. Added provider
  parser tests for Claude cache writes, Codex cached input and OpenCode
  duplicate usage records.
- Interactive Leader per-turn attribution, stream-to-transcript reconciliation,
  pricing coverage and the live workload baseline remain open. Keep transport
  changes behind the Phase 1 gate.

### Live baseline attempt, 21-11-2026

The configured Claude Leader started the reviewed `notes` fixture and
requested the MCP tool catalog once. The catalog response was 21,945 wire
bytes. Claude reported that the account had reached its weekly limit before
it processed the fixture goal. No workers ran, no steps were accepted, and
provider usage was unavailable. The isolated tmux session was stopped to
prevent an automatic retry. This is startup evidence only and does not
satisfy the baseline gate.

### Codex baseline attempt, 21-11-2026

Codex 0.171.0 started the reviewed fixture in an isolated DevAgentics home and
tmux server. Startup exposed that Codex did not pass the configured
`DEVAGENTICS_HOME` to its MCP subprocess. A task-local shim restored Leader
MCP and tmux routing. The repository now explicitly forwards the task home,
runtime directory and tmux identifiers in both Codex MCP configurations.
The infra and planner turns connected through the Worker MCP configuration.

The infra worker built and verified the Python runner. The planner eventually
submitted baseline revision 1. Before its first valid submission, 26
rejected calls retried the same broad operation in response to argument and
contract validation errors. This makes planner request discoverability and
diagnostic quality candidates for cost reduction. The trial incurred manual
startup recovery and paused before writers started for exact-revision
approval. It provides real payload and worker token observations, with
no accepted-step cost and no valid completed-task baseline.

After explicit approval, the Codex team implemented both components and
checkpointed them on the Notes lane. Independent execution of the original
unittest command passed all 11 tests, and Git verified the original acceptance
files and contract were unchanged. The managed checks rejected stdout-only
unittest results because the required test-count JSON was missing. Infra
verified a compatible adapter against success, failure, skip and empty-suite
controls. Revision 3 awaits exact approval because it changes check commands.
The work has not passed managed integration. The current report has 11 worker
turns with stream usage and incomplete pricing, so a displayed zero cost is
not a valid estimate.

### Codex integrated result, 22-11-2026

The approved test-count adapter unblocked both managed component checks
without changing the suites. After independent review caught two API parsing
defects, the corrected candidate passed the full managed check, was approved
and merged. The original command passed all 11 tests on the merged branch.
The original goal, contract and protected acceptance files match the fixture
commit, and the merged worktree was clean.

The strict benchmark outcome remains blocked. Milestone validation requires
an accepted reviewed-integration journal for every required item, including
the read-only audit. A read-only audit cannot generate the journal through
the reviewed implementation path. The final audit and full check were
performed, and the benchmark was paused with the milestone still unready.
Record it as merged and tested, with a failed milestone gate. Do not count
it as a successful run under the preregistered benchmark definition.

Observed local telemetry was 650 tool calls with 124 errors, 1,172,925
response bytes from 50 Leader task-status calls, and 605,705 request bytes
from 37 proposal calls, 34 of which failed. By comparison, 25 tool-catalog
discovery responses totaled 82,622 wire bytes. All 23 completed worker turns
had stream usage. They reported 1,501,422 uncached input tokens, 36,340,224
cache-read tokens and 173,234 output tokens. These are managed-worker stream
totals. They overlap other usage sources and must not be added to the pane
report. Pricing is incomplete, and the run included manual recovery and
approval waits.

For Phase 1, prioritize measuring targeted status responses and proposal
validation improvements before testing transport. These local observations
identify large payload sources in one run. They do not measure catalog
inclusion in upstream model requests or establish financial savings.

### Milestone gate repair, 22-11-2026

The gate now accepts a completed findings-only item with a delivered, closed
order for its current read-only role. Earlier writing orders or unknown
authors still require reviewed integration, and existing journals still
require an approved review of the same candidate. Final configured checks
remain mandatory for every required item. Regression tests cover the
read-only completion and reassignment from a writing order.

Using the same integrated benchmark, the existing read-only report was
accepted and all four rootless Docker milestone phases passed. DevAgentics
now reports `notes-release-v1` ready. The benchmark qualifies as an
integrated delivery under the corrected milestone gate. Incomplete pricing
still prevents a reliable cost-per-success estimate.

### Targeted Leader status trial, 22-11-2026

Added the state-only `task_status` view `progress` to the existing MCP tool.
It returns revision and approval state, pending counts, active steps needing
attention first, and live managed-worker/order information. Active steps and
agents each have a 20-entry cap and an uncapped total. The no-argument
contract still returns the full status. The Leader role uses the smaller
view for routine progress, and telemetry tracks it as a separate operation.

A paired local read of the same saved Notes benchmark snapshot returned
28,120 wire bytes for full status and 881 for progress, a 96.9% reduction.
Measured process-to-response times were 3.722 seconds and 0.009 seconds
respectively in that pair. This is one local observation. Provider token
savings, how often Leaders need to escalate to full status and any effect
on completion still need a controlled run.

### Batched proposal feedback, 22-11-2026

Full Leader and worker proposals now share the input decoder and baseline
validator. The decoder reports the first shape error in each malformed
decision, contract, milestone or repository check definition. The baseline
validator groups independent semantic errors and includes item IDs and
field names where applicable. Feedback is capped at 16 findings with an
omitted count. Existing check, scope, dependency, protected-test, revision
and approval rules remain in force. Both catalogs document the required
reason.

Replayed two rejected Codex planner submissions against an isolated task.
The malformed proposal returned 14 diagnostic lines where it previously
returned one. The semantic proposal returned 16 findings plus an omission
notice where it previously returned one. The feedback grew from 99 to
2,756 bytes and from 24 to 793 bytes respectively. Both were rejected
without writing task state. The controlled provider trial should measure
whether the larger diagnostic response saves enough full proposal retries
to reduce cost per integrated task.

### Explicit benchmark price coverage

The benchmark report now publishes `estimated_api_cost` with a nullable
observed USD amount and counts of observed, unpriced and missing agents. The
human comparison labels incomplete amounts as subtotals. This summarizes
available pane and archived readings. It does not yet reconcile historical
sessions or price every captured provider turn. The saved Codex benchmark
reports an unknown observed cost and still reports the milestone ready.

### Reconciled Codex baseline

[Codex baseline data](codex-baseline.json) includes 440 unique responses from
three Leader threads and seven worker threads. The transcript records 29
turns, including all 23 worker turns. Differencing the worker stream's
successive cumulative totals per thread exactly matched transcript totals
for all 23 turns. The old worker stream totals double-counted earlier turns
after a resume. Version 2 subtracts the previous completed attempt for the
same thread. Missing predecessors leave usage unknown.

The run used 681,390 uncached input, 31,165,056 cache-read and 96,439 output
tokens. The maximum request used 173,017 input tokens, and the transcripts
reported no cache writes. The observed responses price to $4.56 at published
standard short-context API rates checked on 22-11-2026. That figure includes
$2.76 for the Leader and $1.80 for workers. It is an API-equivalent estimate
for a single integrated run, with manual recovery and approval waits.
It is not a subscription charge or comparative savings measurement.
[OpenAI API pricing](https://developers.openai.com/api/docs/pricing)

MCP remains the deployed transport. Reconciled provider usage gives a baseline
for comparing proposal feedback and targeted status in a fresh matched run.
The CLI pilot remains gated on evidence of incremental savings and equivalent
capability confinement.

### Paired proposal-repair trial

Replayed the same rejected Notes proposal against live Codex GPT-6.1 Sol at
medium effort, with up to four attempts per arm. Each attempt used a fresh
noninteractive thread, the same prompt template and the same validator.
The first-error arm received only the first finding from the current
validator, which approximates the former policy. The batched arm received
all the bounded findings. Each candidate was validated in a disposable task
directory. This trial measures proposal repair, without running a team or
modifying the integrated fixture.

Neither arm produced a valid proposal within four attempts. Estimated
API-equivalent inference was $0.1954 for first-error feedback and $0.2005
for batched feedback using the frozen baseline rates. Cost per successful
repair is undefined for both arms. One seed and four attempts per arm are
insufficient to infer a general cost difference. The next candidate is a
small, locally validated proposal example or on-demand schema guidance,
especially for named check definitions and test ownership. Keep the same
validation gates and rerun with multiple seeds before claiming savings.
[Per-attempt results](codex-proposal-trial.json)

### On-demand proposal example trial

Leader and planning Worker tools now expose a shared `proposal_example`
action. Its example is tested against the real submission validator. It
demonstrates test-first ownership, a named check environment, decision
prerequisites and milestone IDs. Validation errors point to it on demand.

Three recorded Notes proposals were replayed against Codex. A first example
with an extra response wrapper produced zero accepted proposals in nine
attempts. Two first replies copied that outer wrapper and had no top-level
application. The final example returns exactly the two submission fields
and includes a decision with prerequisites. In the follow-up, it eliminated
the observed outer-wrapper error, but still produced zero accepted
proposals in six attempts. The pilot changed both the example and the
return-format guidance, so it does not isolate their individual effects.

Recurring blockers included malformed named check definitions and the
validator requiring a test-writing dependency to own an existing immutable
acceptance test. Keep logic for newly authored tests test-first. Evaluate
explicitly distinguishing pre-existing, Git-tracked acceptance files from
new tests, with regression coverage preserving the write-path gates. The
pilot did not demonstrate cost savings or justify a transport change.
[Per-attempt data](codex-proposal-example-trial.json)

### Existing acceptance test protection

Reviewed baselines can declare `application.existing_protected_tests` by
repository. Proposal and approval require each entry to match an exact
committed regular file on the task branch. The order policy adds those
paths to every reviewed writing order in the repository. New
`protected_tests` still need a direct test-writing dependency. An approved
protection change invalidates affected orders and check evidence. Removing
existing protection requires user approval.

Replayed the saved Notes baseline through Leader MCP in an isolated task.
The original mixed declaration still rejects the existing acceptance file
as a newly authored test. With the tracked file classified separately,
the same baseline stores successfully and remains unapproved. This
verifies the gating fix without launching another provider run.

### Named check diagnostics

Proposal feedback now identifies malformed named checks by repository,
profile and lane override. A missing profile environment shows the exact
field path, with one repair example per repository. A stored Notes rejection
that previously described only a missing field now identifies four affected
profiles in one response, with no task state written. The typed parser and
runtime gate still require the same check metadata. This is an offline replay
result. Proposal retry reduction still needs a matched provider trial.

### Matched replay after named-check and existing-test changes

Ran the same three rejected Notes inputs with Codex GPT-6.1 Sol at medium
effort, the same prompt and example, and a limit of two attempts per case.
The earlier run accepted zero of three cases. The new run accepted one of
three. The accepted proposal put the existing Notes acceptance file in the
repository protection list, kept new focused tests under test-first
ownership, and included an authoritative check profile.

The earlier and new runs used six attempts each and cost $0.3065 and $0.3155
in API-equivalent terms at the frozen benchmark rates. This is one replay
per saved input, and the compared runs used different validator builds.
The accepted proposal was never approved or executed. The replay shows an
improvement in observed submission acceptance but cannot establish a
general success rate, causal attribution, or delivery-cost savings.
[Per-attempt results](codex-named-check-trial.json)

### Paired live status interpretation

Sent full and compact responses from the same paused Notes state to fresh
Codex GPT-6.1 Sol threads at medium effort, using the same question and
answer contract. Both arms answered all five progress fields correctly.
Compact status used 15,819 input tokens versus 21,157 for full status,
saving 5,338 input tokens (25.2%). Both arms reported 12,288 cache-read
tokens and 31 output tokens. Applying the frozen baseline rates gives
$0.00860 for compact status and $0.01928 for full status.

This is direct provider usage for one isolated status interpretation, not
an integrated development run. The snapshot had no active steps or open
orders. A broader trial should cover active work, recovery and decisions
that require falling back to full status. The result supports using the
existing compact MCP view for routine progress and supplies no evidence
for changing transport.
[Trial results](codex-status-trial.json)

### Active-status and fallback trial

The scratch snapshot reconstructed open order o48 using its original worker,
step and plan reference. Both Codex arms answered all five routine progress
fields correctly. Compact input used 15,850 provider input tokens versus
22,541 for full status, a 29.7% reduction. Both inputs had 12,288 cache-read
tokens. The API-equivalent estimates are $0.00908 and $0.02214 respectively.

The diagnostic question asked for the execution plan referenced by o48,
which compact status omits. The compact arm correctly requested full status;
the resumed fallback found the exact plan. Its cumulative total was 39,384
input tokens. The direct full-status arm incorrectly requested full status
again despite receiving it, so it did not deliver the answer. Cache counts
differed in the diagnostic arms. This single trial cannot establish the
relative cost of successful diagnostic paths. It motivates testing a direct
order lookup that returns the requested evidence in one small response.

[Per-arm data](codex-active-status-trial.json)

### Direct order lookup trial

Leader `task_status` now accepts `view=order` and an order ID. The state-only
response includes the order's plan, profiles and capped check evidence,
including a comparison of evidence and order revisions. It supports
closed orders. Use `read_check_log` for diagnostic excerpts. Full status
and its existing request shape remain available.

The same Codex plan-reference question from the active-status replay was
answered correctly from a 524-byte order response with 15,799 provider
input tokens. The earlier successful path used 39,384 input tokens across
compact status and a resumed full-status read. The lookup used 59.9% fewer
input tokens in this comparison. The earlier and current calls ran at
different times and cache states. This is a narrow diagnostic comparison
with one order and does not measure task cost or general reliability.
[Trial results](codex-order-lookup-trial.json)

### Worker plan reads

Worker `plan_read` returned about 814 KB across 23 calls in the original
Notes ledger. It now supports an outline with heading line numbers and an
excerpt with a one-based start line and continuation. Both are state-only
reads. Full output remains the default for existing callers. Telemetry tags
the smaller views separately.

On the same saved plan, Worker MCP returned 51,876 wire bytes for the full
plan, 605 for the outline and 5,382 for an excerpt of lines 1–18. In a paired
Codex interpretation, both views answered all four questions about file
scope, checks and dependencies correctly. Input fell from 26,860 to 16,677
tokens (37.9%). The two requests had identical cache-read and output
counts. The trial supplied a known excerpt range and did not include an
outline-discovery model turn. Measure discovery and fallback separately
before treating the difference as a live task saving.
[Trial results](codex-plan-excerpt-trial.json)

### Brief Worker status trial

Managed worker prompts now ask for `status` with `input.view=brief` to
retrieve their open order, authorized scope, plan, check profiles and
pending question or review references. Use full status for entire
contracts and history. The no-argument API still returns full status.

The same reconstructed active Notes order returned 21,290 wire bytes
for full status and 994 for brief status. Both Codex arms answered all
four assignment questions correctly. Provider input was 20,201 versus
15,835 tokens, a 21.6% reduction with equal cache-read and output tokens.
This measures assignment lookup only. The trial did not measure full
contract lookups, later tool calls or delivery reliability.
[Trial results](codex-worker-status-trial.json)

### Captured container output

Added opt-in Worker `execute` compact output and `result_read` through
the same container runner. The response includes exit status and bounded
head/tail previews. Capture is staged outside the container mount and
published only after both streams finish. Retrieval is scoped to the same
task, worker and identity generation. Captures are capped at 2 MiB per
stream and 32 per worker generation. The existing execute view is unchanged.

A real rootless Docker replay used a read-only scratch snapshot and a
deliberately failing command. Compact output preserved the traceback,
final assertion and exit code 7. Its MCP response was 2,844 bytes versus
69,824 for the existing response. Paging reconstructed all 110,011 stdout
bytes and 4,058 stderr bytes. These measurements describe a noisy command,
without a provider call or an assertion about cost per completed task.
[Capture replay](execute-capture-report.json)

### Live compact execution diagnosis

A paired GPT-6.1 Sol replay compared the same deterministic noisy failing
container command rendered through full and compact Worker MCP execution.
Both Codex arms correctly identified failure, the failing test and the
exact assertion. Compact execution used 16,021 input tokens versus 32,364
for the full result (50.5% fewer). Estimated API-equivalent cost was
$0.00462 versus $0.04168 using the baseline pricing snapshot. The compact
arm had 2,304 more cache-read tokens, so the price difference combines
payload and cache effects. The compact preview carried the diagnostics
needed in this case. The test did not measure artifact retrieval or a
subsequent repair.
[Trial results](codex-execute-token-trial.json)

### Buried diagnostic and verified repair

The scratch Python fixture had two similar functions and a two-test suite.
The only failure's traceback was between setup and cleanup noise, outside
the compact preview. The direct full-response arm proposed the correct
one-line edit. The compact arm requested stderr instead of guessing.
One result page exposed the traceback, and the resumed arm proposed the
same edit. Both edits were independently applied to the scratch fixture
and passed the original two tests in rootless Docker.

The direct path used 30,793 input tokens. The compact-plus-retrieval path
used 34,148 cumulative input tokens (10.9% more). Its API-equivalent
estimate was lower, $0.01756 versus $0.03844, because cache reads differed
substantially. This trial demonstrates recovery and confirms that fallback
can increase total input. It does not isolate a general cost advantage or
measure a multi-agent delivery.
[Trial results](codex-execute-repair-trial.json)

### Inline diagnostics after a compact failure

Compact failures now return a bounded selection of distinctive diagnostic
lines from the captured streams. Lines already shown in the head/tail
preview are omitted from this selection. The same buried-assertion case
now exposed the failed test and assertion in the initial compact response.
Codex proposed the same correct edit without `result_read`, and both
original tests passed after the edit in the scratch Docker fixture.

The new response was 3,186 wire bytes and used 15,415 model input tokens.
Earlier successful paths consumed 30,793 input tokens with the full
response and 34,148 cumulatively with compact plus retrieval. The new path
used roughly 50% fewer input tokens than the direct full read and 55% fewer
than the two-turn retrieval. These are single runs with different
cache states. Logs remain available when a failure has unusual formatting
or the selected lines are insufficient.
[Trial results](codex-inline-diagnostic-trial.json)

### Fresh optimized end-to-end Codex run, 22-11-2026

The fresh run used the same initial Notes Git tree and verbatim goal,
a pinned binary from the optimized working tree, and the same Codex
model families. The team implemented both components, corrected two
independent-review findings, merged the reviewed candidate, and marked
its milestone ready. The original discovery command passed all 14
tests in a read-only, network-disabled container on the merged tree.
Git verified the original goal, shared contract and acceptance tests
were unchanged.

The completed baseline and optimized runs used $4.56 and $3.07
API-equivalent respectively at the same frozen rates, a 32.7% observed
reduction. All 349 optimized response records were priced and the 22
worker stream turns matched the transcript totals. Tool response traffic
fell from 3.30 MB to 1.09 MB, excluding discovery/handshake messages.
Leader status traffic fell from 1.17 MB to 94 KB across full, progress
and order views. Full proposals were rejected twice in the optimized
run compared with 34 times in the baseline.

These are two completed deliveries, one per configuration. The optimized
run reused the existing pinned image, had fewer startup retries and
followed a different review and planning path. Both had operator
interventions. The original acceptance suite was identical, but the
teams authored different focused tests. The observed cost difference
does not isolate the effect of the optimizations, estimate variance,
or justify a transport change. Replication with startup conditions
and approval flow held fixed remains open.
[Comparison data](codex-optimized-comparison.json)

### Matched Codex Notes replay, 22-11-2026

Two isolated teams started from the same Notes tree and goal, pinned the
same runner image and used the same Codex model families and counted
check profiles. Both used MCP over the same application code. The control
changed prompt and tool defaults to favor full responses. It still used
six compact progress calls and two direct order lookups. Both teams
received approval for their exact revision 1 and used revision 2 to
resolve an execution dependency without changing scope or checks.

| Metric | Full-response defaults | Compact defaults |
| --- | ---: | ---: |
| Observed API-equivalent Codex cost per completed delivery | $3.40794 | $1.77544 |
| Provider responses priced | 400/400 | 241/241 |
| MCP tool calls | 531 | 319 |
| MCP tool response bytes | 1,331,113 | 706,885 |
| Managed Codex worker turns | 29 | 14 |
| Merged-tree unittest passes | 13 | 14 |
| Milestone | Ready | Ready |

Observed cost fell 47.9% and tool response bytes fell 46.9%. Wire totals
exclude discovery and handshake messages. Worker stream usage matched
deduplicated transcript usage in both arms. The independent reviews
required a persistent-connection fix in the control and an HTTP error
handling fix in the compact arm. Both preserved the original goal,
contract and acceptance files. The original discovery command passed
against both merged source trees in a read-only, network-disabled
container.

The control received one extra operator message with the same check map
and paid for additional evidence reconciliation. A reproduced defect
allowed lane integration to select a newer reported order for a
different step. The workspace fix passes the requested step into
`lanes::merge_into_task`, keeping the existing approval and candidate
validation. A Docker regression accepts an approved step while a newer
unreviewed report remains pending in that lane. Both benchmark binaries
were pinned before the fix. Shared quota interrupted both arms and
temporarily triggered Claude fallback before both resumed their original
Codex sessions. The table prices observed Codex usage at frozen API
rates. The two runs also had different cache histories and separately
authored focused tests.

Keep compact MCP responses with full-detail fallback for limited Codex
rollout. Retain the current transport and provider sessions. The
observed savings include differing review paths and establish an outcome
for this workload. Broader rollout requires representative workloads
and measurements after the review-selection fix. Revert compact defaults
to full responses if review, test or completion rates regress.
[Comparison data](codex-matched-comparison.json)

### Publisher frontend canary preflight, 23-11-2026

Two prior Publisher benchmark preparations agree on archived starting
tree `ce11bbbd68b448789791f529d5e3f221f0f8c45f` and contain the
committed queue-gap acceptance test. The original external source
commit is unavailable in the DevWS checkout. The preflight used the
prepared tree and local image
`sha256:c00609b888c2cd16bd7c362f00523b8fba63b80fdff1ea3d5db95046de4e96bc`.
Record the prepared tree ID for both arms if this archive seeds the
paired replay.

The first network-disabled preflight passed lint and typecheck but
failed 24 tests that check the existence of backend PHP sources
referenced by frontend code explanations. The publisher fixture runner
now includes those 24 exact tracked paths in its source snapshot. The
goal and order scope remain web-only. The second preflight passed lint,
typecheck and 1,995 of 1,997 tests across 188 files. The two failures
were both in the immutable queue-gap acceptance file, as expected before
implementation.

The rootless Docker runner matrix passed, covering read-only execution,
timeout enforcement and both reviewed integration paths. The installed
OpenCode 1.19.6 adapter passed its inherited-project-tools permission
test. The benchmark preparation suite passed. The top-level
`make check` stopped at its Ruby template checks because Ruby is
unavailable here. The Publisher preflight consumed no model quota and
provides the red baseline for the live paired measurement.

### Release scope, 23-11-2026

The operator selected the completed Codex Notes evidence for this release
and asked to end the Publisher trial. Both Publisher tasks were paused
during planning before issuing worker orders. Their runs contribute no
completed-task cost comparison.

Ship the compact MCP views and the existing full-detail paths together.
Use full details for any call whose compact response is insufficient.
The measured improvement covers reviewed Codex Notes deliveries.
The CLI adapters, inbox cursors and session rotation remain proposals
outside this release. The current dispatcher, task state, provider
session resumption and validation paths continue to serve the compact
views.

### Reviewed Codex Leader CLI pilot

The pilot uses the existing read-only Codex Leader shell. It adds two
operations, `leader-read <task> progress` and
`leader-read <task> order <id>`, with the same JSON projections and
output limit as MCP. The operator enables them per reviewed Codex task
with `task leader-cli <task> on`. The launcher advertises them only for
an opted-in Codex Leader and uses the binary's quoted absolute path.
Direct reads require the primary worktree root. The task reader takes
the existing shared lock and opens it read-only when it already exists.
The setting defaults off in older state. Turning it off immediately
disables the CLI entry point.

The worktree check scopes the command. The existing Codex sandbox
provides filesystem enforcement. The pilot leaves the initial MCP
catalog available for fallback and measures the cost of using the CLI
for routine reads under that condition. The acceptance test compares
CLI and MCP JSON, rejects unrelated worktrees and checks rollback.
A local smoke test ran the progress command inside the installed Codex
read-only sandbox, verified that task state bytes stayed identical, and
confirmed that a worktree write was denied.
Live cost per integrated task remains to be measured against compact
MCP with matched fixtures.

#### Live Codex read comparison

Four fresh non-interactive turns with Codex CLI 0.171.0 and
`gpt-6.2-sol` at low effort used the same reviewed task and read-only
sandbox. Each turn fetched task progress and the same closed order, then
returned four expected fields. Both CLI turns made exactly two successful
shell calls and both MCP turns made exactly two successful status calls.
The extracted JSON matched and each pair left task state unchanged.

| Pair and order | Interface | Input tokens | Cached input tokens | Output tokens |
| --- | --- | ---: | ---: | ---: |
| First, 1 | CLI | 48,945 | 46,208 | 134 |
| First, 2 | MCP | 51,765 | 44,928 | 132 |
| Repeat, 1 | MCP | 51,645 | 44,928 | 142 |
| Repeat, 2 | CLI | 51,646 | 44,928 | 142 |

The first CLI turn used 5.4% fewer input tokens. The reverse-order
pair differed by one input token with identical cache and output counts.
Both arms loaded the same MCP catalog. The result verifies live
invocation and response parity for the read pilot. Establishing a cost
benefit requires integrated task runs or a separate scoped catalog trial.

#### CLI-first status and the scoped catalog

An opted-in Codex Leader now starts MCP with a full-status-only
`task_status` advertisement and uses the direct CLI for progress and
order lookup. Existing MCP clients retain all status views. The pilot
keeps full status available as a fallback and keeps the operations that
need mediated writes or approvals on MCP.

The local `tools/list` response fell from 22,163 to 21,859 bytes including
the line terminator. The catalog retained 36 tools. In a fresh read-only
Codex pair, both interfaces returned the expected task and order fields.
Compact MCP used 51,763 input tokens, with 48,000 cached, and 131 output
tokens. The direct CLI with the scoped catalog used 51,749 input tokens,
with 41,856 cached, and 134 output tokens. The total input difference was
14 tokens. Cache mix differed, so this pair provides no evidence of
lower cost.

The broader target is CLI-first for routine operations, with MCP used
for capabilities that require mediation. This pilot narrows discovery
by one schema. The provider still loads an MCP catalog at startup.
Measure the end-to-end Notes comparison before broadening the CLI
surface. Any CLI write path must reuse the existing authorization and
integration gates and work under the same provider confinement.

#### Completed Notes hybrid comparison, 23-11-2026

Both arms ran the same pinned binary against the same starting Notes
tree and rootless runner image. Compact MCP used the regular catalog.
Hybrid used direct Leader CLI reads and the scoped status advertisement.
Both arms independently reviewed and merged their corrected candidate
and marked the Notes 1.0.0 milestone ready. The original unittest
discovery command passed in the pinned read-only, network-disabled
container. Goal, contract and original acceptance files were unchanged
in both final trees.

| Metric | Compact MCP | Hybrid CLI |
| --- | ---: | ---: |
| API-equivalent Codex cost per completed task | $4.12937 | $3.19817 |
| Observed provider responses priced | 441/441 | 416/416 |
| MCP tool calls | 581 | 466 |
| MCP response bytes | 1,681,904 | 1,009,895 |
| Direct CLI progress and order reads | 0 | 15 |
| Independent merged-tree test passes | 13 | 21 |
| Milestone | Ready | Ready |

Observed cost was 22.55% lower in the hybrid run. All 15 direct reads
succeeded. The hybrid Leader made no compact-view status calls through
MCP and used full MCP status three times. MCP counts exclude discovery
and handshake messages. Both runs hit quota after merge and resumed
with explicit recovery for unfinished evidence refreshes. Deduplicated
transcript usage includes responses around those interruptions where
worker stream coverage is incomplete.

The control had more worker turns and review identities. The teams also
authored different focused tests and repaired different defects. This
pair measures delivered cost for Notes. The targeted read experiments
showed near-equal token use under comparable cache counts, and the live
workflow comparison does not isolate transport savings.
[Comparison data](codex-hybrid-comparison.json)

#### Direct Leader inbox read

The opted-in Codex Leader can use `leader-read <task> inbox` from the
primary worktree. It returns the same Leader-only messages, roster and
ready work as an MCP inbox read. Reading leaves the queue unchanged.
Acknowledgment continues through the existing MCP action and state
transaction. The dispatcher wake-up and combined MCP acknowledgment
and read remain available. Integration coverage compares CLI and MCP
output, verifies unchanged state bytes and checks that acknowledging a
Leader message leaves the worker queue intact. Live cost impact remains
unmeasured for complete deliveries.

In a live Codex two-turn comparison, both paths acknowledged only ID 20,
returned ID 21, and preserved pending Leader and worker messages. The
combined MCP path used one tool call and 34,212 input tokens, including
30,720 cached. A separate MCP acknowledgment and CLI read used two
calls and 69,931 input tokens, including 58,880 cached. Output was 89
and 159 tokens respectively. Each path ran once, with a different cache
mix. The one-call advantage is intrinsic to these command sequences.
The observed token difference is specific to the two turns.

The opted-in Leader now prefers the CLI for a standalone inbox read
and uses combined MCP when acknowledging and reading in the same turn.
The scoped catalog advertises the combined and fallback uses of
`leader_inbox`. It continues to advertise `inbox_ack` for an
acknowledgment alone.

#### Prioritize payload savings within the hybrid workflow

Keep hybrid routing. Preserve the combined MCP inbox call when it
saves a round trip. Prioritize the largest remaining response in the
completed hybrid Notes run. Full Worker plan reads returned 215,186
wire bytes across 15 calls, about 21% of all MCP tool response bytes.
Workers used one outline and one excerpt. The Worker tool description
now recommends outlines and excerpts for targeted plan reads. The
original full view and its default behavior remain available. Compare
bytes and provider tokens over the entire lookup, including any extra
calls and full fallback, before changing the default.

#### Live Worker plan lookup with actual tool calls

A 478-line Notes plan was copied into a disposable task. Fresh
`gpt-6.1-sol` low-effort Codex turns used the authenticated Worker MCP
reader to retrieve the milestone ID, version, candidate profile and
required merge approval. The first pair used full then targeted
lookup. Both found all four facts, with different answer formatting.
The repeat used targeted then full lookup, with a stricter output
instruction, followed by a single excerpt with the line range supplied.
All three repeat answers exactly matched. Task state stayed unchanged.

| Repeat path | Calls | Response content bytes | Total input | Cached input | Output | API-equivalent USD |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Outline then excerpt | 2 | 2,790 | 99,626 | 92,416 | 224 | 0.02590 |
| Full | 1 | 37,342 | 75,533 | 60,288 | 193 | 0.03845 |
| Excerpt with supplied range | 1 | 1,556 | 64,429 | 56,192 | 217 | 0.02426 |

The first pair showed the same total-input direction, with 99,572
targeted and 75,477 full input tokens. The targeted route used about
32% more total input in both pairs despite returning much less plan
text. It used fewer uncached tokens and had lower estimated cost at
the frozen Notes rates of $2 uncached input, $0.10 cached input and
$10 output per million tokens. These are cache-dependent observations,
with two pairs from one plan. The supplied-range excerpt used about
15% fewer input tokens than the full read in one replay. Supplying the
range removes the discovery step. A named-section lookup would need
its own test to verify that the same one-call result is possible
when the agent knows only the section title.

#### Named Worker plan sections

Worker MCP `plan_read` accepts `view: "section"` and `heading` for an
exact title. It uses the same Markdown heading rules as the outline,
includes nested headings, and pages within the selected section using
the existing byte and line limits. Missing or duplicate titles direct
the agent to outline or excerpt. Full plan reads retain their existing
behavior. Telemetry records section reads as `plan_read.section`.

One live pair used the same 478-line Notes plan and retrieved the same
four facts in a single call per arm. Full read used 75,539 input tokens
and returned 37,342 content bytes. Named section used 65,846 input
tokens and returned 1,633 content bytes. Both had 55,680 cached input
tokens and returned 193 and 192 output tokens respectively. The named
read reduced total input by 12.8% in this pair. The tested prototype
echoed the title in its response. The final build omits the echo and
passed a direct Worker MCP check against the same fixture. One fixture
and one pair establish the local effect only.

#### Delivery replay prepared

A fresh reviewed Notes worktree is staged with the named-section build,
hybrid Leader routing and the earlier hybrid run's starting tree and
pinned runner configuration. The replay leaves the Worker free to
choose a plan view. Compare section, outline, excerpt and full reads,
including follow-up calls, and require an independently reviewed merge,
frozen acceptance files and a ready milestone before comparing cost per
delivery.

The only configured Codex account was at 97% of its weekly quota at
preflight. The provider reported a reset on 28-11-2026 at 16:52 São Paulo
time. The replay was left staged before launching agents so the
validation can run on a single provider without a foreseeable quota
interruption. This comparison uses an earlier completed run as its
control. It can check adoption and delivery outcome, with review paths
and cache differences recorded alongside cost.

#### Completed named-section delivery replay, 24-11-2026

Codex completed the replay after quota reset. Independent review
approved the exact merged candidates and the release milestone became
ready. A separate run of the original discovery command passed 13 tests
inside the pinned read-only, network-disabled container. The original
goal, contract and acceptance files were unchanged.

The replay cost $2.87160 API-equivalent versus $3.19817 for the earlier
hybrid run, a 10.21% observed difference. All 291 observed provider
responses were priced. Workers made 12 full plan reads (194,832 MCP
wire bytes) and zero section, outline or excerpt reads. The previous
hybrid run made 15 full reads and one each of outline and excerpt
(221,310 bytes across the three views).

The replay verifies delivery quality and shows that advertising the
named view alone did not produce adoption on this task. The end-to-end
cost difference reflects other factors including the different
review paths, authored tests and cache histories. Keep the named view
available and preserve hybrid routing. An adoption experiment would
supply the exact section heading with each applicable order and
compare that path with the same order without a section hint.
[Replay data](codex-named-section-delivery.json)
