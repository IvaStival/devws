# A faster reviewed team: true progress, writer capacity, batched approvals

Status: built and installed (2026-11-16, approved by the user); live check done (board shows 4/56 · 7% · +26 helper steps)

## Why
After 21 hours of real work on `feat-system-observer`, only 4 of the 56
baseline items were accepted. The board's progress bar showed 46%, because it
counts the leader's helper steps (plans, preflights, clarifications), which
are 24 of the 30 steps. Workers were idle 18 of those 21 hours. Two writers
were busy at once for only 40 minutes. One reason: the three capacity slots
were held by an idle planner and an idle reviewer, so `H1-AT`, which was ready,
was refused with "agent capacity occupied". Waiting for the user's approvals
took about 12 hours. Each approval stopped the team on its own: 5 baseline
revisions and 8 runner permissions, asked one at a time.

## Changes
1. **True progress (E).**
   - In a reviewed task, the board's PROGRESS shows baseline items accepted out
     of all baseline items (`4/56 · 7%`).
   - Helper steps are shown after it as `+N helper steps` and no longer count
     toward the percentage.
   - A legacy task keeps counting its steps, as now.
2. **Capacity counts writers (F).**
   - `max_agents` limits writers. Read-only seniors (planners, reviewers) have
     their own limit, also `max_agents`.
   - This applies to spawning (`team.rs`), resuming (`team.rs`) and the
     ready-work check (`workflow.rs`). With it, an idle planner no longer blocks
     a ready item.
   - The status `capacity` reports `writers` and `seniors` separately.
   - Auto-release of idle agents is left out: with separate limits it isn't
     needed.
3. **Batched approvals (G), in the leader's role (`roles/leader.md`).**
   - While a decision is waiting for the user, the leader keeps every
     independent ready item running.
   - It asks for all pending decisions in one numbered message, rather than
     one escalation each.
   - It proposes a runner change together with the baseline revision that
     needs it, as one approval.
   - The leader is given no new tool; this is guidance only.

## Tests
- Board progress for a reviewed task (baseline items plus helper count) and
  for a legacy task (unchanged).
- A planner and a reviewer that are live do not stop a writer from spawning,
  resuming, or showing as ready.
- Writers are still capped at `max_agents`.
