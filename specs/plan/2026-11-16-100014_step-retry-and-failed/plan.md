# Step attempts: 5 by default, one delayed retry, then "failed"

Status: built and installed (2026-11-16, approved by the user); live check pending

## Why

On 2026-11-16, step B1-1-AT used its 3 attempts on infrastructure problems
(no git author, leftover lane files, `.next` build output). Each time the
team stopped until the user granted more attempts by hand. The user decided:
every step gets 5 attempts. When those run out, devAgentics waits 15 minutes
and tries once more by itself. If that fails too, the step gets a new
**failed** status with a red mark, and only then does it need the user.

## Changes

1. **Default 5:** `task::DEFAULT_MAX_ATTEMPTS` goes from 3 to 5. Update the
   doc comment and the `leader.md` text ("the task's `max_attempts` (3)").
   Set `max_attempts = 5` in the running task's `task.toml`.
2. **Delayed retry**, in the dispatcher (`watch.rs::tick`). New state:
   `State::step_retries: BTreeMap<step, StepRetry>`, where `StepRetry` is
   `Waiting { at }` or `Granted`.
   - A step that ended Blocked, Failed or Rejected, has used its whole limit
     and has no retry entry gets `Waiting { at: now + 15 min }`. The leader is
     told: "Step X used its N attempts; devAgentics retries it once at HH:MM."
   - When `now >= at`, the limit becomes `used + 1` (through `attempt_limits`)
     and the entry becomes `Granted`. The leader is told to send the step's
     order again (the auto-nudge wakes it).
   - A step that is `Granted` and ends badly again becomes `Exhausted` (next
     item). The leader is told it failed for good and to escalate to the user.
   - A step the user grants more attempts by hand (`task attempts`) loses its
     retry entry, so the cycle starts over after those attempts.
   - All of this waits while the task is paused.
3. **Status "failed":** new `StepStatus::Exhausted`. The board shows it as a
   red `✕` labelled "failed". The existing `Failed` (a check failed on one
   attempt) stays the yellow `⚠`. It appears in NEEDS YOU.
   `send_order`/reviews still refuse a new attempt, and `task attempts` still
   reopens the step (the status goes back to whatever the next order sets).
4. **Docs:** in `docs/user/devagentics-execution.md` / `agentic/docs/usage.md`,
   wherever attempts are described.

## Tests first
- The default is 5, and a task.toml without the field reads as 5.
- Dispatcher, with an injectable clock (`now` passed in, as `watch_for_a_stall`
  uses `Instant` and timing uses `log::now()`):
  - a step at its limit after a block is scheduled once and the leader told;
  - before 15 min nothing changes; after it the limit is used + 1 and the
    leader is told;
  - when that attempt is blocked too, the step becomes Exhausted and the leader
    is told once;
  - a manual `allow_attempts` clears the entry;
  - a paused task does nothing.
- Board: Exhausted shows `✕` in red, labelled "failed", and is in NEEDS YOU.

## Verification
- `cargo test`, `cargo clippy --all-targets`, install, restart the
  feat-system-observer board.
- Live: `task.toml` says 5, and the board shows the new statuses.
