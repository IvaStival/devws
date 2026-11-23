# Engineering workflows for devAgentics: the PStack ideas, done cheaply

- **Date** 22-11-2026 · **Branch** `feat/opencode-workers`
- **Status** In progress on branch `feat/pstack-workflows` (worktree `../devws-pstack`), rebased onto the CLI work (`d1b2661`). Done: M0–M7 (M4's system check in one container; the deterministic parts of M7) and the review fixes. Left: M8 runs (they need you). Revised 22-11-2026 after reading PStack's source skills (section 9).
- **Related plans:** builds on [token-optimization](../2026-11-21-214851_token-optimization/plan.md) and [cto-leader](../2026-11-19-200000_cto-leader/plan.md) and does not duplicate them.

## Context

You want devAgentics to deliver complete, integrated software at the lowest total cost. PStack gets good results in three ways: a router that picks the smallest playbook that fits (`poteto-mode`, about 23 playbooks), choosing the code's shape before writing it (`architect` / `arena`), and proof instead of claims (`interrogate`, verification skills, "it compiles is not evidence"). Its principles include *laziness protocol*, *guard the context window* and *encode lessons in structure*.

**The main finding: devAgentics already has most of the machinery, but it runs every item at maximum ceremony.** A one-line fix in a reviewed task pays for:
- a planner interview;
- a test-first item split;
- one independent reviewer agent per candidate;
- the full role text again on every worker turn.

So the work is mostly about **routing what exists by risk**, plus four real gaps:
1. shared interface contracts across items;
2. end-to-end proof at the application level;
3. turning failures into learning;
4. budgets.

For Arena and interrogate, you asked for other ways to get the same benefit. The proposals are in M7 and use no N-way parallel implementations.

Labels: **[C]** confirmed in the repo · **[P]** proposed · **[H]** a hypothesis that needs measuring.

---

## 0. In plain words: what each change does, and what the agents see

Each change below says what goes wrong today, what it does, and what changes in what the leader and the workers receive or send. The technical detail is in sections 3–5.

### M0: Measure before changing anything
- **Today:** we can't say what a delivered task costs. The bench adds up tokens, but never divides them by the tasks that actually worked, and it doesn't say which role spent them.
- **What it does:** the bench report gets three numbers:
  - **cost per integrated success**;
  - **tokens per role** (leader, planner, writers, reviewers);
  - **escaped defects**: problems found *after* an item was accepted.
- **What the agents see:** nothing. This is only for you. Every later change is judged by these numbers.

### M1: Stop repeating the same instructions
- **Today:**
  - A worker gets its whole role (2.7–4.5 KB) plus the shared standards again on **every** message, even though its program remembers the conversation. The standards come twice.
  - The leader gets 22 KB at start, including the legacy workflow it never uses.
- **What it does:**
  - **Workers:** the full role is sent on the first turn only. Later turns get a short reminder ("you are hammer-3, call status, here is the message"). The full role goes again after an account switch, a lost session, or every 10 turns, so it isn't forgotten.
  - **Leader:** gets only the workflow its task uses. Rarely needed chapters move out of the prompt: account limits, the OpenCode probe, test-first rules, interfaces, spikes.
  - **A new `guide` action** (leader and workers) returns one of those chapters only when it is needed, like `proposal_example` does today. Example: a worker on a bug fix calls `guide topic=bugfix` and gets "write the failing test first, then fix".
  - **Memory notes** attached to an order are cut to 1.5 KB each, with a pointer to read the rest.
- **Information flow:** the same knowledge, fetched when it's needed instead of pushed every time. The rules stay the same.

### M2: Route each item by risk (the PStack router)
- **Today:** every item goes through the whole pipeline. A typo fix still gets a planner interview, a separate test item, and its own reviewer agent.
- **What it does:**
  - **Kind.** Each baseline item says *what kind* of work it is: `change`, `bugfix`, `feature`, `interface` or `investigation`.
  - **Rigor.** The program computes a **minimum rigor** from facts in the item, with no AI guessing: does another lane use what it builds, does it touch data, auth, migrations or CI, how many files, has it already failed twice?
  - **What rigor decides:**
    - **light**: no planner interview is needed and **no reviewer agent**. The merge check must still pass, and the leader reads the diff and writes why it accepts.
    - **standard**: today's flow, with a reviewer.
    - **strict / critical**: test-first item, reviewer, and the extras from M7.
  - **Floor.** The leader can raise rigor but never lower it below the computed floor. Only you can approve going lower.
- **What the leader sees:**
  - `ready_work` and `status` show each item's kind and rigor, so it knows which steps are required.
  - `accept_step` on a light item asks for an `accept_reason` instead of a review.
- **What the workers see:** a bug-fix order says "name the regression test". Otherwise nothing changes for them.
- **Example:** "fix a typo in the README" is `change` + light: one writer, a merge check, accepted. Before, it took a planner, a writer and a reviewer.

### M3: Shared contracts between lanes (the cookie-vs-JWT problem)
- **Today:** each item writes its interfaces as free text. Nothing makes the back lane and the front lane agree, and nothing notices when one side changes.
- **What it does:**
  - **Interfaces.** The baseline gets a short list of **interfaces**, for example `auth-api v1: POST /login → Set-Cookie session`, capped at 4 KB.
  - **Provides and consumes.** Each item says which interfaces it **provides** and which it **consumes**.
  - **Versioning.** When an interface changes, its version goes up, and *both* the provider and every consumer are reset. Their old work can't be accepted against the old version.
  - **One-way decisions.** Decisions that are hard to undo (auth strategy, data format) must list 2–3 alternatives with tradeoffs before you decide.
- **What the workers see:**
  - `status` returns **only its own item plus the interfaces it touches**. Today it returns every item for its role.
  - The front worker reads the exact `auth-api v1` text the back worker builds.
- **What the leader sees:** a review or acceptance goes stale ("interface changed") instead of merging mismatched code.

### M4: "Done" means the application works
- **Today:** a milestone runs the checks together on the merged branch, but nothing starts the app or runs a user flow. Pull requests can open before any milestone passes.
- **What it does:**
  - **A `system` check.** A repo can define one that starts the application, with several services on a network that has no internet, and runs smoke or e2e tests. It runs at the milestone.
  - **Interface checks.** Interfaces with a check (contract tests) run there too.
  - **Pull requests wait.** `open_prs` is refused until the required milestones are ready.
- **The three levels:**
  - the worker finished its part;
  - the item is merged;
  - the **application** works.

  Only the last one lets PRs open.
- **What the leader sees:** a milestone failure with the failing system log. It orders a fix, the same way it handles an integration failure today.

### M5: Learn from failures, and stop at a budget
- **Today:** the leader's diagnosis of a failure stays in that task's state. The next worker in the same area makes the same mistake. Nothing limits tokens or time.
- **What it does:**
  - **Learning:**
    - When a step that failed is finally accepted, the program saves the diagnosis (cause, evidence, what fixed it) as **knowledge** for that lane. No AI turn is needed.
    - Every later order in that lane carries it automatically. This uses the existing area-knowledge feature.
    - If the same cause happens in 2+ items, the leader is told: "make this a check or a standard", so it can't happen again.
  - **Budgets:** optional `max_tokens`, `max_hours` and review-round limits per task.
    - At 80% the leader is warned.
    - At 100% the task **pauses** (nothing is lost) and you get a summary of what's left. You can raise the budget and resume.
- **What the workers see:** a "the team learned this here" block in their orders.

### M6: Use parallel workers only when they help
- **Today:** how many writers to start is decided only by the leader's prompt ("one per lane"). Nothing looks at your CPU or memory, and containers don't queue.
- **What it does:**
  - `ready_work` adds a **suggested number of writers**: the number of lanes with ready work that don't wait on each other's interfaces, capped by your machine's CPU and memory.
  - Container checks queue for a free slot instead of overloading the host.
  - The leader's rule becomes: "start with one writer; add more only when the suggestion is above 1."
- **What the leader sees:** `staffing: {suggested_writers: 2, host_slots: 3}`.

### M7: Arena and interrogate, done another way
- **Arena: "don't lock in the wrong shape".**
  - PStack builds rival versions. We get the same benefit in two cheaper ways:
    - the one-way **alternatives** from M3, written in one planner turn;
    - a **spike** when the question is a fact you can test ("is SQLite fast enough?"). One worker tries it in a scratch lane, the result is a report, and nothing is merged. Allowed only for critical items.
- **Interrogate: "attack the change before it ships".** The reviewer gets help from code instead of more reviewers:
  1. **A risk brief, computed by the program.** It lists which interfaces changed, which other lanes call this code, and whether tests were weakened (deleted asserts, new skips).
  2. **Test weakening is flagged by the program**, with no AI tokens spent.
  3. **The reviewer's job is to *break* each acceptance criterion.** A finding may include a command that reproduces it. The program runs that command, and findings that don't reproduce are marked *unconfirmed*, so the leader doesn't loop on false alarms.
  4. **The reviewer runs on a different program than the author** (e.g. a Codex reviewer for Claude's code) when one is available, at no extra cost.
  5. **A second reviewer only when there is a signal:** a critical item, two "changes requested", or an integration failure.

### M8: Compare and remove what doesn't pay
- Run the same fixtures (a one-file bug fix, the two-lane Notes app, publisher-gaps) with the old behavior and the new one.
- Keep a mechanism only if it lowers **cost per working task** without more failures or escaped defects.

---

## 1. The architecture today (verified)

| Capability | Where | Notes |
|---|---|---|
| Two workflows only, `Reviewed` (default) and `Legacy` | `execution.rs:15`, `main.rs:575` | [C] No task kind, size class or fast path |
| Baseline = `Application{goal, non_goals, decisions, scopes, items: Contract, milestones, check_configuration}` | `execution.rs:23` | [C] Revisions go up by one (`store` :521); `amend` diffs (:635); `confirm` resets only items whose contract changed (:678) |
| `Contract` per item: `role, repo, lane, dependencies, allowed, supporting_files, reuse, interfaces, behavior, errors, tests, profiles, acceptance, data_effects, protected_tests, writes_tests` | `execution.rs:47` | [C] `interfaces` is free text. **No item-to-item interface link and no versioned shared contract** |
| `Decision{question, recommendation, prerequisites, answer}` | `execution.rs:37` | [C] Interview questions only. No alternatives, not versioned |
| Order binding | `orders.rs:339` `contract_bindings`; `reviews::validate` :448 marks a review stale on drift | [C] Reusable for interface binding |
| Independent review **for every writing step** | `reviews::require_approval` :639 ← `orders::accept_step` :909; again in `lanes::merge_into_task` :229-254 | [C] No risk tier. Reviewer must be a live `code-reviewer`, not the author |
| Candidate (merge) check before review | `reviews::check_candidate_first` :164, `integration::check_candidate` :307 | [C] Whole suite once when the order used `targets` |
| Milestone = union of the items' profiles on the integrated branch | `milestones::validate` :111 | [C] **No services and no e2e.** `--network=none` (`runner.rs:1080`). `open_prs` does not wait for milestones (only the prompt asks) |
| Planner optional in code; leader may `propose` itself | `workflow.rs:371-388` | [C] `leader.md:88` always staffs a planner |
| Test-first split | `leader.md:120` | [C] Prompt only. Code validates it only when `protected_tests` is set (`execution.rs:371`) |
| Failure history, diagnosis before retry, same-cause refusal, attempts (5) | `failures.rs` (`record`, `resolve` :279), `watch::retry_spent_steps` :169 | [C] **Nothing reaches memory automatically.** Failures stay in `state.step_failures` |
| Area knowledge in orders | `notes::knowledge_for` :161 (≤6), `about_step` (≤5), `fenced` :199 | [C] Number of notes capped, bodies not truncated |
| Limits | `max_agents` 3 per writes-class (`start.rs:249`), one writer per lane, containers ≤4 GiB / ≤4 CPU | [C] No token, time or host-capacity budget, and no container concurrency limit |
| Worker prompt per turn | `worker_tools::panel` :1236-1266 | [C] **Role + `shared.md` resent on every resumed turn.** `shared.md` is also baked into the provisioned role (`roles::provision_specialists` :442), so it goes twice (~2.5 KB) |
| Worker `status` | `worker_tools::call("status")` :824 | [C] Returns every contract matching role and repo, not only its own |
| Leader prompt | `leader.md` 22 KB, `include_str!` `roles.rs:10`, sent as the first user turn (`start::opening` :76) and on every leader move | [C] Reviewed (6.4 KB) **and** legacy (4.0 KB) workflows both sent |
| Leader catalog | 36 tools, ~21.9 KB, static `tools()` `mcp.rs:42` | [C] `listChanged` is advertised but only used on binary replace |
| On-demand detail | `proposal_example` (`execution.rs:460`), `status section=…`, `plan_read`, `memory_search` | [C] Our template for lazy loading. **`proposal-example.json` is untracked but `include_str!`'d: commit it or a clean build fails** |
| Measurement | `telemetry.rs` (opt-in, prompt-segment bytes), `bench.rs` report/compare, reconciled Codex baseline ($4.56/run) | [C] No cost per success. `by_role` is collapsed (`bench.rs:~214`) |

## 2. PStack mapped to devAgentics

| PStack idea | devAgentics today | Plan |
|---|---|---|
| Router picks the smallest playbook | One workflow, maximum ceremony | **M2:** per-item `kind` + a deterministic `rigor` floor. The code decides the stages, not a prompt |
| architect: shape before code | Baseline contracts per item | **M3:** versioned `interfaces` that items provide and consume |
| arena: rival builds | — | **M7:** decision alternatives + an optional time-boxed spike, not N builds |
| interrogate: multi-model attack | One reviewer, moderately adversarial | **M7:** falsification review with a deterministic risk brief, repro-verified findings and provider diversity |
| swarm: one worker per uniform unit | Lanes, `max_agents` | **M6:** staffing advice from ready independent lanes and host capacity |
| verification skill / prove-it-works | Profiles, candidate check, milestone | **M4:** milestone `system` profile (app starts, smoke/e2e) |
| reflect / correct: encode lessons | Diagnosis rule | **M5:** auto knowledge note on resolution; recurring cause → "make it a check" |
| guard-the-context-window, lazy skills | `proposal_example`, sections | **M1:** send role text on the first turn only; `guide` topics on demand |
| Fresh helper per round | Reuse workers (prompt) | Not adopted: our session resume is cheaper [H, measure in M8] |

## 3. Target design (the minimum)

### 3.1 Routing: item kind + rigor (M2)

The planner or leader **declares** `kind` in the contract (a few tokens). The **code computes** the minimum rigor, so no LLM classifies anything.

- `Contract.kind`: `change | bugfix | feature | interface | investigation`. With serde, a missing kind defaults to `feature`, which means today's behavior.
- `Contract.rigor: Option<Rigor>` with `light < standard < strict < critical`. Effective rigor = `max(declared, rigor_floor(item, application, failures))`.
- `execution::rigor_floor` (new, pure function, tested):
  - **strict** if any of these holds:
    - the item provides or consumes an interface used across lanes or repos;
    - `data_effects` is non-empty;
    - it touches `sensitive_paths`: a new `RepositoryChecks` / runner field, defaulting to auth, migrations, `*.sql`, Dockerfile, CI, dependency manifests;
    - the item already has ≥2 work failures.
  - **light** only if: one lane, no interfaces provided or consumed, no data effects, no sensitive paths, `allowed` ≤ 3 globs, and kind `change` or `bugfix`.
  - **standard** otherwise.
  - **critical** is declared only; never computed.
- Declaring rigor **below** the floor is impossible. Raising it is free. Lowering an approved item's rigor goes through `user_approval_needed` (`execution.rs:858`), like weakening a check.

**What rigor gates:**

| Stage | light | standard | strict | critical |
|---|---|---|---|---|
| Planner interview | leader may propose the baseline itself | leader's choice | planner | planner |
| Test-first split item | no | no (tests in the same item) | yes | yes |
| Bugfix repro | regression test named in `tests` | same | same | same |
| Candidate check | yes | yes | yes | yes |
| Independent reviewer agent | **no**: the leader reads `git_diff` and records an `accept_reason` | yes | yes, falsification brief (M7) | yes + second opinion on triggers (M7) |
| Interface conformance at milestone | — | if declared | required | required |

- **Code changes:**
  - `reviews::require_approval` and `lanes::merge_into_task` skip the review requirement only when effective rigor is `light` **and** candidate evidence passed. `accept_step` then requires `accept_reason` (logged through `record_decision`, mcp.rs:~543).
  - `kind == bugfix` validation requires at least one `tests` entry naming a regression test.
  - Each item's rigor is shown in `ready_work` and in `status`.
- **Task-level route (prompt + one example).** For ≤3 items in one lane and no one-way decisions, the leader proposes the baseline itself. There is a second, small `proposal_example` variant (`light`). Your first-baseline confirm and approve stay mandatory [C].
- **Rollback switch:** `task.toml` `routing = "off"` forces every floor to `standard`, which is exactly today's gates.

### 3.2 Shared interface contracts (M3)

Short, versioned, and part of the baseline that already exists. No new store.

- `Application.interfaces: BTreeMap<id, Interface{version: u32, owner: item_id, kind: http|function|event|schema|cli|config, spec: String (≤4 KB; OpenAPI fragment, JSON schema or signatures where practical), check: Option<profile>}>`.
- `Contract.provides: Vec<id>`, `Contract.consumes: Vec<id>`.
- **Validation** (`Application::validate`):
  - each interface has exactly one owner;
  - a consumer depends on the owner, or on an `interface`-kind item;
  - no dangling ids.
- `Decision` gains `one_way: bool` and `alternatives: Vec<{option, tradeoffs, cost}>`. A `one_way` decision needs at least two alternatives before it resolves. The auth strategy is a decision; the auth API is an interface.
- **Binding:** `contract_bindings` also stores the specs of the interfaces the item consumes and provides. `reviews::validate` then sees interface drift as stale, with no new mechanism.
- **Revisions:** `confirm` adds to `affected` every item that provides or consumes an interface whose `spec` changed. The version goes up automatically. This is the cookie-vs-JWT fix: a changed contract resets both sides.
- **Scoped retrieval:** worker `status` returns **only its own contract and its interfaces**. Today it returns every matching contract. The spec text is inlined there because it is small and the worker needs it.
- **Integration:** an interface with a `check` profile runs in `milestones::validate` on the integrated branch.

### 3.3 Three levels of "done" (M4)

| Level | Means | Gate |
|---|---|---|
| Worker done | `report` → `Reported` | [C] committed, allowed files, evidence |
| Item done | `accept_step` → `Accepted` | [C] candidate check + review per rigor (M2) |
| Application done | milestone ready | [P] union profiles + interface checks + **system profile** |

- **New `system` check kind** in `check_profiles.rs`. It runs only in `milestones::validate`:
  - a command that starts the app and runs smoke or e2e tests in one container; or
  - an optional compose spec with a Docker network created `--internal` (no egress).

  The evidence is reused with the same hash rules. Still no external network or live data [C invariant].
- `prs::open_prs` refuses until every required milestone is `ready`. Today only the prompt asks for this.

### 3.4 Failure learning and budgets (M5)

- **Learning:**
  - When a step with diagnosed failures is accepted, `failures` writes one `knowledge` note per diagnosed cause. It is deterministic from `Diagnosis{cause, evidence, change}` with `area` = lane, so the next orders there carry it through `knowledge_for`. No LLM turn.
  - When the same `Cause` recurs across ≥2 items in an area, the leader's inbox gets "encode this as a check or standard", PStack's `correct`.
  - The ≥2 work-failure rule raises rigor (3.1), so repeat failures get a reviewer.
- **Budgets** (`task.toml [budget]`, all optional):
  - `max_tokens`: summed from reconciled worker-stream usage (v2 delta) plus leader pane usage.
  - `max_hours`.
  - `max_review_rounds` per item; the default is the existing attempts.
  - `max_writers`: an alias of the existing `max_agents`.
  - At 80% the leader is told. At 100% the dispatcher calls the existing `pause_task` and escalates with the `notes::digest` of what remains. Progress is kept and nothing is dropped.
- [H] The token sum is an estimate: provider coverage differs (token plan §Measurements).

### 3.5 Staffing and scheduling (M6): deterministic, not an LLM scheduler

- `workflow::ready_work` adds `staffing: {independent_lanes_ready, suggested_writers, host_slots}`:
  - `suggested_writers` = min(lanes with ready items and no interface dependency between them, `max_agents`, host_slots).
  - `host_slots` = min(`available_parallelism` / profile CPUs, MemAvailable / profile memory).
- **Runner container slots:** a file-lock counting semaphore under `~/.devagentics/runner-slots/`, sized by `host_slots`. A queued check waits and is not refused.
- Prompt rule: one writer by default. Add one only when `suggested_writers` > 1. Before fanning out, the leader writes a **throughput checkpoint** (from PStack's Feature playbook): blocking first steps, independent workstreams, shared mutable state, smallest safe decomposition ("if one worker is best, say why"). It goes in its summary, not in a new tool.

### 3.6 Token strategy (M1, plus what M2–M3 bring)

| Change | Source | Expected |
|---|---|---|
| Send role + standards only on a session's first turn; resumed turns get a ≤300 B re-anchor (agent id, "call status", the message). Full text again after a session reset, an account fallback, or every N=10 turns | `worker_tools::panel` :1236 | [H] ~3–7 KB less input per resumed turn. Measure drift: `prompt_bytes` telemetry exists |
| Stop sending `shared.md` twice | `panel` vs `provision_specialists` | [C] −2.5 KB per turn |
| Leader gets only its workflow's section (reviewed **or** legacy). Rarely used parts move to `application_workflow action=guide topic=…`: accounts, OpenCode probe, test-first, interfaces, decisions/spikes, falsification | `leader.md`, `start::opening`, `workflow.rs` (like `proposal_example`) | [C] −4 KB for sure; [H] −8–10 KB total |
| Worker `guide` action for the playbook of its kind (`bugfix`: repro first; `interface`: spec rules) | `worker_tools::call` | Lazy skills. Levels 1/2/3 = core prompt / kind guide / `proposal_example`-style detail |
| Worker `status` scoped to its own contract | 3.2 | [H] large for many-item baselines |
| `fenced` notes capped at 1.5 KB each, with a `memory_search` reference for the rest | `notes.rs:199` | [H] |
| The light tier removes a reviewer agent and the planner interview | 3.1 | [H] the largest saving per small task |
| CLI vs MCP | — | **Not in this plan.** It stays gated in the token plan (variant B). Measured savings so far favor targeted outputs (`task_status view=progress`: −97%) over transport. Phase-filtered `tools/list` only if M8 shows catalog cost matters |

## 4. M7: Arena and interrogate, rethought

You asked for the benefit without PStack's machinery. The benefit of **arena** is not committing to the wrong shape on a one-way door. The benefit of **interrogate** is an independent, skeptical view that finds real defects. Cheaper ways to get each:

**Decisions instead of rival builds (the arena idea)**
1. `one_way` decisions need 2–3 `alternatives` with tradeoffs and a cost estimate, written by the planner in its normal proposal turn. That is one agent and a few hundred tokens instead of N implementations. You pick an alternative in the interview that already exists [C flow].
2. **Spike only when it depends on a fact you can test.** PStack's own rule is "run something, don't debate". A `spike` item kind is read-only for the task branch: one worker, a scratch lane, a time box, and its findings become a `report`. It is never merged (`close_step` [C]). At most one per decision, and only at `critical`.
3. The chosen alternative is stored with the decision and bound into interfaces (3.2), so it is not forgotten later.

**Falsification review instead of a multi-model attack (the interrogate idea)**
1. **Deterministic risk brief** in `review_diff`, computed with no LLM:
   - changed interfaces it provides or consumes;
   - callers in other lanes (`graph` when on, else `search_code`);
   - test diff signals: deleted asserts, new skips, protected tests touched (a small diff scanner in `reviews.rs`);
   - the acceptance list.

   The reviewer is told to *try to falsify each acceptance criterion*, with no nitpicks unless a finding is a bug or regression.
2. **Test weakening found by code.** The scanner's hits become automatic blocking findings, so no tokens are spent on them.
3. **Findings with repros.** A blocking finding may carry a `repro` command. The runner reruns it on the candidate. One that doesn't reproduce is marked `unconfirmed` for the leader. This cuts false positives and the review loops they cause.
4. **Diversity for free.** `spawn_agent` prefers a reviewer on a different program than the author when one is available. Same cost, different blind spots.
5. **Second opinion only on triggers:** `critical` rigor, or ≥2 `ChangesRequested` on the item, or an `IntegrationFailed`. That is one extra reviewer at most, under the budget.

[H] Each of these must show in M8 that it lowers escaped defects or review rounds. Any that doesn't is removed.

## 5. Milestones

Each milestone is test-first. `cargo test --offline`, `cargo clippy --all-targets -D warnings` and `cargo fmt --check` (`agentic/`), plus `make check`, pass before the next one. Plan status is kept current.

| # | Work (files) | Acceptance |
|---|---|---|
| **M0** Metrics | `bench.rs`: `cost_per_integrated_success` (spend over all runs ÷ runs with milestones ready); keep `by_role` from `board_usage`; per-rigor counts; review rounds; **leader responses per accepted item**; **escaped defects** (failures found at candidate, milestone or later review on items accepted earlier). Commit `proposal-example.json` | The report on the saved Codex Notes run shows the new fields. Missing pricing stays `null`, not 0 |
| **M1** Prompt diet | `worker_tools::panel` first-turn-only role (Claude `resumed`, Codex thread, OpenCode session); remove the double `shared.md`; split `leader.md` into core + reviewed + legacy; resumed turns point at the provisioned role file instead of inlining it; worker rule "the order's bar is the whole bar"; picked in `start::opening` / `leader.rs` take_over; `guide` topics (leader `application_workflow`, worker action); cap `notes::fenced` | Telemetry: resumed-turn `prompt_bytes` ≤ 25% of the first turn. Tests: full text after fallback, reset and every 10th turn; legacy leader gets legacy text; `guide` serves every topic; role tool-name test passes |
| **M2** Routing | `execution.rs` `Kind`, `Rigor`, `rigor_floor`, validation, `user_approval_needed`; `reviews::require_approval`, `lanes::merge_into_task`, `orders::accept_step` (`accept_reason`); `workflow::ready_work`, `status.rs`; `task.toml` `routing`, `sensitive_paths`; `leader.md` routing table; light `proposal_example` | Tests: a light item merges with no reviewer only when candidate evidence passed and a reason is given; standard still needs a review; a declared light below the floor is refused; a 2nd work failure raises rigor; `routing=off` matches today; an old `state.json` loads as feature/standard |
| **M3** Interfaces | `execution.rs` `Interface`, `provides` / `consumes`, `Decision.one_way` / `alternatives` / `criteria`, two-way decisions default to the recommendation, `confirm` affected set; `orders.rs` binding; `reviews::validate`; worker `status` scoping | Tests: an interface spec change resets provider and consumers and bumps the version; a two-way decision needs no user answer, but a one-way one does; a consumer without the dependency is refused; a bound order goes stale on drift; worker status lists only its own contract and interfaces |
| **M4** App done | `check_profiles.rs` `system` kind (launch, ready, drive, evidence, cleanup) with a base run recorded once; `milestones::validate` runs system and interface checks; internal-only compose network in `runner.rs`; `prs::open_prs` milestone gate | Tests: a failing system check keeps the milestone unready; a check that could not run is `inconclusive` and never counts as a pass; `open_prs` is refused before ready; the network is `--internal` (no egress test). Notes fixture gets a smoke system profile |
| **M5** Learning + budgets | `failures.rs` → `notes` knowledge on resolution; recurrence inbox line (`workflow::leader_inbox`) with the `correct` order of fixes; a new check counts only once it fails on the recorded failing commit; `Diagnosis.premise` required on the 2nd failure at the same gate; deterministic end-of-task **Attention** list; `task.rs` `[budget]`; `watch.rs` 80% / 100% handling via `pause_task` + escalate | Tests: an accepted diagnosed step creates one knowledge note that the next order in the lane carries; a budget breach pauses and escalates once with the remaining items; no budget = no behavior change |
| **M6** Staffing | `workflow::ready_work` staffing; host capacity; `runner.rs` slots as N separate lock files | Tests: lanes joined by an interface dependency count as one; slots queue rather than fail; `max_agents` is still enforced |
| **M7** Decisions + falsification | `execution.rs` spike kind; `reviews.rs` risk brief with the reviewer's safety fact, test-weakening scanner, evidence levels on findings, repro rerun, pre-existing findings non-blocking; `team::spawn_agent` reviewer program preference; second-opinion triggers | Tests: deleted asserts → automatic blocking finding; a non-reproducing repro → `unconfirmed`; a finding outside the diff's hunks → non-blocking `pre_existing`; a blocking finding with only level-1 evidence is refused; a one-way decision without 2 alternatives does not resolve; a spike is never merged |
| **M8** Benchmark + prune | `agentic/bench/fixtures`: add `bugfix-one-file`; use `notes` (two lanes, multi-component) and `publisher-gaps`; arms A (current, `routing=off`, M1 off) vs B (M1–M6); M7 as arm C on strict items | Decision rule and noise floor written before the first run (explain-the-number). ≥5 paired runs per scenario to start (sized from variance, method as in the token plan). Pass = B lowers cost per integrated success, with no fall in completion and no rise in escaped defects. Any mechanism without a measured gain is removed |

Order: M0 → M1 (the cheapest saving, no change to gates) → M2 → M3 → M4 → M5 → M6 → M7 → M8. M3 does not depend on M2 and can go in parallel if useful.

## 6. Risks and tradeoffs

- **Light items skip review, so defects can slip through.** Mitigations:
  - the floor is computed by code, not chosen;
  - the candidate check is always required;
  - the leader must give an accept reason;
  - escaped-defect metric (M0), with a fallback of `routing=off`;
  - your memory rule "never self-approve" still holds for standard and above. Light is an explicit, user-chosen exemption.
- **Role text sent once can drift after compaction or a long session.** An agent can't notice that a rule is missing, so the design never relies on it asking. Mitigations:
  - the program resends when it knows the context is gone (a new session or thread, an account fallback) and on every Nth turn;
  - a `bugfix` or `interface` order names or includes its `guide` chapter;
  - the important rules are enforced by code (regression test required, candidate check, allowlist, test-weakening scanner);
  - drift is measured in M8 as rule violations and review rounds, A vs B.
- **Interface specs become paperwork.** They are capped at 4 KB, required only when an interface crosses a lane or repo, and checked only when a `check` is given.
- **System profile.** Mitigation: offline only, opt-in per repo, evidence reused. Composing services adds runner code: internal network only, same resource caps.
- **Budgets can stop good work.** Mitigation: they pause and never fail; you resume with a raised budget.
- **Schema growth.** Every new field has a serde default, so the old state loads unchanged in the new binary. **Downgrade is not silent** [C, corrected in review]: `Application`, `Contract`, `Decision`, `Diagnosis`, `Review` and `Finding` use `deny_unknown_fields`, so an older binary *refuses* a `state.json` with the new fields. Drain and archive reviewed tasks before rolling back the binary. For a rollback without a downgrade, use `routing = "off"`.

## 7. Compatibility invariants (kept)

Claude, Codex and OpenCode adapters and session resume; `Task::transaction` locking; Docker snapshots and `--network=none` for orders and candidate checks; allowlists and checkpoint validation; evidence hashing; diagnosis rules; ai-memory; legacy tasks untouched (all new gates apply only to `Workflow::Reviewed`); a single source of truth (`state.json` + baseline).

## 8. Verification

- **Unit and behavior tests** per milestone, as above, written first.
- **Fixture replay:** start a reviewed `notes` task in an isolated `DEVAGENTICS_HOME` with telemetry on, then compare `prompt_bytes`, review count and cost per success against the saved Codex baseline (`codex-baseline.json`).
- **End to end:** `DEVWS_ROOT=$PWD config/tmuxinator/rust_tools.sh install --rebuild`, then run `bugfix-one-file` and confirm all of the following:
  - the leader proposes a light baseline itself;
  - no reviewer is spawned;
  - candidate check plus accept reason;
  - the milestone is ready;
  - `open_prs` is allowed.

  Run `notes` and confirm the interface spec is bound and a forced spec change resets both lanes.

## 9. Review against PStack's source skills (22-11-2026)

The first draft used the PStack map page and the 22 playbooks. This review reads the skills themselves in `cursor/plugins/pstack` (last change 19-11-2026):
- the router: `poteto-mode/SKILL.md` and `worker.md`;
- `architect`, `arena`, `interrogate`, `swarm`, `blast-radius`, `figure-it-out`, `correct`, `reflect`, `recall`, `show-me-your-work`, `create-verification-skill`;
- all 24 `principle-*` skills;
- guide chapters 02–10.

### 9.1 What the source confirms (no change)

| Plan choice | PStack source |
|---|---|
| Rigor tiers computed from facts (M2) | figure-it-out: "rigor is gates and artifacts, not 'try harder'"; one-way doors and high blast radius get more |
| Non-author verdict for standard+ (M2) | Shipping: "the agent that judges a change is never the one that wrote it"; autopilot: "no owner merges on its own verdict" |
| Written alternatives, not a second build (M7) | figure-it-out: "a second arena over a settled design is over-engineering"; architect sketches two shapes before code |
| Spike for facts (M7) | poteto-mode: "a fork you could settle by running something → prototype it instead of asking" |
| Recurring cause → check (M5) | correct: "a class counts once it has happened twice"; Encode Lessons in Structure: "the instruction is the symptom" |
| Prompt diet (M1) | PStack itself shrank its skills on 16-11-2026 (#1249, "shorter skill texts, worker.md") |
| One writer by default (M6) | figure-it-out: "parallelize only across seams… don't over-fan" |

### 9.2 What changes in the plan

1. **M0: count leader round trips, not only bytes.** [C, from the saved Notes run]
   - **The data:** the leader made 202 responses, read 19.85M cached tokens, and its largest request was 173k tokens. That is 61% of the $4.56.
   - **What it means:** each leader tool call re-reads the whole leader context, so the leader's cost grows with *how many calls* it makes per item, more than with prompt text.
   - **Change:** M0 adds **leader responses per accepted item**. M2's main value is expected to be fewer leader round trips (no reviewer to spawn, brief, chase and accept). [H] This is measured in M8.
   - **Not in this plan:** rotating the leader's session stays gated in the token plan (P2).
2. **M1: point at the rules instead of resending them** (PStack `worker.md`).
   - PStack's kickoff is one line ("Read `<path>/worker.md` before anything else") plus the goal and the task.
   - devAgentics already writes the role files to `~/.devagentics/roles/`. A resumed turn's re-anchor names that file instead of inlining it.
   - Add one worker rule from `worker.md`: "the bar in your order is the whole bar; add no check, gate or review round above it." It stops scope creep, which costs extra turns.
3. **M7: findings need evidence; unproven findings don't block** (interrogate, blast-radius).
   - interrogate keeps a finding only "when a run proves it". blast-radius ranks evidence: 1 you said so, 2 you pointed at the line, 3 you walked the failure, 4 you ran it, 5 you reproduced it in the app.
   - **Adaptation:** a blocking `Finding` needs level ≥ 2 (`file` and `line` already exist [C]). Level 1 is refused as blocking. A finding with a `repro` (level 4) is rerun by the runner, and if it doesn't reproduce it becomes `unconfirmed`.
   - **Pre-existing problems don't block** (worker.md: "report a problem the base already has in one line"). If a finding's `file:line` is outside the diff's hunks, the code marks it `pre_existing` and it is non-blocking. This is deterministic and cuts review loops.
   - **The safety fact** (blast-radius: "find the one fact it's safe because of"). The risk brief asks the reviewer to state the single fact the change's safety rests on, and its evidence level. It's one line in the verdict, not a new agent.
4. **M3 and M7: judge alternatives with criteria; stop asking the user about reversible choices** (arena Phase A/D, Never Block on the Human).
   - **Criteria:** a one-way decision gets 3–6 `criteria` written before its alternatives, and the pick is scored per criterion. The tie-breaker comes from architect: each piece of state has one owner, and callers need to know least.
   - **Reversible choices:** today every planning `Decision` waits for *your* answer (`workflow.rs:334`, "user answered planning question"). After this change, **only `one_way` decisions need your answer**. Two-way ones take the recommendation and are listed at baseline confirm, where you can still change them. Fewer interview rounds, and you keep the final confirm.
5. **M4: build the system check like a verify skill, and see it fail once** (create-verification-skill, Prove It Works).
   - **Structure:** the profile has five parts: launch, ready (PStack's "doctor"), drive, evidence, cleanup.
   - **See it fail:** "a check you've never seen fail proves nothing." When a system profile is added, it runs once on the task's base commit and the result is stored. The milestone then reads as *old vs new*: a new flow should fail on base and pass on the task branch.
   - **Inconclusive:** a check that could not run (environment, timeout before start) is `inconclusive`, never a pass (figure-it-out). [H] Confirm how today's `Environment` cause is reported, and add the state only if it is missing.
6. **M5: fix repeats in PStack's `correct` order; attack the premise; make the end-of-task brief deterministic.**
   - **Recurrence line:** the inbox line now says to fix at the highest level that works: architecture, then types, then a check whose error names the fix, then a test, then docs last. A new check counts as the fix only after devAgentics reruns it on the recorded failing commit and sees it fail.
   - **Attack the premise:** when a step fails a second time at the same gate, the next `Diagnosis` must add a `premise` (the one assumption both attempts shared) and what tested it. Today's refusal of the same cause and approach [C] stays.
   - **Attention list** (show-me-your-work, ketchup, without their extra reviewer model): at pause, budget stop or completion, you get a list computed from state:
     - light items accepted on a leader reason;
     - unconfirmed or dismissed findings;
     - rigor raised;
     - inconclusive checks;
     - budget used.

     The decisions logged through `record_decision` already form the trail.
7. **M6: separate slots, not a shared counter** (Separate Before Serializing Shared State). The runner takes one of N lock files (`slot-0..N-1`), not a counter file all checks write.
8. **M8: write the decision before measuring** (Explain the Number, benchmark-checklist).
   - Write down the pass rule and the noise floor before arm A runs.
   - Keep the run count, spread and limiter with each number.
   - An effect inside the run-to-run spread counts as "no measurable difference", and that mechanism is removed.
9. **Correction to section 6 (rollback)** [C]: older binaries refuse new fields (`deny_unknown_fields`), so they don't drop them silently. The risk text above is fixed.

### 9.3 Considered and not adopted

| PStack idea | Why not here |
|---|---|
| Fresh subagent for every fix round | Workers are cheap here (the writer was 2% of the Notes run), and resume keeps the checkout and context. The leader is the expensive context, and its rotation is gated in the token plan. [H] M8 can test a fresh retry worker |
| Three-vendor panels and a cross-judge on every arena or interrogate | Several times the review cost. We keep one reviewer, prefer a different program, and add a second only on triggers (M7) |
| Model level per role ("match the model to the job") | devAgentics already chooses an agent per role in config. Mapping rigor to model level is possible later, but nothing shows it is needed yet |
| Formal verification, statecharts | Valuable for concurrency-heavy code, but outside the cost and scope goal of this plan |
| Babysit, Shipping, Autopilot (merge automation) | devAgentics stops at `open_prs`, and merging stays your decision |
| "Never ask the human" for everything reversible | Your baseline confirm and approval gates stay. Only reversible *planning questions* stop asking (9.2 point 4) |
| `reflect` (skill edits from a transcript) | Our equivalent is M5's automatic knowledge notes plus the `correct`-ordered recurrence line, which needs no transcript-mining turn |

## 10. Working beside the CLI work (23-11-2026)

A Codex session is moving the leader from MCP to the CLI in the main working tree. It has `leader-read <task> progress|order <id>`, gated by `state.leader_cli`, and its write paths stay on MCP. This plan runs in parallel without touching that tree.

**Isolation.**
- The branch `feat/pstack-workflows` lives in its own worktree, `../devws-pstack`.
- Its base is a snapshot commit (`0553df5`) of the uncommitted tree, built with a temporary index. The main tree's files and index are unchanged.
- **Integration:** once the CLI work is committed, run `git rebase --onto <that commit> 0553df5 feat/pstack-workflows`. This replays only this plan's commits.

**Files left alone until the CLI work lands:** `main.rs`, `mcp.rs`, `worker_tools.rs`, `start.rs`, `task.rs`, `state.rs`, `check_profiles.rs`, `tests/flow.rs`. So the order is:
- **Now:** M0 (`bench.rs`, done) and the pure parts of M2, M3, M5 and M7 (`execution.rs`, `reviews.rs`, `failures.rs`, `notes.rs`, `workflow.rs`).
- **After the CLI work lands:** M1 (`worker_tools.rs`, `start.rs`, `leader.md`), plus the MCP and CLI wiring of every milestone.

**The CLI rule for this plan.** It is the same rule the token plan uses:
- **Reads:** every new leader read is one function that returns JSON in its own module. Both the MCP tool and a `leader-read` view print that same value, as `status::progress` does today. That covers:
  - kind, rigor and required steps per item;
  - staffing;
  - `guide` topics;
  - the Attention list;
  - the risk brief.

  Proposed views: `leader-read <task> ready`, `guide <topic>` and `attention`. Parity tests compare the CLI and MCP JSON, as the CLI pilot already does.
- **Writes stay on MCP**, where authorization and the gates live: `accept_reason`, proposals with `kind` and `rigor`, budgets, finding repros.
- **Workers keep the worker MCP.** That follows the token plan's variant B, so the worker `guide` stays a worker MCP action.
- **Telemetry:** `leader-read` does not record a telemetry observation yet. So M0's "leader MCP calls per accepted step" misses CLI reads. Add the observation in `main.rs` when the CLI work lands. The bench line says "MCP" until then.

**M0 result (23-11-2026).** `bench report` now shows:
- integrated (every milestone ready);
- cost per integrated success (only with complete pricing; otherwise "unknown" or "not integrated");
- cost by role with share;
- review rounds per reviewed step;
- escaped defects, a lower bound: work failures recorded on a step after it was first accepted;
- leader MCP calls per accepted step (from telemetry).

The JSON report carries the same fields. Six new tests pass. `cargo test` (508), `clippy -D warnings` and `fmt --check` are green.

Checked on two real tasks:
- `feat-system-observer`: 63 of 86 steps accepted, 1.6 review rounds per reviewed step, 0 escaped defects, not integrated.
- `bench-reviewed-20261115-230210`.

**Still open:** the saved Codex Notes baseline is a usage file, not a task directory, so the "report on the saved Notes run" acceptance waits for a fresh Notes run with telemetry on. Per-rigor counts come with M2.

**M2 result (23-11-2026).**

*What was added:*
- `Contract.kind` (change, bugfix, feature, interface, investigation; the default is feature) and an optional `Contract.rigor`.
- `Application::routing(id, work_failures)`, a pure function. It gives strict on sensitive paths, data effects or ≥2 work failures. It gives light for a change or bugfix in ≤3 explicit globs that no item in another lane depends on. Everything else is standard. A declared rigor only raises it.
- `task.toml` `routing = "off"` keeps every item at standard or above.

*What the agents see:*
- `ready_work` and the inbox show `kind`, `rigor`, `required` and `rigor_why`.
- A declared rigor below the floor is refused at propose time.
- Lowering an approved item's rigor needs the user.

*How a light item is accepted:*
- `reviews::accept_light` runs the candidate check. It needs the check to exist and pass.
- It then records an approved review signed by the leader with `accept_reason`, bound like any review.
- `validate` rechecks that the item is still light, so a later failure or a new sensitive path makes that approval stale.
- The leader's `accept_step` takes `accept_reason`.

*Other changes:*
- The review binding was extracted from `reviews::request` into `bind`, shared by both paths.
- The bench counts light acceptances apart from independent reviews and shows items by rigor.
- `leader.md` explains routing: propose small work yourself, `accept_reason` for light items, test-first items only for strict and critical.
- `proposal-example.json` shows `kind`.

*Verification:*
- `cargo test` passes: 520 tests, including 12 new routing, light-acceptance and bench tests.
- The four Docker tests pass. One is new: `a_light_item_merges_on_its_candidate_check_and_the_leaders_reason` (candidate check, leader-signed approval, merge, no reviewer). The existing independent-review lifecycle also passes after the `bind` refactor.
- `clippy -D warnings` and `fmt --check` are green.
- `make check` has two failures, `test_guide` and `test_workspace_template`. Both fail the same way on the untouched snapshot `0553df5`, so they are not caused by this work.

*Not done:*
- No new bugfix rule. Every item already needs a non-empty `tests` list, and a "names a regression test" check would be guesswork.
- ~~No light `proposal_example`~~. Done 23-11-2026: `application_workflow action=proposal_example` with `input.variant = "light"` returns a one-item baseline (`proposal-example-light.json`). Tests check that it validates and routes light, and that the leader gets it by variant.

*Touches files of the CLI work* (small hunks, for the rebase):
- `mcp.rs`: the `accept_step` schema and arm.
- `runner.rs`: test only, one new Docker case.
- `start.rs`, `check_profiles.rs`, `task.rs`: `routing` in `TaskConfig` and its literals.

**M3 result (23-11-2026).**

*What was added:*
- `Application.interfaces`: per interface an id, `owner` item, `kind` (http, function, event, schema, cli or config), `spec` (≤4 KB) and an optional `check` profile. Old baselines write nothing new.
- `Contract.provides` and `Contract.consumes`.

*What validation refuses:*
- an unknown owner or interface;
- an owner that doesn't list the interface in `provides`;
- a second provider;
- a consumer that doesn't depend on the owner;
- a spec over 4 KB;
- an unknown check profile.

*Versions and resets:*
- `store` sets versions itself: an unchanged spec keeps its version, a changed one goes up by one, a new one starts at 1. The planner never writes versions.
- A changed or removed spec marks the provider and every consumer as affected. `confirm` resets them and closes their open orders.

*What the agents see:*
- An order's text carries the specs of the interfaces its item provides or uses ("You use interface auth-api v2 (Http), exactly as specified…"). A resent order keeps them.
- The outline lists the interfaces without their specs. `items` returns the specs of the contracts asked for.
- Sharing an interface across lanes makes an item strict.

*Decisions:*
- `Decision.one_way`, `criteria` (≤6) and `alternatives` (option, tradeoffs, cost).
- A one-way decision needs ≥2 alternatives.

*Docs:* `leader.md` explains interfaces and one-way decisions, and `proposal-example.json` shows an interface.

*Verification:*
- 529 tests pass, 9 of them new.
- The four Docker tests pass.
- clippy and fmt are clean.

*Changed from the design:*
- **Staleness.** It comes from the reset on `confirm`, not from storing interface specs in `contract_bindings` (`state.rs`, a CLI-work file). A reset step needs a new order, so an old review can't be accepted.
- **Worker `status`.** Its scoping waits for the CLI work (`worker_tools.rs`). The spec reaches the worker through its order instead.

*Not done yet:*
- Two-way decisions defaulting to the recommendation (9.2 point 4). Every existing decision is two-way by default, so turning this on would stop all interview questions until planners mark one-way ones. It needs its own decision.
- Interface checks at the milestone. They belong to M4.

**M4, M5 and M7, first parts (23-11-2026).**

*Correction to the audit:* `prs::open_prs` already refuses until every milestone is ready (`prs.rs:42-53`). The table in section 1 was wrong, and that part of M4 needs no work.

*M4, interface checks:* `milestones::validate` now also runs the `check` profile of every interface the milestone's items provide (`milestones::profiles`, tested).

*M4, not done:* the `system` profile kind with services on an `--internal` network. It needs `check_profiles.rs` and `runner.rs`, the CLI-work files.

*M5, learning:*
- When `accept_step` accepts a step with diagnosed work failures, `failures::lessons` turns each cause into one knowledge note for the item's lane: cause, evidence, fix, step and order. There's no agent turn, and `knowledge_for` hands the note to the next orders there.
- `failures::repeated` checks whether the same cause was diagnosed in ≥2 items of a lane. If so, `accept_step`'s answer tells the leader to make the mistake impossible: design, then a type, then a check whose error names the fix, then a test, and docs last.
- This is shown once, at acceptance, not on every inbox read.

*M5, not done:*
- Budgets: `task.rs`, `watch.rs`.
- Proving a new check fails on the recorded failing commit.
- `Diagnosis.premise`.

*M7, deterministic review:*
- **Test weakening.** At review binding, `reviews::weakened_tests` reads the change's unified diff. A test file that loses more assertion lines than it gains, or that switches a test off, becomes an automatic blocking `test_weakening` finding on the review.
  - The reviewer sees it through `review_diff`, because it is part of the review record.
  - An approval must resolve each one with a finding on that file.
  - A light acceptance with any automatic finding is refused.
- **Findings outside the change.** At submit, a blocking finding more than 3 lines from every changed hunk of its file is marked `pre_existing` and does not block. A "changes requested" whose only blocking findings are pre-existing is refused with the reason.
- **The reviewer's order** now says: try to break each acceptance line, no style findings, name an old problem in one line without blocking on it.
- **Tests:** scanner, deleted test file and hunk proximity are unit-tested. The lifecycle Docker test checks the pre-existing refusal, and all 4 Docker tests pass.

*M7, not done:*
- Rerunning repro commands.
- The safety fact.
- The risk brief's callers in other lanes.
- Preferring a reviewer on a different program (`team.rs`).
- A second reviewer on signals.

*Verification:* 535 tests pass, clippy and fmt are clean, and all 4 Docker tests pass.

**M6 result (23-11-2026).**

*Container slots:*
- `runner::host_slots` gives min(CPUs ÷ CPUs per container, `MemAvailable` ÷ memory per container), at least 1.
- Every container (checks, `execute`, image tries) first takes a slot: an exclusive lock on one of N files in `~/.devagentics/runner-slots/`. It waits for a free one and never fails for lack of one.
- The lock is released when the container ends, and the system releases it if the holder dies.
- The container's timeout starts only once it runs.

*Staffing advice:*
- `leader_inbox` shows `staffing`:
  - `independent_lanes_ready`: lanes with ready, unassigned work;
  - `host_slots`;
  - `suggested_writers` = min(lanes, `max_agents`, slots).
- An item waiting on another's interface is not ready, so it doesn't count.

*Leader rule:*
- Start with one writer, and add one only while `suggested_writers` is above the writers at work.
- Before adding one, write the throughput checkpoint in the summary.

*Verification:* 538 tests pass (3 new: memory parsing and the minimum of one, slot waiting and release, staffing limits). The 4 Docker tests pass with slots on.

*Side effect:* the Docker tests created the empty slot files in the real `~/.devagentics/runner-slots/`.

**M7, more (23-11-2026).**

*Second opinion on signals:*
- When a review approves a step that is critical, had ≥2 reviews asking for changes, or whose candidate failed integration, the leader's message says why and asks for a second code-reviewer before accepting, on another program when one is available.
- The newest review decides, as before. No other step gets a second review.

*Safety fact:* the reviewer's brief asks for the one fact the change is safe because of, and how the reviewer knows it: a run beats a pointed line, which beats their word.

*Tests:* one new test (`second_opinion`). The 4 Docker tests pass.

*Deferred, each to be added only if M8 shows it pays:*
- **Repro reruns.** They need runner execution at submit.
- **A reviewer on a different program by default.** The program is chosen at spawn time through roles, accounts and the Codex opt-ins, before the author is known. For now the leader is told when it matters.

**M5, more (23-11-2026).**

*Attack the premise:*
- `Diagnosis.premise` (optional).
- When a step has failed twice at the same gate (the same failure kind), the next order is refused until the diagnosis gives the one assumption every attempt shared and what tested it (20–1000 characters).
- `leader.md` explains it.
- Existing tests that ordered a third attempt now give a premise.

*Attention list:* `workflow::attention` is computed from the state and lists:
- light acceptances with their reason;
- approvals over devAgentics' weakened-test findings, with the resolution;
- findings the base already had;
- checks that could not run (environment, shown as inconclusive, not a pass);
- items made strict by repeated work failures.

It is the `attention` section of the workflow status, and the "Attention" section at the top of the task digest (leader handoffs and reopenings).

*Tests:* 2 new; 541 tests pass in all.

*Still open in M5:*
- Budgets: they need `task.rs` and the usage reading.
- Proving a new check fails on the recorded failing commit.

**M5 budgets (23-11-2026).**

*Configuration:* `task.toml [budget]` with `max_tokens` (input + output + cache, from the team's panes) and `max_hours` (since the first log entry). Nothing is limited without it.

*How the dispatcher checks it:*
- It checks every 30 s, and reads pane usage at most once a minute.
- **At 80%:** the leader gets a "BUDGET: 80% used" message, once per limit. The log line names the limit, so a raised limit warns again.
- **At the limit:** the task pauses with nothing dropped. The log has a budget entry and a pause, and you get an escalation with the accepted count and how to resume. Your pane is marked waiting.
- **Unreadable usage** never pauses.
- **Separate limits:** `max_review_rounds` is not added; the existing `max_attempts` covers it, as the plan said.

*Docs:* `agentic/docs/usage.md` has a "Risk routing and budgets" section that also covers `routing = "off"`.

*Tests:* 4 new (spend thresholds, warned once, pause and escalation, no budget). 545 tests pass in all, and clippy and fmt are clean.

*Touches the CLI-work file `task.rs`:* the `Budget` type and field, plus one `budget:` line next to each `routing:` literal.

**M8 preparation (23-11-2026). No runs yet: runs cost API usage and need you.**

*Fixture:*
- `agentic/bench/fixtures/bugfix-one-file`: `slugify` drops the last word, a one-line fix in `slugs/slugify.py`.
- Its 3 acceptance tests fail as shipped and all 4 tests pass after the fix (checked).
- `scripts/bench_team.sh bugfix-one-file reviewed` prepares it.
- `docs/user/devagentics-bench.md` lists it and the new report lines.

*Pass rule, written before any run:*
- **Arms:**
  - A: `routing = "off"`, today's process;
  - B: routing on.

  M1 isn't built yet, so it isn't an arm. Same models, accounts, answers and fixture commit. Paired runs alternate their order: A,B then B,A.
- **Runs:** start with 5 pairs per fixture (`bugfix-one-file`, `notes`). Add pairs while the spread of A's cost per integrated success is larger than the difference.
- **Primary number:** the median cost per integrated success.
- **B passes a fixture when all of these hold:**
  - its median is lower than A's by more than half of A's run-to-run range, and by at least 10%;
  - its delivered runs are not fewer;
  - its escaped defects are not more.

  Otherwise the result is "no measurable difference" and routing stays off by default.
- **Explaining the number:**
  - On `bugfix-one-file`, B must show ≥1 light acceptance and no planner or reviewer cost. If it doesn't, the routing did not engage, and the run measured nothing about it.
  - Record leader calls per accepted step: the expected cause of any gain.

**Two-way decisions (23-11-2026, approved by the user).**

*What changed:*
- Only `one_way` decisions wait for the user's answer. `Decision::settled` is true when a decision is answered or reversible, and `resolved()` and the interview frontier use it.
- A reversible decision takes its recommendation. It is listed in the workflow status as `assumed_decisions`, so the leader names it at confirmation.
- `answerable` still lets the user answer an assumed decision to overrule it.
- `leader.md` explains this.

*Effect on existing baselines:* their decisions are two-way by default, so they are no longer asked. Planners must mark the hard-to-undo ones as `one_way`.

*Tests:* the old frontier test now uses one-way decisions, and one new test covers the rest. 546 tests pass.

**Code review fixes (23-11-2026).** A high-effort review of the nine commits found 10 issues plus 3 smaller ones. Each fix has a regression test.

| # | Issue | Fix |
|---|---|---|
| 1 | A second review was asked again on every later approval | Asked once: none once the latest order has an independent approval |
| 2 | A light acceptance could overrule a "changes requested" or skip a pending review | Refused while the order has an independent review that is pending or not approved, checked before the candidate check and again under the lock |
| 3 | A revision could make an approved item light without the user; writing down an unchanged rigor asked for approval | `user_approval_needed` compares the effective routing, not the declared field |
| 4 | With `diff.noprefix`, no hunk or weakened test was seen | One `change_diff` with `--src-prefix=a/ --dst-prefix=b/`, tested on a repo with `diff.noprefix=true` |
| 5 | Findings on a deleted file counted as the base's | `touched` falls back to the `--- a/` path |
| 6 | A change's breakage in code it did not edit could not block | `Finding.caused_by_change`: such a finding still blocks, and the reviewer's brief says when to set it |
| 7 | Host slots shrank while checks ran, because they were based on free memory | Three quarters of total memory |
| 8 | Any lock error looked like a busy slot, so a container could wait forever | Only contention waits; other errors fail with the slot's path. No test: the error can't be produced here |
| 10 | A usage read error stopped the dispatcher's pass, and a budget pause still nudged the leader | Usage errors count as unknown, and `paused` is read again before the nudge. No test for the nudge order |
| extra | Repeated causes printed twice; work after a revision reset counted as an escaped defect; `iter().skip(1)` counted as a skipped test | Sorted dedup; counting stops at a reset; skip patterns matched as written and specific |

*Kept as decided:* #9, decisions stored before this change count as two-way and are no longer asked. You approved that. The leader lists them as assumed decisions at confirmation.

*Verification:* 553 tests pass (+7), the 4 Docker tests pass, and clippy and fmt are clean.

**Rebased onto the CLI work (23-11-2026).** The Codex session changed no source code after the first snapshot. It added the hybrid Notes comparison to the token plan: $4.13 with compact MCP against $3.20 with CLI reads, both arms integrated. The branch was replayed onto a second snapshot (`d1b2661`) with no conflicts.

**M1 result (23-11-2026).**
- **Workers:** the role and standards go on the first turn of a session only. Later turns get a re-anchor: the agent id, the tool rules, "call status", and the message.
  - The full role goes again in three cases: after a new panel (a restart, an account switch, a failed turn); when the provider has no session to continue (no Codex thread or OpenCode session, or a new Claude session); and on every 10th turn.
  - `shared.md` is no longer sent twice. A custom role without it still gets it.
  - Size: about 6.6 KB per turn before. Now the first turn is about 4.1 KB and the next nine about 0.7 KB each.
  - Telemetry records `full_role` and the bytes actually sent.
- **Leader:** `leader.md` has marked parts.
  - A task gets only its workflow's part, reviewed or legacy.
  - The account rules are read on demand (`application_workflow action=guide topic=accounts`, or `leader-read <task> guide <topic>`).
  - The prompt has a pointer to them, and so does the inbox message when no worker account can start.
  - A task adopted into the reviewed workflow is told to read `topic=reviewed`.
  - Size: a reviewed leader's role is 24.8 KB → 18.3 KB, at the start and on every handover.
  - A user's own `leader.md` without parts is unchanged.
- **CLI reads in telemetry:** `leader-read` records a `cli` observation, and the bench now counts leader calls from MCP and the CLI together.
- **Not done:**
  - *Note cap.* Agents can't read a whole note: `memory_search` returns snippets. A cap would cut notes with nowhere to read the rest, and adding a `memory_read` tool enlarges every catalog. Deferred until M8 shows long notes cost something.
  - *Worker `guide` topics.* The order already carries the kind's rule (e.g. "name the regression test").
- **Also fixed:** the container-slot test was flaky under parallel tests. A process forked by another test while a slot file was open holds its lock until it execs. The test now polls, as `take_slot` does.

**M3, worker status (23-11-2026).** A worker with an open order on a baseline item sees, in full `status`, only that item's contract and the specs of the interfaces it provides or consumes. With no order it still sees the items of its role and repo, as what may come next. Planners keep `application_read`; reviewers keep `reviewed_contracts`.

**M4 system checks (23-11-2026).**
- A repo's check configuration has `system`: profile names that every milestone covering that repo runs, after its items' and interfaces' checks. Orders and candidates never run them.
- A system profile starts the application and runs smoke or end-to-end tests against it in one container. `--network=none` keeps loopback, so `localhost` works; the Docker lifecycle test proves it with a server on `127.0.0.1:8080`.
- Taking a profile out of `system` needs the user, like `selected` and `candidate`. An empty `system` is not serialized, so old evidence signatures stay the same.
- *Not built:* several services on an `--internal` Docker network. It needs service images, a network per milestone and its cleanup in the runner. Built only when a fixture needs more than one container.
