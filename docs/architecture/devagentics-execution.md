# Architecture: Reliable devAgentics execution

Orders retain their identity and original implementation base while authorization changes. Reports and evidence bind to that identity, so another assignment cannot accidentally receive a completion.

```text
CLI / leader MCP
       │
       ▼
Authoritative order + revision
       │
       ├── Amend → audit history → replace queued order → require delivery
       │
       └── Check/report → per-order operation lock
                              │
                              ▼
                     File/lane policy inspection
                              │
                              ▼
                     Isolated profile phases
                              │
                              ▼
                     Revalidate inputs → evidence → report for review
```

## State and compatibility

`State` adds defaulted maps for order revisions, selected profiles, check evidence and agent recovery, plus an amendment history. Old orders keep revision zero. `TaskConfig.check_profiles` maps repository names to `RepositoryChecks`; existing `Repo.check` strings retain their legacy semantics. Selected profile names are captured when an order is issued; profile definitions remain validated live configuration inputs.

Amendments update allowed globs in place, increment revision, remove current evidence and replace queued copies. They retain base, order ID, step, agent and attempts. Forbidden globs remain intact. Lane containment is enforced independently at check time; amendment glob validation accepts only provably contained extensions.

## Coordination and operation locks

`Task::transaction` remains the state mutation boundary. `control.lock` serializes dispatcher terminal effects, amendment queue replacement, configuration changes, launches, leader handover and pause/resume. The dispatcher reads state after obtaining this lock. Pause waits for an already-issued terminal action, then records paused state before interrupting anything. General state locks are never held throughout test execution.

A separate OS lock in `operations/<order>.lock` rejects concurrent checks/reports for an order and releases if its process exits. Check execution snapshots order/base/revision/HEAD/configuration and file policy. Finalization validates them again under the control and state locks. Stale evidence remains an artifact, but cannot update authoritative state or close an order. Report completion revalidates once more after the runner returns.

Terminal delivery and disk persistence are separate effects. A process crash after a terminal accepts a message but before delivery is recorded cannot offer transactional exactly-once delivery. Durable intent and queued authoritative messages support recovery; operators should inspect the pane before retrying an uncertain terminal action. Ordinary repeated pause/resume calls coalesce queued recovery and do not duplicate acknowledged deliveries.

## Check runner and evidence

`check_profiles::execute` is shared by explicit checks and reports. It checks committed changes against order and lane policies before commands. Every run receives a unique artifact directory and persists typed phase outcomes and an evidence JSON file. Agent responses omit configuration payloads and full logs.

Named phases clear the environment and restore only the documented execution baseline and explicit profile environment. Each prerequisite and test shell runs in an isolated Unix process group; timeout kills that group and reaps the direct child. Prerequisite exits are environment failures; test exits are test failures. Optional JSON counts distinguish required skipped tests and missing runner evidence without fragile output parsing. Diagnostics include a bounded, redacted output tail.

Evidence reuse requires matching revision/base/HEAD/configuration, passing results, a present evidence artifact and current clean file policy. Configuration signatures contain selected definitions and the execution baseline, with source names rather than resolved secrets. Profiles using explicit parent secret sources rerun. Legacy commands rerun with inherited environment semantics.

## Interruption and recovery

Pause persists before invoking `Devws::interrupt`. `recovery` records per-agent intent before the external action and records acknowledgement/error afterward. The shell adapter supports Claude/Codex, sends one Ctrl-C and observes done/idle hooks with a bounded wait. It never substitutes destructive `stop`.

Resume keeps the task paused while recovering gone workers and validating interrupted survivors. It replaces pending recovery messages with authoritative current orders, then enables delivery. Delivery removes the corresponding recovery entry. Busy or failed recovery keeps the task paused; successful worktrees and panes survive retries. Managed leader recovery queues a status-based continuation; the invoking pane is exempt from self-interruption. Automatic leader handover is suppressed while paused.

## MCP effect permissions

MCP tools are classified as `system`, `external` or `destructive`. System
tools manage project-local coordination, including starting, stopping and
replacing task-owned agents. External tools add another repository or publish
work to a remote service. A local worker stop is not a permission boundary;
`Team::stop_agent` resolves an ID from this task before touching its pane.
The catalog includes each tool's effect label.

The dispatcher gates external and destructive calls before invoking their
implementation. A first call persists a pending request with a generated ID,
the exact tool name, arguments and effect, then returns the invocation for the
leader to present to the user. No action runs at that stage. After explicit
user approval, `authorize_action` changes only that request to authorized.
Retrying the same tool with the request ID and unchanged arguments atomically
consumes the authorization before the action runs. A changed invocation,
denial, replay or unknown ID cannot run under that authorization. A failure
after consumption requires a new request and approval.

This gate is independent of plan approval and step review. Existing state files
load with an empty permission list. `task_status` reports requests and labels
each role with its group, repository access ceiling, sandbox and provider
approval mode. Board role details carry the same permission summary; configured
role and order controls remain the enforcement ceiling for repository writes.

## Worker availability and startup fallback

`worker::LaunchState` snapshots the role and ordered provider/model candidates
in `State::worker_launches`. The preferred program comes first, followed by
`[fallback]` and `[[alternatives]]`. Without explicit alternatives, Claude
workers get Codex `gpt-6.1-sol` at medium effort, and Codex workers get Claude's
default model at high effort. Each alternative owns its model, effort, sandbox
and approval settings; provider-specific values are never copied across programs.
Role instructions, write ceiling, group and lane remain unchanged.

```text
spawn / explicit resume
       │
       ▼
snapshot candidates → skip unavailable accounts (week, then 5-hour)
       │
       ▼
persist attempt → launch one task-owned worker pane
       │
       ├── ready → deliver the existing queue
       │
       └── explicit startup rejection → close failed pane
                          │
                          └── next unattempted candidate / unavailable
```

Account windows at or above 85% reject new workers. Account-level rejection
skips further models on that account; model-level rejection allows another
model on the same account. Candidates deduplicate by account/model. Selection
and terminal effects run under `control.lock`, with attempts persisted before
launch. Synchronous typed API errors and structured opening-turn transcript
errors share the same classification. Ordinary configuration/launch errors do
not trigger retries.

`Devws::startup_failure` reads the current pane's transcript metadata and a
runner-owned error marker. It recognizes explicit provider error envelopes,
Claude `isApiErrorMessage` records, and Codex `event_msg` error records. Codes
such as `rate_limit`, `usage_limit_reached` and `insufficient_quota` reject an
account; `model_not_found`, `model_not_available`, `model_unavailable` and
`unsupported_model` reject a model. Unknown provider errors remain actionable
failures. Ordinary chat/tool text and visible terminal content are never retry
evidence. Reads are bounded to the latest 256 KiB of complete JSONL records.
The runner reports missing executables as `provider_unavailable` and other
nonzero exits as generic `provider_exit`; a more specific transcript error wins.
It clears old evidence when reusing a pane. State hooks publish transcript
metadata before completion, so a failed opening turn cannot become ready first.

The dispatcher checks rejection only before readiness and while unpaused.
Fallback uses a fresh pane and the same logical worker ID. Queued orders,
revisions, lane ownership and completed work stay intact; a stale former-pane
error cannot restart the new candidate. When alternatives are exhausted, the
worker is marked unavailable, its closed pane no longer counts as live, and
the leader receives the reasons once. An explicit resume rechecks availability
and reconstructs open work through the existing recovery path. Explicit resume
can use corrected provider settings from the role file while retaining the
snapshotted instructions and scope; it also checks
capacity, plan approval and conflicting lane writers before reviving an
unavailable worker. Older task state loads without startup snapshots; explicit
errors in those workers are reported instead of inventing alternatives.

`task_status` exposes the actual worker `agent`, `selected_program`,
`fallback_reasons` and `unavailable` state. Preferred role settings remain
visible separately. Startup selection is independent of the leader's existing
two-program handover policy.

## Extending the runner

Add a new named profile through repository configuration. For another test framework, first provide a wrapper implementing the `DEVAGENTICS_RESULT` count contract. If a dedicated adapter is later justified, keep its evidence classification separate from subprocess execution and cover malformed/missing output. Any new input affecting check validity must participate in configuration validation or disable evidence reuse.

## Referenced files

| File | Responsibility |
| --- | --- |
| [orders.rs](../../agentic/src/orders.rs) | Exact-order validation, amendments and report finalization |
| [check_profiles.rs](../../agentic/src/check_profiles.rs) | Profile configuration, isolated phases and evidence |
| [checks.rs](../../agentic/src/checks.rs) | Git policy inspection and legacy process execution |
| [state.rs](../../agentic/src/state.rs) | Compatible execution metadata |
| [task.rs](../../agentic/src/task.rs) | State transactions, configuration and control lock |
| [recovery.rs](../../agentic/src/recovery.rs) | Durable interrupt/resume intent |
| [worker.rs](../../agentic/src/worker.rs) | Candidate selection, durable startup attempts and provider error evidence |
| [devws.rs](../../agentic/src/devws.rs) | Startup evidence and terminal adapter |
| [team.rs](../../agentic/src/team.rs) | Pause/resume and lost-pane restart |
| [watch.rs](../../agentic/src/watch.rs) | Paused delivery and recovery acknowledgement |
| [agent_cli.sh](../../config/tmuxinator/agent_cli.sh) | Non-destructive terminal interruption |
| [mcp.rs](../../agentic/src/mcp.rs) | Tool effect catalog and one-use approval gate |
| [status.rs](../../agentic/src/status.rs) | Role permission summaries and pending requests |
| [board.rs](../../agentic/src/board.rs) | Team board role permission summaries |
| [integration.rs](../../agentic/src/integration.rs) | Merge candidates, candidate checks and their evidence binding |
| [bench.rs](../../agentic/src/bench.rs) | Run measurement and comparison |

See [user commands and configuration](../user/devagentics-execution.md).

## Task-scoped worker percentage consent

Worker admission uses a local 85% guard independently from the leader's 95%
handover policy. `LaunchState.percentage_blocks` records account/window evidence
separately from `blocked_accounts`, which represents actual provider rejection.
When two compatible accounts are percentage-blocked and no candidate starts,
`UsageLimitRequired` causes `Team::launch_worker` to persist a pending consent
request. The initially reserved worker remains unavailable with its identity and
scope, allowing resume rather than duplicate replacement.

`State.usage_limit_approval` defaults to `not_requested` for old tasks and supports
`pending`, `approved` and `denied`. `set_usage_limit_approval` records the user's
explicit decision under the control lock and a state transaction, logging it as
a review. It is a project-local tool, separate from exact-call external grants
and plan approval. Denial/revocation suppresses repeated prompts; explicit user
reconsideration can approve the existing request. Consent does not cross tasks
or enter repository history exports.

Every selection reads current consent. Approved selection bypasses only the
percentage guard; actual provider failures retain bounded fallback semantics.
Explicit resume rebuilds candidate selection with the existing worker scope,
rechecking plan approval, capacity and writer ownership. Revocation changes
future launches, not live workers. `task_status.usage_limit_approval` exposes the
state, evidence and next action to a restarted leader.

## Reviewed execution boundary

`execution` stores versioned baseline proposals separately from approved state.
Decision prerequisites define the interview frontier. Contracts carry specialist,
lane, file/interface/test definitions and pinned check configuration. Approval
is revision-bound; material amendments invalidate affected work and evidence.
Stable work IDs are independent of milestone application versions. Old task state
still defaults to legacy mode and is not automatically adopted.

Reviewed coordinators have only the leader MCP catalog. Workers run as managed,
noninteractive Claude turns with native tools and other MCP configurations
disabled. The worker MCP server binds identity to a private runtime session and
exposes no leader approval, external-action or merge capabilities. Healthy turns
resume the same conversation; uncertain turns retain durable intent and require
explicit recovery. Unsupported provider adapters fail closed. This application
boundary does not claim isolation from hostile same-user processes controlling
runtime files or a compromised Docker daemon.

`runner` exports tracked source into disposable generations, excluding protected
data, credentials and Git metadata. Only these generations enter non-networked,
non-root containers with a read-only root filesystem, dropped capabilities,
resource limits and no host socket/home mounts. Source allowlists and protected
paths are explicit configuration; do not classify live data directories as source.
A dedicated per-phase result directory carries structured test counts, without
exposing the surrounding task metadata. Checkpoint import validates scope,
source identity, links and executable modes, then commits only the permitted paths.
Git hooks, filters and fsmonitor callbacks cannot execute source-controlled code
during reviewed operations.

Named profile prerequisites run before dispatch in an independent generation.
Unchanged environment failures are cached until relevant inputs change or an
operator requests a retry. Reviewed validation never reads parent secret sources
or falls back to inherited host-shell checks. Evidence includes the immutable
runner configuration; changed source, order, baseline checks or environment
invalidates reuse. Final milestone checks validate the integrated application.

`reviews` binds independent identities and authoritative evidence to immutable
source/target commits and an `integration` candidate tree/commit. Reviewers see a
dedicated candidate checkout exported read-only to their tool container. Acceptance
revalidates inputs under the control lock, journals intent, then fast-forwards to
the approved candidate. Recovery acknowledges only that candidate, optionally
followed by metadata-only history; unrelated changes require operator inspection.
Lane catch-up propagates accepted code by fast-forward and blocks divergent
integration instead of creating unreviewed resolution code.

`milestones` stores combined-state evidence against code fingerprints, required
contracts, check definitions and runner configuration. History-only commits do
not change that code fingerprint. Missing/stale/failed evidence blocks readiness
and draft PR publication. This feature does not add release/tag/deployment effects.
Exports preserve approved planning and accepted review provenance, not worker
credentials, runner policy or machine-specific environment evidence.

`performance` incrementally reads bounded log data and records actual check,
queue and worker-turn intervals. Aggregate phase time is distinguished from the
union of elapsed intervals, and unavailable historical timing remains unknown.
`Task::state` takes a shared read lock without rewriting the state file; source
fingerprints use in-process SHA-256 rather than per-file Git processes. Readiness,
capacity and review details are available through status and the leader's board
dialog. No automatic staffing change or fixed speed claim follows from a snapshot.

## Delivery gates

Four gates keep a team from finishing tasks without a working system, and one
measurement shows whether they help.

- **Attempt budget.** `TaskConfig.max_attempts` (default 3) bounds a step's
  orders and its reviews asking for changes. `send_order` and `review_request`
  refuse past it; `State.attempt_limits` holds per-step raises, written only by
  the user's CLI command, never by an MCP tool.
- **Protected tests.** A contract's `protected_tests` are appended to every
  implementation order's `forbidden` globs (`Contract::forbidden_for_order`), so
  the existing file-policy, checkpoint-import and review-evidence checks refuse
  changes to them. A `writes_tests` item writes them; validation requires the
  implementation to depend on it, and the runner to expose the tests as source.
- **Candidate checks.** `RepositoryChecks.candidate` names profiles run on each
  merge candidate before its review. `integration::check_candidate` checks the
  candidate commit out detached, prepares a runner snapshot under a synthetic
  worker identity and runs the profiles outside the control lock. Evidence is
  keyed by candidate tree, because `commit-tree` stamps a time and the commit id
  changes on every computation. `review_request` re-derives the candidate under
  the lock, requires the same source, target and tree, and stores the evidence
  hash in the review. `reviews::validate` (merge and acceptance) and
  `integration::recover` require passing evidence for the exact candidate and
  the current candidate configuration. A failing candidate fails the step and
  tells the leader. Legacy tasks have no runner and skip the gate.
- **Codex reviewers.** `team::reviewed_adapter` admits Codex only for the
  read-only `code-reviewer` role of a task whose user set
  `State.codex_reviewers`; a refusal is a model failure, so startup falls back
  to Claude. `worker_tools::codex_turn` runs `codex exec --json` with
  `--ignore-user-config --ignore-rules`, `sandbox_mode="read-only"` and
  `approval_policy="never"` as config (resume takes no sandbox flag), the worker
  MCP server as its only server, in the reviewer's read-only snapshot. The
  thread id from the first JSON event is kept in `State.worker_sessions`. The
  boundary is weaker than Claude's: the shell cannot write but can read host
  files, which status and docs state.
- **Measurement.** `bench::build` derives a report from state and log alone:
  first-pass acceptance (one order, no review asking changes), corrections by
  `Category` (required on blocking findings; failed candidate checks count as
  integration defects), timing aggregates and their overlap-free union, user
  interventions and usage. `review_wait` intervals come from the review's
  request and submit times.
