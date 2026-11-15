# Plan: Specialist execution, independent review and measurable throughput

- **Date:** 14-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** In progress — execution approved
- **Authorization:** User approved [spec.md](spec.md), including specialist floor agents, detailed execution contracts and independent review before merges. Implementation and the container-backed tool runner were explicitly authorized.
- **Additional direction:** Reuse frontend/backend reference-project rules with SOLID, Object Calisthenics, security and data preservation. Floor workers may delete scoped project source/artifacts. Database/persistent-data destruction and mutation outside the assigned project are leader-only after explicit scoped user approval.
- **Settled planning direction:** Planner-led grilling with the user; a whole-application baseline split into scopes/workstreams/steps before agent allocation; milestone application releases with separate stable work IDs; one baseline approval plus explicit approval for material changes.

## Scope and delivery decisions

Implement incrementally in devws. Do not modify, pause or reconfigure the running application team. No deployment, commit, push, application-repository migration or remote branch-protection changes are implied.

Use the existing task state, roles, named-check runner and lane worktrees. Add narrowly scoped modules for execution contracts, reviews and performance aggregation. Keep leader acceptance and user approval; do not add autonomous dispatch or automatic acceptance.

Introduce a versioned `reviewed` workflow for new tasks, with a serialized `legacy` default when reading old configuration. New-task initialization selects reviewed mode and provisions the starter templates; finding an existing task must not change its mode. Existing tasks adopt it only through an explicit preview/apply operation while paused, with pending implementation isolated into work branches. There is no automatic downgrade from reviewed mode.

All reviewer results are recorded through a role-scoped runtime interface that derives the reviewer from its registered invocation context. Do not expose the leader's complete MCP catalog to workers or trust an `agent_id` argument as proof of identity. This enforces the application API boundary; it does not claim to isolate hostile processes running as the same OS user with unrestricted task-file access.

Database-data and project-boundary enforcement is a prerequisite to launching the new specialists, not a prompt-only convention. Inventory provider and subprocess capabilities first; block any unsupported destructive/external route. Floor workers may delete scoped project source files; explicit user approval is required for leader-managed persistent-data destruction, cleanup outside the project and managed storage provisioning outside the assigned project. Do not assume writable sandboxes prohibit unlink/delete or provide database isolation.

Safe inbox/activation transport remains owned by the separate [communication spec](../2026-11-14-134415_safe-team-communication/spec.md). New features use structured CLI/MCP state and do not add terminal injection. If its scoped worker transport is already implemented, reuse it; otherwise provide the minimal scoped CLI/runtime context required here without implementing a second messaging system. Completion of this throughput plan alone does not fix existing prompt collisions.

## Step 1 — Compatible workflow models and measurement baseline

**Files:** `agentic/src/state.rs`, `task.rs`, `log.rs`, new `performance.rs`, `main.rs`.

**Actions:**

- Add defaulted/versioned workflow, planning/baseline/milestone references, contract references, review references and lifecycle events. Preserve old state/config deserialization and existing repository history behavior; imported accepted history must not imply review approval.
- Define typed events for assignment readiness, dispatch/delivery, checking, review requests/results, environment blocks, capacity wait, merge/history activity and scope revisions. Include task/order/revision IDs and timestamps; record monotonic durations where measured in one process.
- Implement bounded incremental aggregation from existing logs plus new events, with unknown historical intervals explicitly marked. Separate aggregate worker/check seconds from the union of elapsed intervals. Do not infer exact coding duration from idle/working pane snapshots.
- Capture a read-only baseline fixture from the observed task using aggregate counts and synthetic/sanitized events, without copying application code, prompts, tokens or environment values into tests.

**Verification:** Compatibility fixtures load; deterministic overlapping intervals are counted correctly; user wait/scope changes are separated; repeated summary calls do not rescan all prior logs. Tests use an injected clock, not sleeps.

## Step 2 — Specialist and reviewer contracts

**Files:** `agentic/src/roles.rs`, `agent_args.rs`, `start.rs`, `team.rs`, `agentic/roles/samples/`, role-loading tests.

**Actions:**

- Add backward-compatible role capabilities for implementation specialties and independent review. Reuse repository/home overrides and current group/provider rules.
- Supply starter templates for backend/data, API/integration, frontend/UI, docs and a read-only code reviewer. Keep stack-specific facts in repository overrides and execution contracts, rather than assuming every project uses the same stack.
- Include responsibilities, required inputs, conventions, output evidence, boundaries and escalation conditions. A reviewer is a read-only senior capability; it is not a floor writer and cannot perform leader mutations.
- Package shared standards plus frontend/backend guidance, based on read-only inspection of `~/Projects/orbit-front/orbit-front/CLAUDE.md` and `~/Projects/maestro/CLAUDE.md`. Preserve source provenance in maintainers' docs; runtime rules must work without those repositories. Do not copy secrets, business-specific code or setup commands.
- Shared rules cover SOLID, Object Calisthenics applied idiomatically, TDD, descriptive names, explicit errors, reuse, cohesive units and scoped changes. Frontend guidance covers component/hook/service/client separation, validated schemas and selective subscriptions. Backend guidance covers interfaces/DI, services, DTOs, repository conventions, transactions and idempotent integrations/jobs. Security covers validation, authorization/tenancy, injection, secrets and least privilege.
- Give reviewers the same rule bundle and require actionable findings linked to rules and observable defects. Target-project canonical conventions and explicit user constraints remain authoritative; conflicts block readiness rather than silently generating architecture churn.
- Implement actor-scoped leader-only action authorization for destructive effects and out-of-project mutations. Bind canonical target, project/environment, operation/arguments and one-use decision; workers only request, never consume leader grants. Extend existing permission mechanisms where suitable without conflating action approval with plan, review or percentage consent.
- Audit raw shell, subprocess, filesystem, database and network routes. Enforce assigned-root containment and destructive-operation restrictions with supported execution isolation/tool mediation. Missing enforcement blocks the capability; do not substitute an unsafe command blacklist. Include symlink/path traversal, protected database files and existing persistent-data overwrites in the threat model; distinguish permitted scoped source deletion.
- Provision the catalog for new reviewed workflows without overwriting custom roles. Spawn only selected roles for ready work; role availability is distinct from live staffing.
- Bind role-scoped runtime invocation context during launch/restart. Reviewer submissions derive identity from that context and validate task membership, capability and assignment; tests cover identity spoofing and stale contexts.
- Keep existing model restrictions, fallback, plan approval, capacity, lane ownership and usage consent checks.

**Verification:** Role parsing/overlay tests, launch arguments for both providers, immutable reviewer checkout permissions, rejection of reviewer mutation APIs, and provider-fallback preservation of specialist instructions. Test permitted scoped source deletion, and add negative tests for persistent-data/record deletion, purge/drop/reset, outside-root writes, symlink escape, shell/subprocess bypass, worker consumption of a leader grant, denial/replay/changed arguments, and unsupported runner capability. Tests use disposable authorized fixtures and must not delete existing user data. Rule bundles work with reference repositories absent.

## Step 3 — Planner interview and whole-application baseline

**Files:** `agentic/src/plans.rs`, new `execution.rs`, `state.rs`, `questions.rs`, `main.rs`, `mcp.rs`, planner/leader role guidance.

**Actions:**

- Persist an application planning session: goals/non-goals, decision tree, round/question IDs, recommendations, answers, assumptions, unresolved prerequisites and baseline revision. Resume the same interview after leader/planner restart rather than repeating settled questions.
- Give the planner attributed product/design questions and a scoped answer-reading interface. Reuse the eventual inbox transport; do not type into user drafts or expose leader permissions. Until safe attributed transport is available, let the leader relay identified planner questions/answers through existing user interaction. Do not block user clarification on permission to mutate code.
- Ask currently answerable design questions in rounds and investigate repository facts rather than asking the user to supply them. Record explicit confirmation of shared understanding before offering baseline approval.
- Build the complete known-application baseline: functional scopes, applicable backend/frontend/data/integration/security workstreams, stable step IDs, shared interfaces, dependencies, tests, reviews, migration/data-preservation concerns and milestone acceptance. Mark unknowns as unresolved or approved investigations; never invent precise decisions.
- Allocate specialties, lane ownership and agent slots from the approved dependency graph, not before it. Do not open writers while the baseline is unapproved or relevant requirements are unresolved.
- Bind explicit user baseline approval to its revision/content. Separate it from permission to delete data, change outside-project targets, publish, or exceed usage limits. Accepted senior contracts can refine the baseline without adding per-scope user approval when requirements are unchanged.
- Add material-amendment preview/decision with old/new baseline diff, affected interfaces/data/security, dependent steps, checks, reviews, milestone commitments and resource changes. Explicit user approval is required before impacted work uses the revised baseline. Routine corrections stay within the existing approved contract; uncertain classification escalates.

**Verification:** Restart preserves interview context and avoids repeat questions; unapproved/stale baseline cannot dispatch writers; planner cannot record user approval on its own; approval of an old revision cannot approve a changed one. Material changes wait for approval while unrelated work may continue. Deletion/external permissions remain ungranted. A representative baseline covers the whole known app before assignment.

## Step 4 — Versioned execution contracts and readiness validation

**Files:** new `agentic/src/execution.rs`, `plans.rs`, `orders.rs`, `state.rs`, `main.rs`, `mcp.rs`.

**Actions:**

- Store a structured manifest alongside the existing senior plan Markdown: item ID, plan revision, specialist, lane, dependencies, allowed/supporting files, reusable code references, interfaces, behavior/errors, tests, environment/check profiles, acceptance criteria, non-goals and unresolved questions.
- Validate schema, duplicate/unknown IDs, dependency cycles, lane containment, role capability and check availability. Retain semantic senior/leader assessment; nonempty text alone is not readiness.
- Pin accepted contract revision to each order. Require dependency acceptance and accepted contract readiness for reviewed-workflow floor assignments.
- Pin the coding-rule bundle and target-project conventions; enumerate data effects, cleanup, external targets and any required leader action grants. A destructive migration command or write to global configuration cannot hide inside an ordinary check profile or prerequisite.
- Validate known supporting file areas before Git catch-up or other dispatch side effects. Unexpected additions still require the existing explicit amendment path; keep final actual-diff checks.
- Add contract amendment/impact preview: invalidate affected readiness, checks and review requests; mark dependent work for revalidation and block its acceptance if already in flight. Leave unrelated contracts unaffected. Do not interrupt running turns automatically.

**Verification:** Table-driven malformed/valid manifests, unresolved decisions, foreign roles/lanes, cycles, stale plan revision, dependency completion, amendment impact and old prose-plan compatibility. Assert rejected dispatch leaves refs and queues unchanged.

## Step 5 — Named checks, evidence reuse and environment preflight

**Files:** `agentic/src/check_profiles.rs`, `orders.rs`, `execution.rs`, `main.rs`, `mcp.rs`, check/profile fixtures.

**Actions:**

- Expose why a check ran or reused evidence using the existing exact-order cache. Do not add cross-order caching or relax restrictions for parent-secret sources.
- Add explicit preflight using existing prerequisite definitions. Bind results to the relevant execution context, profile/configuration identity and freshness; leader preflight never proves a worker sandbox works.
- Make legacy-check migration a previewable configuration operation. Do not attempt to split arbitrary shell syntax automatically; present explicit named profiles and retained required gates for review before application.
- Guide workers to run `devagentics check run` for authoritative final validation, then report with eligible evidence reuse. Keep targeted TDD and every mandatory full test/lint/type/build gate.
- Persist environment blocks and avoid unchanged automatic retries. Report a compatible authorized runner if one is configured; otherwise show the missing capability. No implicit privileged runner or prose-only evidence import.

**Verification:** Count command invocations: check then unchanged report runs the gate once; changed HEAD/revision/configuration/worktree, missing artifact or restricted environment reruns/rejects. Missing services remain outstanding gates; required skipped tests cannot pass. Concurrent check/report operations remain serialized per order while independent checks can overlap.

## Step 6 — Immutable review requests and authenticated verdicts

**Files:** new `agentic/src/reviews.rs`, `state.rs`, `orders.rs`, `main.rs`, `mcp.rs`, `start.rs`.

**Actions:**

- Add leader operations to request/read reviews and scoped reviewer operations to inspect assignments, submit findings and return `approved`, `changes_requested` or `blocked`. Keep reviewer assignments separate from implementation steps so review does not itself require another reviewer.
- Persist request ID, author/reviewer identity, order/contract revision, source/base/target commits, diff/integration identity, required evidence identity and findings. Capture immutable code artifacts or a dedicated read-only checkout for review; never let an actively edited worktree be the only reviewed source.
- Findings include file/line, severity, blocking status and rationale. Fixes require authoritative checks and reviewer confirmation; nonblocking suggestions do not create correction loops. Preserve dismissed findings and their reasons.
- Validate distinct reviewer/author identities, capabilities, current assignment and exact evidence binding on submission. Reviewer silence, provider failure and elapsed time remain blocked states.
- Reuse reviewer processes across compatible assignments. Queue reviews when capacity is full and expose the capacity reason rather than bypassing the gate.

**Verification:** Foreign/self-authored/spoofed/stale submissions fail; valid findings and corrected re-review persist across restart; reviewer APIs cannot mutate code, approve plans, accept steps, widen scope or merge. An unchanged reviewed artifact remains inspectable after the worker proceeds elsewhere.

## Step 7 — Enforce review at every managed integration boundary

**Files:** `agentic/src/lanes.rs`, `orders.rs`, `reviews.rs`, `task.rs`, merge integration tests.

**Actions:**

- Enumerate the current merge paths: `orders::accept_step` → `lanes::merge_into_task`, and `orders::send_order` → `lanes::catch_up` → private merge helper. Refactor helpers so no reviewed integration call can omit a validated authorization object.
- Prepare the exact integration candidate in an isolated temporary worktree without changing the task/lane branch. Bind source/target parents, candidate tree and required validation identity to the review request. Conflicts block preparation; conflict-resolution code is a new candidate requiring review.
- At acceptance, hold the task control boundary and relevant repository mutation lock, revalidate current order/contract/evidence, reviewer approval and actual refs. Integrate only the approved candidate with expected-target checks; a moved source/target returns stale review before mutation. Test the available Git mechanism rather than relying only on process-local state locks.
- Journal prepared/approved/integrating/merged/accepted state so a crash between Git effects and task persistence can be reconciled idempotently. Recovery must recognize the exact candidate and never treat arbitrary branch advancement as an approved merge.
- For catch-up, permit verified fast-forward propagation of already reviewed accepted history. Divergence requires preparing/reviewing the integration candidate before updating the lane; do not create an unreviewed automatic merge commit. Plan catch-up between assignments and account for evidence invalidation.
- Require isolated work branches for reviewed implementation. Block direct integration-branch writer assignments and give existing-task adoption an explicit migration preview; no automatic copying of dirty trees.
- Preserve synchronous history saving after acceptance; record its time and do not confuse history commits with reviewed code changes. Target ref movement from history still requires fresh commit binding for later pending reviews.

**Verification:** Both CLI/MCP acceptance and catch-up tests assert unchanged refs for missing/rejected/blocked/foreign/stale reviews. Cover source/target races, two simultaneous accepts, conflicts, dirty worktrees, branch switching, reviewed propagation and crash recovery. Ensure exactly one accepted integration and no accidental rollback of unrelated work.

## Step 8 — Milestone versions, integration validation and publication boundary

**Files:** `agentic/src/specs.rs`, `execution.rs`, `state.rs`, `history.rs`, `check_profiles.rs`, `reviews.rs`, `prs.rs`, `main.rs`, `mcp.rs`, end-to-end fixtures.

**Actions:**

- Extend `specs.rs` and persisted planning/history state with stable scope/work item IDs (`S01`, `S01-B01`, `S01-F01`) and milestones (`M01` → application version `1.1.0`). Reserve versions without tying numbering to step order or starting agents. Validate unique IDs/versions and milestone dependency cycles; never renumber reordered, retried or released work.
- Keep new milestone identity separate from legacy `spec`/step patch numbers. Old tasks/history retain their original displays; explicitly migrate future planning metadata and expose the distinction in status/board. No automatic reinterpretation of imported versions.
- Persist milestone requirements and states `planned`, `in_progress`, `ready`, `released`. A milestone can span scopes; only its required accepted steps, resolved independent review findings, satisfied milestone dependencies and exact-artifact integration evidence permit ready status. A partial scope cannot make it ready accidentally.
- Mark released only from recorded evidence of a separately authorized release operation. This feature must not create a release/tag/deploy command or infer release from acceptance, version reservation, a push or draft PR.
- Run the required combined-state integration checks after lane merges in an isolated checkout of the exact code revision. Persist repository/profile/environment-bound evidence distinct from order evidence; do not fabricate an open order for this gate.
- Block reviewed-workflow completion/publication readiness while required integration evidence is missing, stale or failed. Code changes invalidate it. Treat metadata-only history changes explicitly with a validated code-tree identity rather than silently treating a different HEAD as checked.
- Keep `open_prs` an external action requiring its existing exact-call permission; it pushes and creates draft PRs, never merges them. Report that remote/manual merges require separately configured host protection.
- Any future managed final-merge API must consume the same review authorization and validation guards; do not add such an API in this implementation.

**Verification:** Reordering/retries do not change work IDs or application versions; legacy history is unchanged. Cross-scope milestone requirements gate readiness; changed material commitments require baseline amendment approval; readiness never implies release. Individually passing lane changes that fail together cannot complete the task. Stale final evidence blocks readiness. Existing external permission replay/denial tests still pass, and legacy PR behavior remains compatible.

## Step 9 — Ready-work view and capacity-aware leader workflow

**Files:** `agentic/src/execution.rs`, `performance.rs`, `status.rs`, `board.rs`, `board_view.rs`, `board_tui.rs`, `watch.rs`, `agentic/roles/leader.md`, specialist templates.

**Actions:**

- Expose ready items and explicit dependency, environment, specialist, lane, review and capacity blockers through status/CLI/MCP and concise board details.
- Show reviewer queue time and ready-but-unassigned work. Count idle capacity only when a concrete compatible item is dispatchable; account for planner/reviewer slots.
- Specify staffing for a three-agent fixture: preserve a writer and reviewer opportunity, release/reassign an unneeded planner only through deliberate leader action, and reuse healthy workers. Never auto-raise capacity or shut down an agent with unresolved responsibilities.
- Update guidance to use focused implementation slices, authoritative checks, plan/interface contracts and independent review. Record clarifications without creating development steps merely to acknowledge them.
- Add timing around catch-up, review, checks, acceptance/history and control-lock waits without claiming all tests run under the global lock. Use incremental metrics and structured sources; do not add transcript scraping or pane capture.

**Verification:** Two independent lanes can make progress within the configured slots; a dependent item stays blocked. A three-agent writer/planner/reviewer scenario progresses without skipping review. Paused tasks do not dispatch; busy agents retain work. Board tests cover missing/unknown timings and bounded output.

## Step 10 — Adoption, documentation and measured comparison

**Files:** `agentic/src/main.rs`, `start.rs`, `execution.rs`, user/architecture docs, `agentic/docs/usage.md`, this spec and plan; appropriate Rust and shell integration tests.

**Actions:**

- Implement explicit preview/apply adoption while paused: list missing roles/contracts/profiles, active orders, direct-branch writers, unreviewed pending merges and required operator actions. Apply only once prerequisites are met; preserve accepted legacy history without inventing review verdicts.
- Include exact managed storage/provisioning targets and cleanup/deletion policy in setup/adoption previews. Ask for explicit leader-only scoped authorization before those effects; do not create blanket permission to mutate other projects. Document the difference between generating deletion-feature code and executing deletion on existing data.
- Document setup, specialist customization, execution-contract examples, independent review/corrections, preflight, evidence reuse, final validation, capacity planning, measurements and safe-inbox limitations using the applicable documentation skills.
- Run a deterministic fixed-scope replay comparing legacy-style duplicate checks with the authoritative path. Report command count, first-pass acceptance, correction causes, review queue time and overlapping elapsed intervals; do not use brittle wall-clock speed assertions.
- Validate an isolated real-workflow fixture with a contract/interface shared across two lanes, a missing-service block, a reviewer-requested correction and final integrated validation. A real application pilot is optional and requires the user's explicit selection/adoption; do not touch the current application team automatically.
- Update plan/spec status and verification results. Record any remaining bottlenecks; do not promise a particular speed-up.

**Verification:** Old and new task fixtures load; preview is read-only and invalid adoption makes no changes; adopted future merges cannot bypass review. Examples and relative doc links match implemented commands.

## Final verification

From `agentic/`:

1. `cargo fmt --check`
2. `cargo test` — unit, race/recovery and end-to-end workflow coverage.
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo build`

Run the isolated tmux/shell regression tests relevant to changed launch/configuration surfaces: `bash scripts/test_agentic.sh`, `bash scripts/test_agentic_cross.sh`, and `bash scripts/test_agent_cli.sh` when their interfaces are affected. Use isolated fixtures; never inject test messages into the user's team panes. If sandbox process/network restrictions block a necessary check, request the specific required escalation rather than weakening the check.

Review the final diff against scope, role permissions, merge call sites, compatibility and preservation of existing work. No commits or pushes unless requested.

## Execution tracking

- [ ] Step 1 — Compatible state and baseline
- [ ] Step 2 — Specialist/reviewer roles and runtime identity
- [ ] Step 3 — Planner interview and application baseline
- [ ] Step 4 — Execution contracts and readiness
- [ ] Step 5 — Checks, reuse and preflight
- [ ] Step 6 — Review requests and verdicts
- [ ] Step 7 — Merge guards and recovery
- [ ] Step 8 — Milestone versions and final integration gate
- [ ] Step 9 — Ready-work/capacity views and guidance
- [ ] Step 10 — Adoption, docs and evaluation
- [ ] Final verification

## Execution notes

- Execution approved by the user. Existing application task is untouched.
- Initial provider capability inspection: source deletion is permitted by the user clarification. Arbitrary shell/network access still does not enforce protected database data and project boundaries; unsupported reviewed runner capabilities remain blocked pending a verified isolation route.

### Earlier implementation checkpoint (superseded below)

- Added compatible application/proposal, decision, contract, milestone and review records plus bounded timing aggregation and check interval instrumentation.
- Added portable specialist standards/templates and non-overwriting provisioning; new CLI tasks select reviewed mode while existing tasks preserve legacy behavior.
- Added shared CLI/MCP planning operations, exact-revision confirmation/approval, unanswered decision frontier, adoption preview and review submission identity checks.
- Reviewed launch, resume, assignment, lane catch-up/merge and publication fail closed because raw provider runners do not enforce protected database/project boundaries. Legacy tasks are untouched. No unsupported guarantee is presented as implemented isolation.
- Added regression coverage for baseline approval, stale decisions, role preservation, foreign/missing reviews, legacy compatibility, overlapping timings and blocked adoption with unchanged refs/configuration.
- Still outstanding: verified mediated runner and worker identity transport, actual role-aware dispatch, integration candidate/journal, prerequisite execution in protected contexts, milestone final evidence/readiness and full board/capacity workflow. The ten-step plan remains in progress; no completed-feature claim or execution-status promotion.
- Asked the user whether to add a container-backed tool runner with only project source mounted and protected databases/host network unavailable. No container/image/daemon changes made while awaiting that decision.

### Verification checkpoint (15-11-2026)

- Rust suite outside sandbox, serial execution: 215 unit tests and 8 integration tests passed. A parallel run encountered an order-lock contention; serial verification passed without weakening the lock.
- `cargo fmt --check`, `cargo clippy --all-targets --offline -- -D warnings`, and `cargo build --offline` passed.
- `scripts/test_agentic.sh` and `scripts/test_agent_cli.sh` passed using isolated tmux servers outside the sandbox.
- Cross-runtime test passed after correcting its stale report invocation (`--order o1`) and waiting for the persisted delivery receipt. All three isolated shell suites passed.
- Nothing committed, pushed, deployed or applied to the running application team. The container-runner choice is still pending; remaining dependent work is not marked complete.

- Container tool-runner amendment approved. Implement disposable source snapshots with bounded non-networked/nonprivileged execution and validated host checkpoints. Initially enable only the verified Claude MCP-only adapter; unsupported provider routes fail closed and may use compatible fallback. No live application adoption or deployment implied.


### Verified delivery checkpoint (15-11-2026)

The approved container amendment is implemented. New reviewed tasks have scoped Claude MCP-only workers, isolated source snapshots, contract-bound assignment and authoritative checks, independent candidate-bound reviews, guarded merges with recovery, and milestone integration evidence. Legacy tasks retain their workflow. Baseline proposals/interview decisions, material amendments, portable specialist standards, explicit adoption, inbox acknowledgement and capacity/readiness views are implemented.

Recovery now preserves pending reviewer work and restores its exact immutable candidate checkout. Concurrent milestone validations use separate source generations. No running application team was adopted or reconfigured; no provider account call, image pull, release, commit or push was performed.

Verification:

- 225 Rust unit tests and 8 integration tests passed; the two opt-in Docker tests passed separately against the existing local image.
- All three isolated shell suites passed: `test_agent_cli.sh`, `test_agentic.sh`, `test_agentic_cross.sh`.
- Formatting, Clippy with warnings denied, build and diff whitespace checks passed.
- The real Docker fixture covers scoped source deletion, protected data exclusion, denied reviewer writes/self-approval, unchanged-check reuse, moved-target rejection, changes-requested verdicts, refreshed approval, exact merge, idempotent recovery and final milestone validation. Provider turns are not billed or exercised by this fixture.

The full plan remains in progress for evaluation rather than implementation expansion: a representative two-writer shared-interface replay, the full three-agent staffing scenario, and the fixed-scope legacy-versus-reviewed comparison have not been demonstrated. No measured application speed-up or complete coverage of every race scenario is claimed. Existing timing reports cover recorded intervals and preserve unknown historical timings; they do not establish exact coding time or a critical path. Keep these acceptance items open rather than presenting the whole ten-step plan as executed.

Operational limitations: reviewed mode currently supports only the verified Claude adapter; unsupported provider routes remain blocked. A suitable local toolchain image and explicit source/protected roots are required. Existing tasks need explicit adoption. Divergent lane catch-up is blocked pending reviewed conflict resolution. Version readiness never performs a release or deployment.
