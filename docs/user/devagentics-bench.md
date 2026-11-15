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

   It prints the folder, the commands to start the team and the check. It
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

## Protocol

- Run each way of working at least ten times on the same fixture, from a fresh
  copy each time, with the same models and answers.
- Compare the distributions (median and spread of first-pass share,
  corrections, elapsed time, user interventions and cost), not single runs.
- A run counts as delivered only when the fixture's acceptance tests pass on
  the task's branch and its milestones are ready.
- Record what changed between the series (devws version, models, limits). A
  difference smaller than the spread within a series is not a result.
