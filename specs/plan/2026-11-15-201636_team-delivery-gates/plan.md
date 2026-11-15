# Plan: Team delivery gates

- **Date:** 15-11-2026
- **Branch:** feat/team-delivery-gates
- **Status:** In progress — Steps 0–1 done
- **Authorization:** The user approved this plan in chat (commit WIP first; all four improvements; Codex only as a read-only reviewer).

## Context

The analysis of the "DevWS multi-agent architecture" proposal against upstream `main`
(8b1d662, PRs #23/#24, the new *reviewed* workflow) found that most of the proposal
is already built (contracts, dependencies, independent reviewer, milestone gate before
PRs). Four gaps remain, and the user chose to fix all four:

1. **Integration defects are found late**: each merge fast-forwards a reviewed
   candidate commit (`integration.rs:28`), but checks only ever ran on the lane
   head; the combined code is first tested at milestone validation.
2. **No enforced retry budget**: `attempts` is counted (`orders.rs:150-167`) but
   only prose limits it (`roles/leader.md:106`); review rounds are unbounded.
3. **Implementers can edit the tests that judge them**: `Contract.tests` is prose,
   `forbidden` never comes from the contract (`orders.rs:170`).
4. **No measurement**: the plan's own Step 10 (legacy vs reviewed replay) is open;
   no fixture, no review timing, no correction categories.
5. **Codex is blocked in reviewed mode** (`team.rs:421-425`). Codex CLI 0.169 cannot
   disable its shell tool, so per the user's choice: **Codex only for the read-only
   code-reviewer**, read-only sandbox, opt-in per task, limit documented.

First the local work in progress must be committed and upstream merged (the user chose
"commit WIP first", branch `feat/workspace-templates`).

Commits follow CLAUDE.md's commit standard: **no AI/co-author lines**.

## Step 0 — Commit WIP and merge upstream

- `git switch -c feat/workspace-templates`; inspect `git diff`, commit all current
  modified + untracked files (templates, system workspace, tests) with a descriptive
  message.
- `git merge origin/main`; resolve the 4 known conflicts: `README.md`, `install.sh`,
  `scripts/test_agentic_cross.sh`, `scripts/test_install_lifecycle.sh` (keep both
  sides' intent; show each resolution to the user in the summary).
- Run `scripts/check.sh` (or at least `cargo test` in `agentic/` plus the touched
  `scripts/test_*.sh`) and commit the merge.
- Remove the scratch worktree (`git worktree remove …/scratchpad/upstream`).
- Branch `feat/team-delivery-gates` off it. Save this plan as
  `specs/plan/2026-11-15-<time>_team-delivery-gates/plan.md` and keep its status current.

## Step 1 — Enforced attempt and review-round budget

Files: `agentic/src/task.rs`, `state.rs`, `orders.rs`, `reviews.rs`, `main.rs`,
`status.rs`, `roles/leader.md`.

- `TaskConfig.max_attempts` with `DEFAULT_MAX_ATTEMPTS = 3`, following the
  `default_max_agents` pattern (`task.rs:11-15`); update the `TaskConfig` literals
  (start.rs:234, task.rs:222, prs.rs:202, orders.rs test fixture).
- `Step.attempt_limit: Option<u32>` (`serde(default, skip_serializing_if)`), raised
  only by a **CLI-only** user command `devagentics task attempts <task> <step> <n>`
  (not in the leader's MCP catalog), so going past the budget is a human decision.
- `send_order`: before `step.attempts += 1` (`orders.rs:150`), bail when
  `attempts >= limit` with "step X had N attempts, the most this task allows:
  escalate to the user (they can allow more with `devagentics task attempts …`)".
- `reviews::request`: refuse when the step already has `limit` reviews with
  `ChangesRequested`, same message.
- `task_status` steps show `attempts`/`limit`; leader.md replaces the prose rule with
  the enforced one.
- Tests (orders.rs fixture, reviews.rs tests): third retry refused; user raise allows
  one more; old state/config loads with defaults.

## Step 2 — Protected acceptance tests

Files: `agentic/src/execution.rs`, `orders.rs`, `roles/samples/code-reviewer.md`,
`roles/standards/shared.md`.

- `Contract` gets `protected_tests: Vec<String>` (globs) and `writes_tests: bool`
  (both `serde(default)`; `deny_unknown_fields` stays).
- `Application::validate` (`execution.rs:141`): `paths(&protected_tests)`; an item
  with `protected_tests` and `!writes_tests` must list a `writes_tests` item in its
  `dependencies`, so `ready()` lands the tests first.
- `send_order` reviewed branch (`orders.rs:53-74`): for `!item.writes_tests`, add
  `item.protected_tests` to `request.forbidden`. All enforcement already reads
  `order.forbidden`: `checks::inspect` (forbidden wins), checkpoint import
  (`runner.rs:495-533`), review evidence; `amend` already preserves forbidden.
- `ensure_contract` (`orders.rs:722-763`): protected tests must be inside runner
  `source_paths` and outside `protected_paths` (implementers still read them).
- Reviewer guidance: a diff that deletes/weakens assertions, skips tests or
  special-cases fixtures is a **blocking** finding. Note in docs that existing
  installed role files are not overwritten (`roles.rs:510-513`).
- Tests: implementer order touching a protected test fails file policy; the
  test-writing item may write them; validation rejects a missing writer dependency.

## Step 3 — Check the integration candidate before review

Files: `agentic/src/check_profiles.rs`, `integration.rs`, `milestones.rs`,
`reviews.rs`, `state.rs`, `performance` call sites.

- `RepositoryChecks.candidate: Vec<String>`: profile names run on each merge
  candidate (a fast build/smoke set); validated as existing profile names. It is
  part of `check_configuration`, so it is baseline-approved like other checks.
  Optional: when empty, `workflow status`/`task_status` warn "no candidate check".
- Extract milestones.rs:136-166 (look up profile + environment, call
  `check_profiles::run_container_profile` per phase) into a shared
  `run_profiles(task, worker, repo, names, folder)`; milestones reuse it.
- New `integration::check_candidate(task, lane, candidate)`: detached worktree at
  `candidate.commit` under `task.dir()/candidate-checks/` (pattern of
  reviews.rs:158-170), `runner::prepare` with a synthetic worker
  `candidate-<short>` (never the reviewer's/author's identity), run the candidate
  profiles **without the control lock**, store
  `State.candidate_evidence[commit] = { commit, tree, configuration, phases }`,
  record a `"candidate_checks"` timing interval.
- `reviews::request`: compute the candidate and run the check before taking
  `control_lock`; under the lock recompute the candidate and require it unchanged.
  A failing check refuses the review with the failed phases; the step stays
  Reported and the leader orders a correction (category `integration_defect`).
- `Review.candidate_evidence: String` (`serde(default)`) = content hash.
  `reviews::validate` (used by `merge` and `require_approval`) and
  `integration::recover` require it to match passing, current evidence when
  candidate profiles are configured.
- Tests: validate fails for missing/failed/mismatched/stale candidate evidence;
  unconfigured → no requirement; extend the ignored Docker e2e
  (`runner.rs:1040-1289`) with a candidate profile, including a pair of lanes that
  pass alone and fail together.

## Step 4 — Benchmark: correction categories, review timing, report, fixture

Files: `agentic/src/reviews.rs`, new `agentic/src/bench.rs`, `main.rs`,
`agentic/bench/fixtures/…`, `scripts/bench_team.sh`, docs.

- `Finding.category: Option<Category>` (requirement_ambiguity, stale_plan_interface,
  scope_omission, environment_mismatch, implementation_defect, integration_defect,
  test_weakening); reviewer `review_submit` schema and prompt ask for it.
- `Review.requested_at` / `submitted_at` (serde default) and a `"review_wait"`
  timing interval.
- `bench::report(task, usage) -> Report` from state + log: elapsed, steps, first-pass
  acceptance (accepted, attempts == 1, no ChangesRequested), corrections by category,
  review rounds and wait, candidate/milestone failures, check time (aggregate vs
  union from `performance::Timing::summary`), interventions (log Escalate / Question
  to user / Approve / permission requests), tokens and cost via
  `board_usage::UsageReader::of_team`, milestone readiness.
- CLI: `devagentics bench report <task> [--json]` and `bench compare <a> <b>`
  (clap pattern at `main.rs:60`, arm next to `Command::History`).
- Fixture `agentic/bench/fixtures/notes/`: a small Python-stdlib repo with two lanes
  (`api/`, `client/`) that share a JSON contract, a `GOAL.md` (the fixed scope) and
  acceptance tests that fail on the skeleton. `scripts/bench_team.sh <fixture>
  <legacy|reviewed>` copies it into a fresh git repo and prints the `devagentics`
  start command; it never launches real agents itself (they use the user's accounts).
- Docs `docs/user/devagentics-bench.md`: protocol of ≥ 10 runs per mode on the same
  fixture, what each metric means, no speed claims from a single run.
- Tests: report from a synthetic state/log (first-pass, categories, overlapping
  intervals not double-counted, unknown history stays unknown); `scripts/test_bench.sh`
  checks the fixture copy and that its acceptance tests fail initially.

## Step 5 — Codex as a read-only reviewer (opt-in)

Files: `agentic/src/team.rs`, `worker_tools.rs`, `state.rs`, `main.rs`, `status.rs`,
docs.

- `State.codex_reviewers: bool`, set by CLI-only
  `devagentics task codex-reviewers <task> on|off` (user decision, not MCP).
- `team.rs:421-425`: allow `Program::Codex` only when the role is
  `code-reviewer`, `!role.writes`, and the task opted in; every writer and the leader
  stay Claude-only (start.rs/leader.rs gates unchanged).
- `worker_tools::codex_arguments`: `codex exec --json --ignore-user-config
  --ignore-rules --skip-git-repo-check -s read-only -C <review snapshot> -m <model>
  -c model_reasoning_effort=… -c approval_policy="never"
  -c mcp_servers.devagentics_worker.command=… -c mcp_servers.devagentics_worker.args=[…]
  <prompt>`; later turns `codex exec resume <thread> …`. Reuse `toml_string`
  (`agent_args.rs:172`). The thread id comes from the first `--json` event and is
  stored in `State.worker_sessions[agent]`. Exact flags (`approval_policy`, thread
  event name, network off in read-only) are verified against the installed CLI first.
- `panel` (`worker_tools.rs:360-375`) branches on the selected program; provider
  error lines feed `worker.rs::failure_from_record`, which already reads Codex events.
- `task_status`/board label the reviewer "codex · read-only (can read host files)";
  docs state the limit.
- Tests: argument builder flags; gate matrix (Codex writer refused, Codex reviewer
  refused without opt-in, allowed with it); session id persisted and reused.

## Step 6 — Docs and plan status

- Update `docs/architecture/devagentics-execution.md`, `docs/user/devagentics-execution.md`,
  `agentic/docs/usage.md`, `roles/leader.md` for the four features.
- Mark the matching open items of `specs/plan/2026-11-14-225804_team-throughput/plan.md`
  as done or still open, with honest status.

## Verification

- Per step, TDD: write the failing test, implement, then in `agentic/`:
  `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build`.
- `scripts/test_agentic.sh`, `scripts/test_agentic_cross.sh`, `scripts/test_agent_cli.sh`,
  new `scripts/test_bench.sh`, then the full `scripts/check.sh`.
- Docker e2e: `DEVAGENTICS_TEST_IMAGE=<image> cargo test -- --ignored` when a
  local image exists; otherwise report it as not run.
- Codex: one live `codex exec` smoke run of the reviewer adapter against a throwaway
  checkout, **only after asking the user** (it uses their Codex account).
- Benchmark: `devagentics bench report` on a synthetic task; a real legacy-vs-reviewed
  run is the user's to start (real agents and accounts).
- Commit at the end of each step on `feat/team-delivery-gates` (no AI credits); no push
  or PR without asking.
