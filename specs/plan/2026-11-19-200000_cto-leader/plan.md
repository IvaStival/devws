# Make the devAgentics leader a CTO

- **Date** 19-11-2026
- **Branch** `feat/opencode-workers`
- **Status** Executed (all phases); live verification on feat-system-observer

## Context

In the live reviewed task `feat-system-observer`, steps burned through attempts for reasons that weren't bad work:
- C5a-1-AT used 8 orders and B3 used 11;
- the causes were a stale-check bug, the test-first deadlock, evidence going stale after merges, and baseline revisions resetting dependents;
- each time, the leader re-ordered and asked the user for more attempts.

Root causes found while exploring:

- **The leader can't see the code.** In reviewed tasks it has no shell and no tool that reads a file, a diff or git history. The only exception is `project_overview`, which covers six root files, cut at 8 KB each. `roles/leader.md` even tells it to `git diff` a worktree it can't reach.
- **Failures leave no trace on the step.** A step keeps only `status` and `attempts`. Reasons go out once, in inbox messages. `task_status` shows no cause.
- **Every order counts as an attempt** (`orders.rs` `send_order`: `step.attempts += 1`). That includes re-checks and orders devAgentics forced.
- **`send_order` never asks what will be different**, so the leader can repeat the same failing order.
- **A baseline revision resets every transitive dependent** of a changed item (`execution.rs` `confirm`), and doesn't tell the leader.
- **Workers rediscover the code on every order.** Memory notes are tied only to steps; nothing shares knowledge about an area of the code.

**Goal:** the leader understands the project, diagnoses failures and changes the approach, or delegates to someone who can fix the problem. Only real work failures count as attempts. Workers inherit what the team already knows through ai-memory.

**User decisions:**
- The leader gets **read-only code access only**: no running commands, no writing. Workers still write all the code.
- **Diagnose before retry is enforced.**
- The leader **shares knowledge with workers through ai-memory**.

Defaults I picked for open points:
- The leader may also read `*.md` docs outside `source_paths`, but never what the exclusion rules refuse.
- A change to the goal or non-goals no longer resets every item. The leader is told to check the unaccepted ones.

## Phase 1: Failure history and fair attempt counting

**New `src/failures.rs`.** State lives in a map, so the ~41 `Step` literals stay untouched:
- `State.step_failures: BTreeMap<String, Vec<Failure>>`
- `State.diagnosis_waivers: BTreeSet<String>`

**`Failure`** fields: `at`, `order: Option`, `kind`, `summary` (≤500), `diagnosis: Option`, `resolved_by: Option<order>`.

**`FailureKind`** has three weights:
- **Work**, always counts: `CheckFailed` (test, required-skipped or file-policy failure, dirty tree, files out of scope) and `ChangesRequested`.
- **Ambiguous**, counts unless the leader's diagnosis names a non-work cause: `CheckTimeout`, `IntegrationFailed`, `ReviewBlocked`, `AgentBlocked`, `LeaderRejected`, and `Unrecorded` (legacy steps already failed).
- **Not work**, never counts: `CheckEnvironment` (only environment or runner failures), `StaleEvidence`, `RevisionReset`, `AgentStopped`.

**`Cause`**, the leader's diagnosis:
- work causes: `implementation_defect`, `test_defect`, `plan_defect`, `missing_context`, `integration_conflict`;
- non-work causes: `other_work`, `environment`, `devagentics`.

**`Diagnosis`:** `{cause, evidence, change}`.

**Functions:** `record` (at most one failure per order), `pending`, `counts`, `work_failures`, `needs_diagnosis`, `resolve`, `guidance`, `status_json`.

**Where failures are recorded,** in each site's existing transaction:

| Site | Kind |
|---|---|
| `orders.rs` report: blocked | AgentBlocked |
| `orders.rs` report: failed checks | classified from the evidence phases |
| `orders.rs` `report_findings` blocked | AgentBlocked |
| `reviews.rs` `check_candidate_first` | IntegrationFailed or CheckEnvironment |
| `reviews.rs` submit | ChangesRequested or ReviewBlocked |
| `orders.rs` `reject_step` | LeaderRejected |
| `team.rs` `stop_agent` with an open order | AgentStopped |
| `execution.rs` revision reset | RevisionReset |
| stale evidence | StaleEvidence |

For stale evidence, add a typed error `failures::Stale`. The stale bails in `check_profiles.rs` and `reviews.rs` return it, and the MCP handlers for `review_request` and `accept_step` downcast it and record the failure.

**Counting:**
- `ensure_attempt_left` and `reviews::request` both gate on `failures::work_failures`, which replaces both the order count and `changes_requested`.
- `set_attempt_limit` and `watch.rs` `retry_spent_steps` use the same count. Stale `Waiting` entries are cleaned up.
- `Step.attempts` stays as a count of orders, for the board and `bench`.
- No migration is needed: old steps start with 0 work failures, and already-failed steps get a pending `Unrecorded` failure.

**`task_status` per step** (`status.rs`): `orders`, `work_failures`, `attempt_limit`, `needs_diagnosis`, and the last 3 failures. These appear only for open steps, with summaries ≤160 chars.

## Phase 2: Diagnose before retry, enforced in `send_order`

**Request:**
- `OrderRequest.diagnosis: Option<Diagnosis>`, with `diagnosis: None` added to its 12 literals.
- The MCP schema gets the `cause` enum, `evidence` and `change`.

**`failures::resolve`,** called inside `send_order` before the attempt gate:
1. **No pending failure:** nothing is needed.
2. **Only not-work failures pending:** a diagnosis is optional. They are marked resolved and cost nothing. This covers re-checks, resets and environment failures.
3. **A work or ambiguous failure pending:** a diagnosis is required, with `evidence` ≥20 chars and `change` ≥10. Without one, the order is refused, naming the failure and where to look (`read_check_log`, `git_diff at=<order>`, review findings).
4. **Same cause twice with the same `Approach`:** refused. An `Approach` is the agent, plan, contract hash, `allowed`, `read` and `targets`. The refusal says: "change the approach (another agent, an amended contract or plan, other read/targets, split), or `close_step`, or `escalate`."

**Waiver:** only the user's `devagentics task attempts` waives the same-cause rule, once (`diagnosis_waivers`). The leader's `allow_attempts` raises the limit, needs a diagnosed cause, and never waives.

**Failure messages:** `failures::guidance` is appended to the report-failure, INTEGRATION FAILED, review-verdict and retry messages, and to the attempt-limit refusal.

## Phase 3: Read-only code tools (new `src/code_access.rs`, thin handlers in `mcp.rs`)

**The tools,** all `Effect::System`:
- `read_file{path, at?, repo?, start?, lines?}`: a folder path lists its entries.
- `search_code{pattern, at?, repo?, glob?, regex?}`
- `git_log{at?, repo?, path?, limit?}`
- `git_diff{at? | from?/to?, repo?, path?, stat?}`

**What `at` names:**
- the task branch (the default);
- `lane:<name>`;
- an order `o123`, meaning its `base..head` from the order's check evidence, its open workplace, or its review source;
- a hex SHA.

Everything else is rejected, which stops option injection. Paths always go after `--`, and patterns go in with `-e`.

**Path policy:** a new `runner::leader_readable(path, Option<&Configuration>)`, next to `exclusion`:
- The built-in exclusion rules and `protected_paths` always apply, even with no runner configured.
- With a runner, the path must also match `source_paths` or be a `*.md` file.
- `runner::relative` becomes `pub(crate)` to validate paths.

**Git access:**
- Only object-level git commands (`ls-tree`, `cat-file`, `grep`, `log`, `diff`), through `runner::safe_git` plus a new `safe_git_bytes`. Nothing touches a worktree or an index.
- Lane branches share the repo's object store (`lanes.rs` uses `git worktree add`), so every command runs in `repo.worktree`.

**Output limits,** all below `fitting`'s 80k:
- `read_file`: 300 lines by default, 1500 at most, 60k chars at most; symlinks, submodules, binaries and files over 2 MiB are refused.
- `search_code`: 200 matches.
- `git_log`: 20 commits by default, 100 at most.
- `git_diff`: filtered with `--numstat` first (left-out files are only counted), then at most 20k chars per file and 60k in total, with a hint to ask for one path.

**Follow-up, not in this plan:** move the reviewer's `review_diff` (`worker_tools.rs`) onto `code_access` diffing, so it gets the same filter and caps.

## Phase 4: Revisions reset only what changed (`execution.rs` `confirm`)

- `affected` becomes only the items whose contract changed. Drop the transitive-dependents loop and the reset on a goal change.
- **Changed items:** close their orders, drop their queued messages and evidence, set them to Rejected, and record RevisionReset.
- **Removed items:** set them to Closed.
- **Dependents:** keep their status. `ready()` still blocks new orders on dependencies that aren't accepted.
- **The leader is told** which items were reset and which dependents to check with `read_file`/`git_diff`. On a goal change, it's told to check every unaccepted item.

## Phase 5: Shared knowledge through ai-memory

- **A new `NoteKind::Knowledge`** (`memory.rs`) and a `Note.area: Option<String>`. The area is a lane name or a path prefix such as `backend/app/Services/`. Pages are stored under `<prefix>/knowledge/<slug(area)>/` (`notes.rs`).
- **Saving:** `memory_save` (the leader tool in `mcp.rs` and the worker action in `worker_tools.rs`) takes `area`, which is required for the knowledge kind. It holds how an area works: key files, conventions, traps, how to test it.
- **Attaching:** a new `notes::knowledge_for(task, devws, lane, paths)` returns the knowledge notes whose area is the order's lane or a prefix of its `allowed`/`read` paths, newest first, at most 6. `send_order` attaches them next to the step notes it already includes (`mcp.rs`, `notes::about_step` + `fenced`), with: "The team's knowledge of this area is below: rely on it, read only what it doesn't cover, and save what you learn with `memory_save` kind=knowledge area=…".
- **Curation:** the leader saves knowledge after investigating an area, corrects outdated notes by saving a newer one, and checks an area's knowledge before reading the code itself.

## Phase 6: The leader's instructions, first message, docs

**`roles/leader.md`:**
- **A new section, "Know the project; own the outcome":**
  1. Learn the project at the start, and after a move or compaction: `project_overview`, then `read_file` for CLAUDE.md, AGENTS.md, README and the docs they name, plus the area knowledge in memory.
  2. Investigate before deciding: `read_check_log`, `git_diff at=<order>`, `read_file`/`search_code`, `git_log`, review findings.
  3. Root-cause discipline: classify each failure, re-order only with a `diagnosis`, and on the same cause twice change the approach or close the step.
  4. Delegate: environment or image problems to infra, contract flaws to a planner (amend), devAgentics bugs to the user with evidence.
  5. Share what it learns as knowledge.
- **Remove** the shell-only `git diff` instruction and the "you have no shell" wording, and update the attempts paragraph and the evidence list in "When to ask the user".

**Other files:**
- `src/start.rs`: `LEADER_FIRST` and `LEADER_CONTINUES` start by learning the project.
- `src/roles.rs`: the tool-name test adds `read_file`, `search_code`, `git_log` and `git_diff`.
- `docs/how-it-works.md`: the tool tables, attempts and steps, and new subsections "Failure history and diagnosis" and "Shared knowledge".
- `docs/usage.md`: the tool list, the attempts paragraph, the waiver, the migration note.
- `specs/plan/<date>_cto-leader/plan.md`: this plan, with its status kept current.

## Order of work

Phases 1 → 2 → 4 → 3 → 5 → 6, each test-first, and each with `cargo test`, clippy and fmt passing before the next. Phase 4 is independent of Phase 2. Phases 3 and 5 must land before Phase 6, because `leader.md` names their tools.

## Verification

**Unit and behaviour tests:**
- **`failures.rs`:** what counts and what doesn't; ambiguous failures follow the diagnosis; one failure per order; legacy failed steps need a diagnosis.
- **`orders.rs`:**
  - a re-order after a failed check needs a diagnosis;
  - a re-check after stale evidence needs none and is free;
  - five environment failures still allow an order;
  - the same cause twice is refused until the agent or contract changes;
  - the user's waiver works once;
  - the limit applies to work failures.
- **`reviews.rs`:** an integration failure is ambiguous; changes requested counts once.
- **`watch.rs`:** the retry ignores not-work failures.
- **`execution.rs`/`workflow.rs`:** only changed items reset; dependents keep their status and are named to the leader; a removed item is closed.
- **`code_access.rs`:**
  - reads at the task branch, a lane and an order;
  - `.env`, keys and protected paths are refused, left out of search, and only counted in a diff;
  - paths outside `source_paths` are refused, `.md` is allowed, and with no runner `.env` is still refused;
  - `..`, absolute paths and `-` refs are refused;
  - output is capped with a hint;
  - `git status` and HEAD are unchanged afterwards.
- **`notes.rs`/`mcp.rs`:** a knowledge note for `backend/app/` is attached to an order whose `allowed` is under it, and not to a front-lane order; `memory_save` refuses knowledge without an area.
- **`mcp.rs`:** the tool list, `send_order` with a diagnosis, `read_file` through the server, and `task_status` showing failures.

**End to end:**
- `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.
- Install: `DEVWS_ROOT=$PWD config/tmuxinator/rust_tools.sh install --rebuild`.
- Restart the `feat-system-observer` team when it's idle, then confirm that:
  - the leader starts by reading CLAUDE.md through `read_file`;
  - `task_status` shows `work_failures` and `needs_diagnosis`;
  - a re-order of a failed step without a diagnosis is refused;
  - the next order carries area knowledge.
