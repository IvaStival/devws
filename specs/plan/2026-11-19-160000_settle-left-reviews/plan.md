# The leader settles the reviews it leaves behind

- **Date** 19-11-2026
- **Branch** `feat/opencode-workers`
- **Status** Executed

Reported steps sat on the board's "waiting for the leader" list for up to 83
hours: the leader read the reports of plans, preflights and audits, used them
and moved on without accepting or rejecting them. Nothing reminded it, and
neither accept (approving) nor reject (re-ordering) fits a step whose report
was simply used or superseded. The list then read as problems for the user.

## Step 1 — A reminder per left review

The dispatcher tells the leader, in one inbox message, every step that has
waited for it (reported, check failed, blocked, sent back) longer than
`STALE_REVIEW_SECS`: accept, reject or close it. Each step once per wait
(`State::reminded_reviews` keeps the wait it was reminded of); a step that
waits again is reminded again. Not while the task is paused.

## Step 2 — `close_step`

A new leader tool and step status `closed`: the step waited for the leader
(reported, check failed, blocked, sent back) and its work is not needed, or
its report was used. Logged with the reason; it leaves the waiting list and
counts as neither accepted nor open. Refused when:

- an agent still works on it (ordered): stop it first;
- its last order's author writes: dropping written work is a scope change
  (reject it, or propose an amendment);
- another baseline item depends on it: dependents need it accepted.

## Step 3 — The leader's rule

`roles/leader.md`: settle every report in the turn you read it — accept,
reject or close; never leave one waiting.

## Tests

- orders: close only a waiting step, with a reason; refused for an ordered
  step, a writer's step and a step another item depends on.
- watch: a left review is reminded once, again after it waits anew, never
  before `STALE_REVIEW_SECS` or while paused.
- board: a closed step is not waiting for the leader.
