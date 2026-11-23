# Measuring an agent team

`devagentics bench` measures a finished team run, so a change to how the team
works (the reviewed workflow, a new gate, another model) is judged by what it
does to delivery, not by how a run felt. One run proves nothing: agents vary
from run to run. Compare many runs of the same fixed scope.

## A run

1. Prepare a fresh copy of a fixture. `notes` is a small two-lane app (an HTTP
   API and its client sharing one contract) with acceptance tests that fail
   until the team builds it:

   ```sh
   scripts/bench_team.sh notes reviewed     # or: notes legacy
   ```

   `bugfix-one-file` is a one-line bug in one file, with acceptance tests
   that fail until it is fixed: risk routing should take it as light (no
   planner and no reviewer agent). It prints the folder, the commands to start the team and the check. It
   starts no agent itself: the team runs on your accounts.
2. Start the team with the printed commands and give the leader the folder's
   `GOAL.md` unchanged. Answer its questions the same way on every run.
3. When the team has stopped, save the report:

   ```sh
   devagentics bench report <task> --json > runs/notes-reviewed-1.json
   devagentics bench compare <legacy task> <reviewed task>
   ```

## What it reports

| Measure | Meaning |
| --- | --- |
| accepted steps, first pass | Steps accepted with one order and no review asking for changes, and their share. |
| corrective orders | Orders after a step's first. |
| reviews (asking changes) | Submitted reviews, and those that sent work back. |
| corrections by category | Blocking review findings by cause (`requirement_ambiguity`, `stale_plan_interface`, `scope_omission`, `environment_mismatch`, `implementation_defect`, `integration_defect`, `test_weakening`), plus failed candidate checks as integration defects. |
| busy | Recorded seconds per kind of work (checks, candidate checks, worker turns, queue and review waits), summed over agents, so they overlap. |
| recorded elapsed | The same intervals with overlaps counted once. A task from before intervals were kept shows a lower bound. |
| user | Escalations, approvals, permission requests and extra attempts you allowed. |
| milestones ready | Milestones whose integrated validation passes for the current code. |
| tokens, cost | The team's total, at API prices. |
| integrated, cost per integrated success | Whether every milestone is ready, and the run's cost when it is and every agent was priced (unknown otherwise; sum a failed run's cost into the ones that integrated). |
| cost by role | Each role's cost and share: leader, planner, reviewer, writers. |
| review rounds per reviewed step | Independent reviews per step that had one. |
| light acceptances, items by rigor | Light items accepted on the leader's reason without a reviewer, and how the items were routed. |
| escaped defects | Work failures recorded on a step after it was first accepted: a lower bound. |
| leader MCP calls per accepted step | From payload telemetry; each call rereads the leader's context, so this drives its cost. CLI reads are not counted yet. |

## Protocol

- Run each way of working at least ten times on the same fixture, from a fresh
  copy each time, with the same models and answers.
- Compare the distributions (median and spread of first-pass share,
  corrections, elapsed time, user interventions and cost), not single runs.
- A run counts as delivered only when the fixture's acceptance tests pass on
  the task's branch and its milestones are ready.
- Record what changed between the series (devws version, models, limits). A
  difference smaller than the spread within a series is not a result.

## Payload telemetry

To capture payload sizes on a benchmark task, create an empty
`benchmark-telemetry` file in its task directory before sending the goal.
The task directory is `${DEVAGENTICS_HOME:-~/.devagentics}/tasks/<task>`.
Remove that marker to stop recording. Keep the same setting across all runs
in a comparison.

DevAgentics appends metadata to `benchmark-telemetry.jsonl` in that directory.
It records MCP calls and catalog sizes, worker prompt segment sizes, and
worker turn starts and finishes. The ledger omits prompt text, tool arguments
and responses. Keep the ledger with the benchmark artifacts and archive it
with the task when the run ends.

`devagentics bench report <task> --json` includes `payload_telemetry` when a
ledger exists. It shows calls, failures, total bytes and p50/p95 response bytes
by surface and operation, as well as worker turn coverage. MCP byte counts
exclude line delimiters and measure the local protocol. Worker stream usage keeps absent counters unknown and separates uncached input,
cache reads, cache writes, output and reasoning where exposed. It is separate
from team usage and cost. Interrupted turns may have a start record without a finish record.

## Pricing coverage

Use the JSON report's `estimated_api_cost` for observed API-equivalent prices.
`observed_usd` is null when none of the observed usage has a known price.
When `missing_agents` or `unpriced_agents` is nonzero, any amount is a priced
subtotal. Agent counts describe the pane and archived readings available to
this report. Earlier Leader sessions can be absent even when every agent has a
reading. The legacy `usage` object remains available for its token counters.

Codex worker records with version 2 and `usage_source: codex_thread_delta`
subtract the previous total from the same resumed thread. If that previous
attempt lacks a complete result, the delta is unknown. Version 1 Codex worker
records can contain cumulative thread counters. For an existing Codex run,
deduplicate transcript `token_usage_record` entries by thread and response ID
and reconcile the resulting turn totals against the stream. Use either
source for each turn to avoid double counting.

## Proposal guidance

The Leader can request `application_workflow` with action
`proposal_example`, and planning workers can request action
`proposal_example` through `worker`. Both return the same example, checked
by the current proposal validator, including named check environments, decision prerequisites and test ownership.
The returned object is the proposal's input, with `reason` and `application`
at the top level. The example is read-only. Use the task's actual repo name,
scope, roles, check commands and next revision. Keep existing immutable
acceptance files outside all write allowlists and in the full checks.

A proposal can declare already-tracked acceptance files under
`application.existing_protected_tests`, for example
`{"notes":["tests/acceptance/test_notes.py"]}`. Keys are task repository
names. Values are exact committed regular-file paths, checked against the
task branch. Every reviewed writing order in that repository forbids editing
them, including orders that author new tests. Keep them in the relevant
final checks. Use an item's `protected_tests` with a direct `writes_tests`
dependency for newly authored tests instead. An amendment that removes an
existing protection requires user approval.

For order diagnostics, call `task_status` with `view: "order"` and the
`order` ID. The response includes its plan, assigned profiles, recorded
check outcomes and whether the evidence revision matches the order revision.
Use `read_check_log` for the full diagnostic excerpts. An evidence revision
match alone does not establish that the underlying code or runner is current.

Workers can call `plan_read` with `view: "section"` and `heading` for a
known heading title. The lookup matches the title exactly and returns
the section and its subsections up to the next peer or parent heading.
For a paged section, pass `next_line` as `start_line` with the same
heading. Use the outline and a line-range excerpt if titles repeat.

Workers can call `plan_read` with `view: "outline"` to find headings and
line numbers, then request `view: "excerpt"` with a one-based `start_line`
and `line_count`. An excerpt contains up to 120 lines and 8 KiB of plan text.
Use `next_line` to continue. If a single line exceeds the text limit,
`oversized_line` is true and the excerpt contains its prefix. The default
view still returns the full plan. The telemetry report counts outlines
and excerpts separately.

Managed Worker prompts now request `status` with `input.view: "brief"` at
the start of a turn. This view includes the current order's plan, permitted
and forbidden paths, targets, profiles, and pending review and question
references. Use `view: "full"` for complete contracts, questions, and
review history. Calls without a view still return full status.

For a noisy container command, use Worker `execute` with
`input.view: "compact"`. The response includes the exit code, bounded
stdout/stderr previews, and a result ID when capture succeeded.
Use `result_read` with the ID, `stream` and `offset`; follow
`next_offset` to page through the captured text. Each stream retains
up to 2 MiB and reports truncation. Results belong to the current
worker identity generation. After 32 captures in a generation,
compact requests return the existing bounded output and an explicit
capture-limit notice. The normal execute view remains available.

On a failed compact execution, `failure_diagnostics` surfaces a bounded
set of recognizable error lines that the stdout/stderr previews omit.
This helps with buried assertions and build errors. Use `result_read`
when you need context around a line or a diagnostic the summary missed.

## Full details on demand

For a Leader status call, use `task_status` with `view: "full"` to see
roles, account limits, recent logs and failures. Use `view: "progress"`
for routine updates and `view: "order"` for a specific order.

For Worker calls, request `input.view: "full"` for `status`, `plan_read`
or `execute` when you need the original response format. If a compact
execution already ran, inspect its captured streams with `result_read`.
Fetching the existing result avoids repeating a command that can change
the worktree.

## Direct CLI pilot for Codex

For an existing reviewed Codex team, run
`devagentics task leader-cli <task> on` and reopen its Leader. From the
primary worktree root, the Leader can use these commands.

```sh
devagentics leader-read <task> progress
devagentics leader-read <task> order <order-id>
devagentics leader-read <task> inbox
```

The commands return JSON using the same readers and result limit
as MCP. Codex runs them under its existing read-only sandbox. The commands
require the matching worktree and an opted-in reviewed Codex task. Other
Leader operations and the entire worker workflow continue through MCP.
The pilot's MCP catalog advertises `task_status` for full status. Earlier
MCP calls with explicit status views remain compatible.
Run `devagentics task leader-cli <task> off` to disable the direct reads
immediately. Reopen the Leader to remove the CLI instructions.

To measure the pilot, use matched tasks with the switch on and off.
Keep the compact response views, model, checks, and fixture constant.
Compare provider usage through merge and milestone validation. Count
direct CLI invocations and MCP fallbacks from the Codex transcript.
Both arms retain MCP. Record the narrower `task_status` schema in the
CLI arm and the regular schema in the MCP arm.

A CLI inbox read leaves messages pending. After handling them, call
`application_workflow` with `action: "inbox_ack"` and `input.ids`.
When acknowledgment and the next read are needed together, the existing
MCP `leader_inbox` call accepts `input.ack` in the same request.
Count both commands on the split path when comparing it with the combined
MCP call.
