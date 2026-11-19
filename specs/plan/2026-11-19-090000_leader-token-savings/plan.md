# Fewer leader calls

- **Date** 19-11-2026
- **Branch** `feat/opencode-workers`
- **Status** Executed

The leader works only through tools, and every call and its result stay in its
context. Of about 210 calls in one session, about 34 went to replacing workers
that crashed on an account error, about 25 to reading the inbox and
acknowledging it, and about 6 to guessing order IDs.

## Step 1 — A worker refused by its account restarts on the next one

When a managed worker's turn fails with an account error (no credit, usage
limit, rate limit) before it did anything, devAgentics records the failure:

- it holds that account for 30 minutes, so no new agent of the task starts on
  it (`accounts::disabled_for`), and `task_status` shows the hold;
- it drops the claimed turn, since nothing of it ran;
- it marks the worker for a restart. When the dispatcher sees its panel gone,
  it restarts that worker alone, on its next account, with its order. This
  happens only when the task is not paused.

The leader gets one message saying what happened. A turn that already used a
tool still goes to explicit recovery, as before.

## Step 2 — One inbox call

`leader_inbox` takes `ack` (the IDs handled last time) and acknowledges them
before reading. It also returns a one-line roster of the live agents: role,
lane, program, its order's step, lost or not. Reading the inbox then replaces
the separate `inbox_ack` call and most status reads.

## Step 3 — A wrong order ID lists the right ones

`run_check` on an unknown order answers with the open orders and their agents
and steps.

## Verify

Unit tests for each step. Run the devAgentics suite, fmt and Clippy, then
install the build.
