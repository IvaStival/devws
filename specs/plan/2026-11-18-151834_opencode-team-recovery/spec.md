# Restore OpenCode team recovery and response verification

- **Date** 18-11-2026
- **Branch** `feat/opencode-workers`
- **Status** Executed

## Context

After a usage override, team recovery restarted OpenCode Floor agent
`hammer-12` with `deepseek/deepseek-flash` and reported that two other
unavailable workers had empty pane IDs. The task stayed paused. The subsequent
response-only order used a step without a reviewed execution item, which
`send_order` rejected before delivery.

## Requirements

1. Treat an empty pane ID as a worker without a pane. Recovery should resume
   deliveries to eligible workers when another unavailable worker cannot be
   restarted and has no open assignment or review. Report any skipped workers
   and preserve their state for retry.
2. Preserve recovery safeguards for workers with outstanding assignments or
   reviews, including pane readiness and prevention of duplicate delivery.
3. Provide a supported way to verify a response from the selected OpenCode
   worker without publishing a work order or changing repository files. Regular
   reviewed assignments still require an accepted execution item. Explain that
   requirement in the error returned for an unknown item.
4. Use the existing task usage override and model routing for the probe.

## Likely files

- `agentic/src/team.rs` and `agentic/src/recovery.rs` for worker and task recovery.
- `agentic/src/orders.rs`, `agentic/src/mcp.rs` and the worker delivery path for
  order diagnostics and response verification, with the exact path determined
  by the existing transport.
- Relevant Rust regression tests for recovery, order validation and delivery.

## Acceptance criteria

- A restarted OpenCode worker can receive work after resume when unrelated,
  unavailable workers have empty pane IDs and no outstanding assignment.
- Recovery retains and reports pending work when a worker with an outstanding
  assignment cannot restart.
- A reviewed order for an unknown execution item explains how to proceed and
  cannot create an unreviewed assignment.
- A response-only probe uses the selected OpenCode worker's normal delivery
  path. Record whether a live reply was observed and any runtime prerequisite
  that prevents verification.
