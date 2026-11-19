# OpenCode team recovery and response verification

- **Date** 18-11-2026
- **Branch** `feat/opencode-workers`
- **Status** Executed

## Step 1 — Resume eligible workers

Add regression cases to the team and recovery fixtures. Recover a selected
worker with two unavailable peers that have empty pane IDs and no pending
assignments. Skip launch failures for those idle workers, keep them available
for retry, and surface each failure reason through the resume result. Cover
the opposite case with an outstanding order or review to ensure that recovery
retains the work and continues to report it as blocked. Check that retrying
resume does not duplicate panes or queue entries.

## Step 2 — Verify a response through the managed OpenCode worker

Add a leader MCP action that queues a fixed response-only prompt for a live,
ready, idle OpenCode worker on an unpaused task. Identify the prompt with a
unique expected reply. Reuse the no-order managed message path and the existing
provider configuration. Render a diagnostic-specific prompt and verify the
reply in provider output so it can be surfaced to the leader. Prevent the probe
from creating a step, binding an execution item, or allowing write actions.
Cover enqueue, delivery, response matching, and busy or unavailable workers.
Update the unknown execution item error to name the requested step and explain
the accepted-item requirement for ordinary reviewed orders.

## Step 3 — Verify the integrated workflow

Run the focused Rust tests, the devAgentics test suite, formatting, Clippy, and
the repository check. Run the probe through real OpenCode with the existing
local provider fixture, if available. Verify the selected live worker and model
when the running task and authenticated provider are available. Record which
environment produced each observed response and any prerequisites that prevent
a live verification.

## Verification

- Rust tests passed with 400 unit tests and 9 integration tests. Six optional
  runtime tests were skipped by the default run. Formatting, Clippy with
  warnings denied, and the diff whitespace check passed.
- OpenCode 1.19.6 passed the existing adapter fixture with a local provider.
  A separate end-to-end fixture sent `probe_worker` through the devAgentics
  MCP server, ran the managed OpenCode Floor panel, and observed the exact
  reply in the leader's inbox with no order or execution item created.
- `make check` stopped at the Ruby prerequisite in `scripts/check.sh`.
- The live task was unpaused and its active workers were using Codex at
  verification time. No reply from `deepseek/deepseek-flash` was observed.
- Installed the tested devAgentics build and verified that its leader MCP
  catalog exposes `probe_worker` for the live task.
