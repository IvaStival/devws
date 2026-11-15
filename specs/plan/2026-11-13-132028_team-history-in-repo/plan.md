# Plan: The team's history in the repo

- **Date:** 13-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed
- **Authorization:** The user asked to save the team history into the project and load it if it exists, so a leader on another server can continue. They chose: commit on every accepted step; on load, the done steps and plans come back and the plan stays approved.

## Context

A task lives only in `~/.devagentics/tasks/<task>/` (`state.json`, `plan.md`, `plans/*.md`) on the machine that ran it. Starting the same branch on another server makes an empty task: DONE is empty, `next_version` restarts at 1.1.0, and the leader does not know what was done.

## Design

- **In the repo:** `.devagentics/history/<task id>/` on the task's branch, in the first repo's worktree. It holds `history.json` (task, branch, approved, and the accepted steps in order: id, title, attempts, and their spec's release version), `plan.md`, and `plans/<step>.md`.
- **Saved:** after every `accept_step`. The history is written and committed with a pathspec, so only those files are committed and someone else's uncommitted work is never swept in. The message is `chore(team): record <version> <title>`. A failure to save is logged in the task's log; it does not undo the acceptance.
- **Loaded:** when `task start` makes a new task and the worktree has a history for it. The accepted steps come back, each with its release version as its `spec`, so DONE and its numbers read exactly as before. `approved` comes back too, and `plan.md` and the execution plans are copied into the task. The log says how many steps were loaded, and the leader's first message says that the task continues from that history.
- **Orders are unaffected:** the work check ignores `.devagentics/history/`. A history commit made while another order is open in the same worktree would otherwise fail that order's file rules.
- **Versions keep going:** `next_version` becomes the highest minor + 1, not the count + 1. Specs left out of a history (unfinished) can then never make a new spec reuse a number.

## Steps

1. **New** `agentic/src/history.rs`: `save(task)`, `load(task, worktree)`, the `History` format.
2. `orders::accept_step` saves the history after accepting.
3. `checks::inspect` ignores the history folder.
4. `start::start_task` loads the history of a new task and picks the leader's first message.
5. `specs::next_version`: the highest minor + 1.
6. Docs (`agentic/docs/usage.md`) and the leader role (`roles/leader.md`).

## Tests

- Accepting a step commits `.devagentics/history/<task>/` with the step, the plan and the execution plans, and only those files.
- Starting the same branch in a fresh home loads the steps (DONE, versions, `next_version`), the plan (approved) and the execution plans. The leader's first message says so.
- A history commit during another open order does not fail that order's check.
- `next_version` after a gap.
