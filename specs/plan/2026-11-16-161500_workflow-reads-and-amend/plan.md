# Workflow status in pieces, and baseline amendments

Status: built and installed (2026-11-16, approved by the user); live check pending

## Why

On 2026-11-16 the feat-system-observer leader (Claude) got stuck.
`application_workflow status` returned 246 KB, 188 KB of it the baseline
(54 items, 173 KB). Claude Code saves a result that big to a file, and the
leader's role has no file reader. Planner-8's tools didn't show it either.
Even with the data, revision 4 would have needed the whole 188 KB baseline
sent back through `propose`. The user chose to fix both.

## Changes
1. **An outline of the baseline** (`execution::outline`): everything but the
   items' full contracts. Each item is just `{title, scope, role, lane,
   dependencies}`.
2. **Leader reads (`workflow::call("status", input)`):**
   - Without `section`, it returns the status with `application` and
     `proposal` as outlines, plus a `read_more` hint.
   - With `section` (`application`, `proposal`, `runner`, `ready_work`,
     `reviews`, …), it returns only that part.
   - For `application`/`proposal`, `items: [ids]` returns those items' full
     contracts. An unknown id is an error.
   - `task_status.reviewed_workflow` gives only the baseline's and the
     proposal's revisions and item counts (`workflow::brief`), because
     task_status carries much else. The board keeps the full status for its
     details.
3. **Planner reads:** the worker action `application_read` (non-writers)
   takes `{proposal?: bool, items?: [ids]}` and returns the outline or those
   items. Worker `status` gives the proposal as an outline.
4. **Amendments (`execution::amend`):** they start from the pending
   proposal, if it is the next revision, or from the current baseline. On top
   of that, they apply only the changes:
   `{goal?, non_goals?, scopes: {id: text|null}, items: {id: contract|null},
   milestones: {id: milestone|null}, decisions: [decision…] (by id),
   check_configuration: {repo: checks|null}}`. `null` removes. The result
   goes through `store`, the same as a proposal: validated, next revision,
   unapproved. The leader's action is `amend {changes, reason}`, the
   planner's is `plan_amend {changes, reason}`. The planner's reports the
   step, like `plan_proposal`.
5. **Docs:** the tool description, `leader.md`, the planner's instructions,
   and `usage.md`.

## Tests first
- The outline has no contract bodies, and it keeps ids and titles.
- `status` without a section has outlines. `section=runner` returns only the
  runner. `items` returns those contracts, and an unknown id is refused.
- `amend` adds, replaces and removes items on the next revision, and keeps
  everything else. A second amend builds on the first proposal. An invalid
  result is refused, through `store`.
- The worker's `application_read` and `plan_amend` work for a planner and are
  refused for a writer.

## Verification
`cargo test`, `cargo clippy --all-targets`, install, and restart the leader
so it gets the new tool description. Live: the leader reads the runner section
and the items it needs, and amends revision 4.
