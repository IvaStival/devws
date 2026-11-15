# Spec: Improve team throughput without weakening completion checks

- **Date:** 14-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** In progress — reviewed runner implemented and verified; scenario evaluation remains
- **Amendment:** User requested reference-project standards and explicitly approved leader-only database/persistent-data destruction and out-of-project mutations. Floor workers may delete scoped project source/artifacts. Implementation was subsequently authorized.
- **Planning amendment settled:** Whole-application discovery with the user precedes scope/backend/frontend decomposition and agent allocation. The user chose milestone application releases with separate scope/step IDs, and baseline approval plus explicit approval for material changes.

## Context and observed baseline

The user reports that the application team takes too long to finish. Read-only inspection of the local `feat-system-observer` task on 14-11-2026 found:

| Observation | Interpretation and limits |
| --- | --- |
| 262 accepted steps and one ordered step | Completed step count is not application completion; the plan has changed over time. |
| 57 steps with more than one attempt | Rework exists, but attempts alone do not identify its cause. |
| 37 failed check records: 20 mentioning file-policy violations, 17 other command/environment failures | These are record classifications, not 37 independently diagnosed bugs. |
| 278 check-duration records totaling 14,355 seconds | About four aggregate execution hours; concurrent intervals overlap and this is not elapsed delay or guaranteed savings. |
| Median check-to-accept interval of 22 seconds across 262 matched steps | Typical review latency is modest; very long intervals include pauses and possible user decisions. Removing reviews is not justified by this measurement. |
| Current task capacity is three agents; one planner and one implementation worker were live | A snapshot does not establish avoidable idle time or prove another step was ready. |
| `task.toml` has no configured named check profiles | Existing reusable evidence and structured environment phases are not being used by this task. |
| PostgreSQL/socket tests failed in worker sandboxes and were rerun by the leader | Environment capability should be checked before assigning the affected gate, without dropping it. |
| A new overall plan began after the previous final documentation sequence | Scope additions must be distinguished from slow execution of unchanged scope. |

The runtime already supports named checks, prerequisite commands, isolated environments, order-bound evidence reuse, lane ownership, amendments and persistent workers. The first changes should make these capabilities usable and measurable rather than duplicate them.

## Goals

1. Reduce repeated validation and environment-related retries while retaining every required gate.
2. Keep independent, approved work running when capacity and file ownership permit it.
3. Reduce administrative work and unnecessary agent restarts without removing human approval or leader review.
4. Explain elapsed time using evidence: implementation, checks, review, dependencies, environment, capacity, permission and scope changes.
5. Let specialized floor workers complete well-defined assignments without repeated corrective instructions from the leader.
6. Require an independent code-review agent to approve the exact proposed changes before any team-managed code merge.

## Requirements

### 1. Explain where time goes

- Add a bounded task performance summary to CLI/MCP status, with a board view or linked detail when useful.
- Record lifecycle events needed to distinguish ready, queued, delivered, implementing, checking, awaiting review, blocked and completed work. Track check wall time and reuse separately from worker activity.
- Distinguish actual observations from inferred intervals. Mark missing history as unknown; do not reconstruct exact implementation time from pane state or pretend the existing logs support a full critical-path analysis.
- Aggregate elapsed intervals correctly: do not add concurrent workers and label their sum elapsed task delay. Attribute environment failures, file-policy failures, retries and scope revisions separately.
- Measure lane catch-up, merge, history persistence and control-lock wait separately. Check commands already run outside the global control lock; do not assume all validation is serialized. Keep synchronous durable history until any alternative has an explicit recovery design.
- Report idle capacity only when a known dependency-ready item could run within lane, agent, permission and resource constraints.
- Prefer structured state and bounded/incremental log processing; do not add routine pane capture or repeatedly scan full transcripts.

### 2. Run required checks once for the same validated inputs

- Make named check profiles the recommended workflow for new tasks and provide a previewable migration path for legacy commands. Preserve existing task behavior until explicitly configured.
- Have worker guidance use the authoritative devAgentics check command after implementation. Reporting must reuse eligible evidence through the existing runner rather than independently rerun an identical full gate.
- Keep evidence bound to order, revision, base, HEAD, configuration, clean worktree and the existing environment restrictions. A changed input invalidates it; failed, skipped-required, missing or stale evidence is never accepted.
- Retain targeted TDD checks during development. Every completed implementation must still satisfy the project's required tests, lint, type checks and build. A targeted test is not a substitute for required full validation.
- Keep a required final integration gate after lane merges. Pre-merge evidence cannot prove the combined application passes.
- Do not add cross-order caches or trust prose claims that tests ran elsewhere. An external runner, if needed, must return validated evidence through an explicitly designed, authorized interface.

### 3. Detect unsuitable environments before expensive work

- Reuse check-profile prerequisites to verify required services, versions, connectivity and test capabilities before assigning work whose acceptance depends on them.
- Model preflight results with scope and freshness; a check in the leader's environment does not prove a worker sandbox has the same capabilities.
- Classify a missing environment as an environment block, not an implementation failure. Keep the required gate visibly outstanding.
- Route it to an already authorized compatible runner where available; otherwise identify the missing capability and ask for the necessary action. Never auto-escalate privileges, weaken a gate or accept skipped required tests.
- Avoid repeating the same known environment failure without a relevant change or explicit retry; retain an actionable blocked state while unrelated work continues.

### 4. Schedule approved independent work explicitly

- Extend execution plans with a small structured work manifest: stable item ID, lane, dependencies, expected file areas, required checks and acceptance criteria. Do not attempt to infer a reliable dependency graph from arbitrary prose.
- Validate dependency references, cycles, lane scope and required check configuration before dispatch. Existing prose plans remain supported and do not silently become auto-scheduled.
- Expose ready items and specific block reasons to the leader. The leader still chooses staffing, sends orders and reviews results in the initial implementation; a fully autonomous scheduler is outside this scope.
- Dispatch independent ready items across disjoint lanes within the configured agent limit. Count planners against that capacity and expose when a planner occupies a slot needed by implementation.
- Keep dependent items blocked until prerequisites are accepted and their changes are available. Serialize same-lane writes and merges where required; do not add workers just to make the team appear busy.
- Account for lane catch-up and integration cost before splitting work further. More lanes can add merges and invalidate checked HEADs; concurrency must follow real independence.
- Do not raise the three-agent limit or change provider/model choices automatically. Report capacity limitations so the user can make an informed change if desired.

### 5. Reduce avoidable handoffs and file-policy rework

- Prefer one reviewable implementation slice with one acceptance outcome over separate administrative orders that merely repeat answers. Record clarifications as existing messages/annotations unless they create a real deliverable or materially revise the plan.
- Reuse healthy workers across consecutive lane assignments, preserving the current role and ownership rules. Restart only for a concrete lifecycle, capacity or provider reason.
- Validate planned file areas and required supporting changes before dispatch. Offer an explicit amendment when unexpected work is necessary; never silently widen permissions.
- Reduce unnecessary repeated context in status and report messages while retaining durable links to plan, evidence, order revision and diff.
- Preserve overall user approval, accepted senior execution plans, independent code review, leader acceptance and all existing external-action and usage-limit consent boundaries.

### 6. Specialize workers around stable responsibilities

- Reuse existing role files, repository overrides and role loading. Introduce a small opt-in catalog of specialist templates rather than a second role system.
- Suggested responsibilities are backend/data, API/integration, frontend/UI, and documentation. Adapt names and stack guidance to the target repository; install or select only roles required by its actual backlog.
- Each specialist contract defines its responsibility, approved working area, repository conventions to consult, expected inputs, supported checks, output evidence and stop/escalation conditions. A specialist is a role and knowledge boundary, not permission to widen lane ownership.
- Match each work item to a required role. Detect unavailable or incompatible roles before dispatch; do not silently hand a specialist assignment to a worker lacking the required contract.
- Do not keep every specialist running at once. Spawn only for ready work within capacity, reuse healthy workers for consecutive compatible items, and retain one writer per lane even when several specialties concern it.
- Specialists may make routine implementation decisions within the accepted contract. They must ask when requirements conflict, an interface changes, supporting files exceed scope or an environment cannot run a required gate.
- Keep existing provider/model restrictions and fallback behavior. Renaming a generic worker is not sufficient evidence of specialization and does not guarantee fewer mistakes.

### 7. Give floor workers execution-ready assignments

- Seniors inspect the actual repository and existing reusable components before specifying implementation. Plans reference the inspected revision and concrete existing patterns; invented APIs and stale examples are not acceptable inputs.
- Every implementation item includes: objective and non-goals; required specialist and lane; prerequisite items; exact permitted file areas and known supporting files; existing units to reuse; input/output shapes and interfaces; expected behavior, errors and edge cases; required regression tests and observable outcomes; check profiles and environment requirements; acceptance criteria; and explicit conditions for asking a question.
- Where several items share an interface, settle its names, types, payload examples, errors and ownership before parallel dispatch. Use a contract or fixture that both sides can test. Mocks must agree with it and final integration still validates real behavior.
- Describe algorithms, migration ordering or tricky invariants where they remove ambiguity. Avoid prescribing every line of code or duplicating the repository in a long prompt; include enough detail to execute and verify the item independently.
- Add a plan-readiness gate before leader acceptance and dispatch. Mechanically validate required fields, dependencies, scope and configured checks; the senior and leader still assess semantic correctness. Presence of fields cannot prove a plan is correct.
- Resolve open questions and unmet prerequisites before marking an item ready. The floor worker performs a short contract/environment check at the start of the assignment and reports a specific blocker if it cannot proceed. This is part of the assignment, not a separate acknowledgement order.
- Pin the accepted plan revision to the order. Amendments invalidate affected readiness and evidence, identify impacted dependent items, and require explicit acceptance before affected work continues; do not force unrelated lanes to replan.
- The expected steady-state flow is: accepted execution contract → specialist implements and tests → authoritative check/report → independent reviewer approves → leader accepts and merges. Corrective orders remain possible when evidence warrants them, but should not be the normal way to finish specifying the work.
- Classify subsequent corrections as requirement ambiguity, stale plan/interface, scope omission, environment mismatch, implementation defect or integration defect. Compare same-scope first-pass acceptance and corrective turns before claiming improvement. Do not penalize workers for asking a necessary early question.

### 8. Require independent review before code merges

- Add a read-only code-review specialist role using the existing role mechanism. Select domain-specific review guidance for backend/data, API/integration or frontend work when appropriate; a separate process per domain is not mandatory.
- The reviewer is a different task-owned agent identity from the implementation author. It cannot approve its own implementation, mutate the reviewed tree, widen order scope, accept its own review as leader, or invoke a merge. Reviewer permission checks must be enforced by the runtime, not only by prompt instructions.
- Review the accepted execution contract, exact diff, surrounding code, interface compatibility, regression tests and authoritative check evidence. Passing tests alone are not review approval.
- Persist a structured result: request/order identity and revision, author/reviewer identity, source/base/target commit IDs, reviewed artifact/diff identity, verdict (`approved`, `changes_requested` or `blocked`), and actionable findings with file/line, severity and rationale. Nonblocking suggestions are distinct from findings that must be fixed.
- Require all blocking findings to be resolved and re-reviewed before approval. A worker's claim that a finding is fixed does not close it. A reviewer may document why a finding is inapplicable; preserve that discussion in the review record.
- Bind approval to the exact order revision, source HEAD, target HEAD and integration candidate. Changed code, contract, required evidence or integration context invalidates approval and requires refreshed review. Prevent a time-of-check/time-of-merge gap by revalidating identities under the merge/control boundary immediately before effects.
- Enforce the gate in the team merge path, including `accept_step` before lane integration. Direct CLI and MCP calls must share the same guard. Missing, rejected, blocked, stale or foreign review evidence must fail before Git mutates a branch. Default approval or leader prose is not a substitute.
- Cover final integration into a destination branch through the same guard if a team-managed final merge is introduced. The current PR tool creates a draft PR; it does not enforce remote/manual merges. Do not claim to protect external Git commands or repository-host merges without separately configured branch protection and reviewer integration.
- Task-to-lane catch-up may import only changes already accepted through this review gate. If catch-up creates new conflict-resolution code or an unreviewed integration result, stop and require review before continuing. The detailed plan must enumerate all merge call sites and distinguish reviewed propagation from new code integration.
- For this workflow, code changes must be isolated in a lane/work branch until review approval. A writer editing directly on the integration branch cannot provide a meaningful before-merge gate; require migration to an isolated branch for new reviewed implementation work. Old accepted history remains historical and is not retroactively labelled reviewed.
- Reuse healthy reviewers and review independent lanes concurrently when capacity permits. Reviewers count toward `max_agents`; expose review capacity and queue wait. With a three-agent cap, plan staffing explicitly rather than silently raising it or deadlocking with all slots occupied by idle planners/workers.
- Bound corrective loops: findings produce a focused correction assignment followed by checks and re-review. Repeated unresolved findings or reviewer disagreement are surfaced to the leader/user; neither timeout nor reviewer unavailability auto-approves a merge.
- Expose review state and blocking findings in task status and the board. Record review timing separately so the added quality gate's effect on throughput is measurable.
- Preserve current human approvals and provider usage-limit rules. The user has requested review as part of the new workflow; enable it for new tasks created with this workflow. Existing running tasks need explicit adoption and review records for future pending merges; do not change them during planning or silently exempt future merges once adopted.

### 9. Share coding standards and enforce data/project boundaries

- Extract a portable baseline plus optional frontend/backend conventions from the user-selected reference files: `~/Projects/orbit-front/orbit-front/CLAUDE.md` and `~/Projects/maestro/CLAUDE.md`. These are design references, not authority to execute their setup commands or copy application code/configuration. Package the resulting rules with devAgentics so other servers do not need those absolute paths.
- Shared baseline: SOLID, Object Calisthenics, TDD, clear descriptive names, small cohesive units, explicit error handling, reuse before creating abstractions, no dead code and scoped changes. Object Calisthenics guides shallow control flow, cohesive objects and encapsulation; do not introduce meaningless wrappers or apply object-only rules mechanically to functional/UI code. Record a concrete rationale when an idiomatic project pattern requires a departure.
- Frontend conventions: components render and handle UI events; business hooks orchestrate behavior; domain services own API access through the project's established authenticated client; schemas validate boundaries and supply types; use granular state subscriptions and existing UI components. Verify framework/client behavior against the target repository's installed stack, not copied version claims.
- Backend conventions: thin entry points, cohesive business services, dependency injection through established interfaces, typed DTOs, repositories where the project uses them, explicit transaction boundaries, idempotent/retry-safe jobs and integration error handling. Reuse established integration clients and preserve sensitive-data handling.
- Security is part of implementation and independent review: validate untrusted inputs, enforce authorization and tenant boundaries server-side, avoid injection/unsafe rendering, keep secrets out of code/logs/messages, minimize privileges and dependencies, and test relevant failure/access cases. Frontend checks alone never establish authorization.
- Every specialist and reviewer receives the same versioned baseline and applicable repository conventions, pinned to the execution contract. Conflicts are surfaced before dispatch; reference-project preferences do not silently override the target project's canonical instructions or the user's explicit boundaries.
- Floor specialists may delete or replace source files and implementation artifacts within their assigned project/worktree and accepted file scope. This does not authorize deleting database records, database files, volumes, queues or other persistent data; those actions require explicit approval. Treat truncate/drop/reset/purge, destructive migration/rollback, destructive restore and overwriting that loses data as destructive actions too. Soft delete is not an automatic exception.
- Reviewers remain read-only. Specialists cannot destroy database/persistent data or change anything outside their explicitly assigned project/worktree. They submit an action request to the leader and wait. Only the leader may execute such an action after the user explicitly approves the exact action and scope. Permission is bound to actor, canonical target/project/environment, operation and arguments; it is one-use and does not pass to a worker or cover a changed action. General plan approval, tests or review approval do not grant it.
- Enforce these boundaries in runtime tools and execution isolation, including shell/subprocess routes, path/symlink escapes and filesystem/database/network access. Prompt instructions or command blacklists alone are insufficient. If the selected provider/runner cannot enforce the boundary, block that capability and explain the limitation rather than claim it is protected.
- Project-local source/artifact cleanup within the accepted scope is permitted for floor workers. Persistent-data resets, destructive migrations and cleanup outside the assigned project remain leader-only after explicit approval. Disposable isolated test data must be explicitly identified as such; never point teardown at an existing user database. Ordinary sandbox write access does not distinguish source files from database files; protected data paths and network/database capabilities require explicit runner enforcement. Keep unresolved permissions as visible blockers.
- The setup/adoption preview must enumerate any required managed storage outside the application project (task metadata, role configuration, review artifacts). Obtain explicit scoped approval before provisioning it; workers use scoped runtime APIs rather than independently modifying arbitrary home/global files. Never expand project roots or grant global write access to make an assignment pass.
- Preparing code that implements an approved product deletion feature is distinct from executing deletion on existing data. Flag its impact in the contract and review; live execution still requires the separate leader/user authorization above.

### 10. Whole-application planning and milestone releases

Before implementation, the planner interviews the user using the grilling workflow: maintain a decision tree, ask each currently answerable question in rounds with a recommendation, record answers and unresolved dependencies, and request confirmation of the shared understanding. The planner presents product/design questions directly to the user through attributed project communication; the leader remains responsible for execution permissions and approved scope. Interview answers never grant deletion or out-of-project authority.

Build a complete baseline of known application requirements before assigning implementation workers: application goal and boundaries → functional scopes (`escopos`) → backend/frontend and applicable data/integration/security workstreams → independently verifiable steps and dependencies → execution contracts. Include shared interfaces, migrations/data preservation, user flows and error states, tests, final integration, documentation and release acceptance. Mark inapplicable workstreams explicitly rather than inventing frontend/backend work for every scope.

Choose agent specialties, lane ownership and concurrency from that dependency graph after the baseline is understood. Keep unknowns visible with a planned investigation or explicit question; a long document does not make an unknown decision resolved. Do not create artificial precision for facts that depend on future discoveries.

Keep the baseline versioned and trace each scope and step to its user goal, contract revision, implementation order, review and release. Changes during implementation go through an impact preview identifying affected contracts/interfaces, dependency readiness, tests, agent assignments and release commitments. Preserve historical decisions and completed identifiers; do not silently rewrite the original plan.

Release mapping distinguishes internal work identifiers, application versions and actual release readiness. The user chose milestone releases: scope `S01`, backend step `S01-B01`, frontend step `S01-F01`, milestone `M01` mapped to application version `1.1.0`. A milestone may include multiple scopes and explicitly required steps across workstreams. Stable IDs remain unchanged on reorder, retries and review; allocating an ID does not create a deployable version. Existing legacy step-release numbers retain their historical meaning and display.

One explicit user approval of a versioned whole-application baseline authorizes its detailed execution. Do not ask again for every scope or routine correction within unchanged approved behavior. Material changes require an impact proposal and explicit approval: added/removed product behavior, changed shared/public interfaces, data lifecycle/access/security policy changes, milestone inclusion or release commitments, and meaningful resource/cost commitments outside the baseline. Uncertain classification returns to the user. Leader-managed corrections preserve tests, review and scope boundaries; separate deletion/out-of-project action approval always remains mandatory.

Milestone readiness requires accepted required implementation steps, resolved blocking review findings, satisfied cross-scope dependencies and passing integrated validation for the exact milestone artifact. Track `planned`, `in_progress`, `ready` and `released` distinctly. Marking ready does not tag, publish or deploy; released requires evidence of an explicitly authorized release action. Changes after readiness invalidate affected validation/readiness. Preserve prior released versions and never renumber them when the plan changes.

**Settled decisions:** Q1 milestone releases and separate work IDs; Q2 one approved baseline plus approval for material changes. Recorded from the user's explicit answers. The revised plan is subject to execution approval.

## Recommended delivery order

1. Capture the timing/correction baseline and define specialist contracts, planner interviews, the whole-application baseline and execution-ready item schema.
2. Add plan-readiness checks, role-aware dispatch and independent reviewer contracts so floor workers receive complete, accepted assignments.
3. Enable named profiles, evidence reuse and environment preflight using existing runner capabilities.
4. Enforce persisted review approval before merge, including stale-review checks, isolated implementation branches and correction flow.
5. Expose dependency-ready work and delay categories; validate concurrency, reviewer capacity, worker reuse and focused slices.
6. Validate milestone version mapping/readiness and compare a fixed-scope replay and a real task before claiming a speed improvement.

This is sequencing within the specification, not implementation authorization. The detailed plan follows scope approval.

## Constraints and related work

- Do not change or interrupt the currently running application team during planning.
- Do not alter files, gates or task configuration in the application repository as part of this devws feature without separate explicit scope.
- Do not remove tests, turn failures into passes, auto-accept steps, bypass independent review or file boundaries, or relax approvals for speed.
- The safe communication spec at `../2026-11-14-134415_safe-team-communication/spec.md` owns inbox delivery and draft preservation. Use its transport once available; throughput work must not introduce additional terminal injection.
- Preserve backwards compatibility with old task state, prose plans and legacy checks. Present incremental adoption rather than forcing a task restart.
- Avoid arbitrary speed promises. Report measured elapsed time, aggregate check time, repeated checks avoided, time blocked by cause, and critical-path readiness only where supported by structured data.

## Files and areas involved

| Area | Expected action |
| --- | --- |
| `agentic/src/log.rs`, `state.rs`, `task.rs` | Persist lifecycle timing, blocks and compatible structured work metadata |
| `agentic/src/check_profiles.rs` | Expose prerequisite/preflight and existing evidence-reuse behavior |
| `agentic/src/orders.rs`, `plans.rs`, `specs.rs` | Validate execution contracts, plan revisions, readiness, scope, dependencies and authoritative validation |
| `agentic/src/team.rs`, `lanes.rs`, `watch.rs` | Preserve worker reuse, ownership and safe dispatch; report stalls and block reasons |
| Review module to be scoped in the implementation plan; `agentic/src/orders.rs`, `lanes.rs` | Persist independent verdicts, findings, exact commit bindings and enforce merge guards |
| `agentic/src/status.rs`, `main.rs`, `mcp.rs`, `board*.rs` | Performance, ready-work and environment visibility |
| `agentic/src/roles.rs`, `agentic/roles/`, worker role generation | Opt-in specialist templates, role contracts, focused slices and authoritative check workflow |
| Packaged specialist standards; `agentic/src/agent_args.rs`, `mcp.rs`, runner/tool boundaries | Versioned shared/domain rules and enforceable leader-only deletion/external-mutation permissions |
| `docs/user/devagentics-execution.md`, `docs/architecture/devagentics-execution.md` | Configuration, migration, measurements and workflow documentation |
| Rust tests and appropriate integration fixtures | Deterministic timing, evidence invalidation, preflight, dependency and concurrency coverage |

## Success criteria

- [ ] A fixed fixture with two independent lanes can run both ready implementation items within capacity; a dependent item remains blocked.
- [ ] Concurrent intervals are not double-counted as elapsed delay; missing historical measurements remain unknown.
- [ ] Running the authoritative named check then reporting unchanged work executes each required gate once. HEAD/revision/configuration changes force rerun.
- [ ] A missing service blocks its required gate with an environment reason; unrelated ready work can continue and acceptance remains impossible without passing evidence.
- [ ] Scope errors are detected before dispatch when declared areas conflict; unexpected supporting changes use explicit amendments.
- [ ] Healthy workers can execute consecutive assignments without restart, preserving review and ownership checks.
- [ ] Plan changes and user-wait time are distinguished from unchanged-scope implementation time.
- [ ] Final merged-state required validation still runs and fails on integration defects.
- [ ] Legacy tasks load and continue without losing orders or silently changing their checks.
- [ ] A specialist assignment cannot dispatch with an unavailable role, unresolved contract, missing required field or incompatible lane scope.
- [ ] An execution contract references real repository patterns, explicit interfaces, tests and environment requirements; its revision is pinned to the order.
- [ ] A representative cross-lane fixture uses the same interface contract on both sides and passes final integration without corrective interface orders.
- [ ] Plan amendments invalidate only impacted readiness/evidence; unchanged unrelated work may continue.
- [ ] A fixed-scope evaluation reports first-pass acceptance and correction categories alongside elapsed time, without promising zero corrections.
- [ ] An independent reviewer can request changes, review a correction and approve an exact integration candidate without writing or merging code.
- [ ] Missing, blocked, rejected, stale, foreign or self-authored review evidence prevents every team-managed code integration path before Git changes the destination.
- [ ] Mutating source/target HEAD, order/contract revision, evidence or conflict-resolution content invalidates approval; concurrent merge attempts cannot use stale approval.
- [ ] Review capacity is visible and the three-agent fixture can progress without bypassing the reviewer gate or increasing capacity.
- [ ] Old tasks remain compatible; newly adopted reviewed workflows isolate implementation changes and gate future merges without inventing reviews for historical work.
- [ ] Specialist/reviewer contracts pin the same portable coding/security standards with target-project conventions and no dependency on the reference repositories being installed.
- [ ] Project-local scoped source deletion is permitted for floor workers; reviewer writes, destructive database operations, out-of-project writes and path escapes fail before effects through supported tool and runner routes.
- [ ] Only the leader can use an exact approved destructive/external action; denial, changed arguments, replay, worker forwarding and general plan approval do not execute it.
- [ ] Unenforceable runner capabilities and unapproved cleanup/provisioning remain explicit blockers rather than silent permission exceptions.
- [ ] Rust tests, formatting, lint and build pass; measured comparison reports its assumptions and any unchanged bottlenecks.

- [ ] Planner interview rounds, answers and unresolved questions survive restart; unresolved material decisions cannot produce an approved baseline.
- [ ] A baseline decomposes into scopes/workstreams/steps and milestones before agent allocation, with stable IDs across reorders/retries and explicit dependencies.
- [ ] Material amendments require explicit user approval and invalidate only affected contracts/readiness; routine within-scope corrections do not add user approval turns.
- [ ] A milestone becomes ready only after all required steps, reviews and exact-artifact integration gates pass; allocating a version never marks it released.
- [ ] Legacy release-number displays remain intact, and historical versions are never silently reinterpreted or renumbered.
