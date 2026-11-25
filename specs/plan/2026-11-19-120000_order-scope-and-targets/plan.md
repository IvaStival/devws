# Orders say where a worker works and where its checks run

- **Date** 19-11-2026
- **Branch** `feat/opencode-workers`
- **Status** Executed

Today every worker `check` runs the whole project (Biome over all of `web/`,
the full 2095-test suite, all Pest tests), and workers explore the repo before
they start. The leader knows the step's files, so it says them in the order.

## Step 1 — The order names the files

`send_order` takes, besides `allowed` (the files to change):

- `read`: the related files the worker must read first;
- `targets`: the paths its checks run on (test files, folders).

Both are paths from the repo's root, without `..` or shell characters. The
worker sees them at the top of its order and in its `status`.

## Step 2 — Checks run on the targets

A check of an order with targets gets them as `DEVAGENTICS_TARGETS`
(space-separated) and runs only there when its command uses them. Targets are
part of the check's evidence, so a targeted pass never stands in for a full
one. The approved commands do not use the variable yet: the leader proposes a
revision that does, and the user approves it.

## Step 3 — The full suite once, at review

When the order's checks ran on targets, `review_request` runs every selected
profile in full on the merged candidate (the lane's work with the task
branch), not only the candidate profiles. A failure there is an integration
correction, as today.

## Step 4 — Instructions

The leader's instructions say every writing order names `allowed`, `read` and
`targets`, and that a planner's execution plan lists them for each step. The
worker standards say to start from `read` and run checks on the targets.

## Verify

Unit tests for each step, the devAgentics suite, fmt and Clippy, and the
Docker runner tests with the task's image.
