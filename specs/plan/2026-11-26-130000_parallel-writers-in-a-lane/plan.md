# Parallel writers in a lane: work that doesn't overlap runs at the same time

- **Date** 26-11-2026 · **Branch** `feat/agentic-lane-seats` (worktree `~/Projects/devws-seats`), on top of PR #37
- **Status** Approved and built (26-11-2026): A, B and C, with tests. Not yet tried live.
- **Built differently from the plan:**
  - Ready items list `overlaps` (the unmerged orders they can't run next to) instead of `alongside`: it says what to wait for.
  - Seats are `lanes/<lane>.<n>` on `<lane branch>.<n>` (a dot, which a lane's name never has).
  - The second overlap check runs when an open order's files are widened (`orders::amend`), not before a merge: that's the only way an open order's globs change.
  - Fixed on the way: resuming a lost writer was refused while any writer was live in its lane; it now checks the seat.

## Context

In feat-system-observer, the front has about 24 steps left and one writer.
The leader wanted more speed, so it planned a new lane just for the redesign
(`TeamWorkspace/`). That lane would exist only to get a second worker,
because devAgentics allows **one writer per worktree**
(`team.rs:206-210`), and a lane *is* one worktree (`lanes.rs:30-100`).

Most of what parallel work needs already exists:

| Already there | Where |
|---|---|
| Each item lists the files it may write (`allowed` globs) | `execution.rs:120` (`Contract.allowed`) |
| An order takes those globs; review rejects a change outside them | `orders.rs:266`, `checks::inspect` |
| An item waits for its `dependencies` and the interfaces it `consumes` | `execution.rs:118-145` |
| Each lane has its own worktree, branch, merge and candidate checks | `lanes.rs`, `integration.rs` |
| The board's staffing hint: one writer for each lane with ready work | `workflow.rs:645-660` |

What's missing: a lane can't hold more than one worktree, so two items that
write different folders still wait for each other.

## In plain words: what changes

1. **A lane can have more than one seat.** A seat is a worktree with its own
   branch, so it gets its own writer. The first seat is the lane as it is
   today. Seat 2 (`front#2`) is created when a second writer starts in the
   lane. It has the same name, files, image and checks, so everything that
   asks "which lane" gets the same answer.
2. **Two seats never work on overlapping files.** An order can go to a seat
   only if its `allowed` globs **provably** don't overlap those of any other
   unmerged order in the lane. "Provably" is deliberately strict:
   - `web/src/components/TeamWorkspace/**` and `web/src/pages/History/**`
     don't overlap: they split at `components` / `pages`.
   - `web/src/**` and `web/src/pages/History/**` overlap.
   - An item with no globs, or `**`, overlaps everything, so it runs alone
     in its lane.
   - If they overlap, the order is refused with the conflicting order and
     glob named. It goes to the seat that holds the other order once that one
     merges.
3. **The leader still decides.** devAgentics doesn't start workers or send
   orders by itself. It refuses unsafe parallel work and tells the leader
   what can run alongside: `task_status` lists "can run now, next to …" for
   each ready item. The staffing hint counts groups of ready items that
   don't overlap, not lanes.
4. **Merging stays as it is.** Each seat merges into the task's branch like
   a lane does today. Because the globs don't overlap and review enforces
   them, the merge has no text conflicts. Changes that each pass alone but
   break together (a type both import) are caught by the existing candidate
   checks on the merged result. When one seat merges, the other seat's
   candidate is rebuilt on the new base and its checks run again. That's the
   price, and the plan doesn't hide it: the board shows the reruns.
5. **Limits stay the same.** `max_agents` still caps all writers. A new
   `max_seats` caps the seats in one lane (default 2; `task.toml` can raise
   it). The host's container slots still cap the suggestion.
6. **No more fake lanes.** The leader's role text says: use a lane for a
   different area (image, checks, folder); use a seat for more hands in the
   same area.

### Example: today's front

| Ready item | `allowed` | Seat |
|---|---|---|
| G4 Agent rail | `web/src/components/TeamWorkspace/Rail/**` | front#2 |
| R2 History empty state | `web/src/components/Observe/History/**` | front |
| G5 Records panel | `web/src/components/TeamWorkspace/Records/**` | front#2, after G4, or front#3 if `max_seats` = 3 |
| R3 Ledger layout | `web/src/components/Observe/Layout/**`, `web/src/constants/layout.ts` | front, after R2 |
| G7 Page assembly | `web/src/app/**`, `web/src/constants/**` | refused while R3 is open: it overlaps on `web/src/constants/layout.ts` |

The leader sends G4 to a new writer in the front lane (it gets seat 2) and R2
to the existing writer. G7's refusal says "overlaps o2101 (R3) on
`web/src/constants/layout.ts`". That's exactly the clash a separate lane
would only have found at the end.

## Design

### A. Seats (`lanes.rs`, `state.rs`, `team.rs`)
- `Agent.seat: u32` (`#[serde(default)]`; 0 and 1 both mean the lane's own worktree, so older states read as seat 1).
- `lanes::seat(task, lane, number) -> Lane`: number 1 is the lane itself.
  Number n ≥ 2 is the same `Lane` with `branch = <lane branch>-<n>` and `worktree = lanes/<name>-<n>`, created on first
  use from the task's branch, like `lanes::add`. Name, repo and globs are
  the lane's own, so `for_lane(name)`, item matching (`workflow.rs:38`,
  `orders.rs:1152`) and checks need no change.
- `lanes::workplace(agent)` and `merge_into_task` resolve the agent's seat
  instead of the lane by name. `integration::candidate/merge` already take a
  `Lane`, so they get the seat's.
- `team.rs:206` changes "one writer per lane" to "one writer per seat". A
  writer started in a lane whose seats are all taken gets the lowest free
  number, up to `max_seats`; past that the start is refused and says so.
- A seat's worktree stays after its writer stops, for the next writer. It's
  removed with the task, like lanes.

### B. Disjoint orders (`checks.rs`, `orders.rs`)
- `checks::disjoint(a: &[String], b: &[String]) -> bool`: true only when
  every pair of globs splits at a literal path segment before either one's
  first wildcard. An empty list or `**` is never disjoint. This is
  conservative like `checks::within`: when in doubt, they overlap.
- `orders::send_order` in a lane with another seat busy: refused unless the
  order's `allowed` is disjoint from every other unmerged order in the same
  lane. The error names the order, its step and the first overlapping pair.
- The same check runs before a merge, against the order that merged since,
  in case an amendment widened the globs.

### C. What the leader sees (`workflow.rs`, `status.rs`, `roles/leader.md`)
- `staffing` counts the largest set of ready items in a lane whose globs are
  pairwise disjoint (greedy; exact isn't needed for a hint), per lane, capped
  by `max_seats`, `max_agents` and host slots.
- `task_status` ready items gain `alongside: [ids]`, the open orders it may
  run next to.
- The board's lane line shows its seats: `front 2/2 seats · 4 ready, 1 waits
  on overlap`.
- `roles/leader.md`: seats instead of extra lanes for more hands; keep
  shared files (routes, constants, layout) in their own items, so they block
  only what really touches them.

### D. Not in scope
- devAgentics starting writers or assigning orders by itself.
- Matching review capacity to writers. Reviews already wait in the queue,
  and the board shows the wait.
- Timing per lane or agent (today's timing intervals have no agent). That's
  worth a follow-up so a lane's bottleneck shows on the board.

## Tests (written first)
- `checks::disjoint`: `a/**` vs `b/**` yes; `a/**` vs `a/x/**` no; `a/*.ts` vs
  `a/b.ts` no; empty vs anything no; `**` vs anything no; `a/x.ts` vs `a/y.ts`
  yes; `{a,b}/**` vs `c/**` no (braces are not parsed: overlap).
- A second writer in a lane gets seat 2 with its own worktree and branch; a
  third is refused when `max_seats` = 2; seats are per lane.
- An order whose globs overlap another unmerged order in the lane is refused
  with the order and glob named; a disjoint one is accepted.
- Two seats' accepted steps both merge into the task's branch; the second's
  candidate is rebuilt and its checks run against the new base.
- Checks, images and item matching in a seat are the lane's (the
  `for_lane(name)` result is unchanged).
- `staffing` suggests 2 for two disjoint ready items in one lane, 1 for
  two that overlap.
- The flow test (`tests/flow.rs`): two writers in the front lane, disjoint
  items, both accepted and merged.

## Verification
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo
  test`, `scripts/check.sh`.
- Live, on feat-system-observer: once it's installed and the team is
  restarted, the leader starts a second front writer. G4 (TeamWorkspace)
  runs next to an R repair, and G7 is refused while an order touching
  `constants/` is open.
