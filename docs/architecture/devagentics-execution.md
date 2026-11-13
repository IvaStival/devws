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

See [user commands and configuration](../user/devagentics-execution.md).
