# Reliable team execution

Use these commands to keep reports attached to the right assignment and run checks with explicit environments. The leader has equivalent MCP tools. Existing task files load automatically; agents must use the new report syntax after upgrading devAgentics.

## Permission levels

The leader MCP catalog labels each tool as `system`, `external` or
`destructive`. Project-local actions, including starting, stopping and replacing
the task's agents, are system actions and require no extra approval. Actions
outside the project, such as adding another repository or publishing pull
requests, first return a permission request and take no action.

The leader must show the exact requested call and its effect, then wait for
your explicit approval. After you say yes, it records that decision with
`authorize_action` and retries the same call once with its `approval_id`.
Approval applies only to those exact arguments. A changed call needs a new
approval; denial and replay do not run the action. Plan approval and step
acceptance remain separate controls.

`task_status` reports each role's group, repository access ceiling, sandbox
and provider approval mode. Floor roles have order-scoped repository writes;
seniors are read-only. The Agents board shows the role's permission summary
in its details.

## Continue when a provider or model is unavailable

You do not need to approve worker fallback. When Claude is exhausted, the
leader can start the same role on Codex automatically. Codex workers can use
Claude in the same way. The default Codex alternative is `gpt-6.1-sol` at medium
effort; the default Claude alternative uses its default model at high effort.
Fallback keeps the worker's role, lane and queued assignment.

Accounts at 95% or more of their week or five-hour allowance receive no new
workers unless you explicitly approve the task override described below. Explicit provider or model rejection during the opening turn also
tries an alternative. A worker already doing an order is not automatically
interrupted or moved.

To choose alternatives yourself, edit the role in `.devws/roles/` or
`~/.devagentics/roles/`. Put these settings inside its existing TOML header,
after the role's top-level settings:

```toml
[fallback]
agent = "claude"
model = "sonnet"

[[alternatives]]
agent = "codex"
model = "gpt-6.1-sol"
effort = "medium"
approvals = "auto"
```

The preferred role configuration runs first. Then `[fallback]` and each
`[[alternatives]]` entry are tried in order, once per account/model. Explicit
alternatives replace the defaults. Each candidate has its own provider settings.
An exhausted account skips all remaining models on that account; an unavailable
model permits another model on the same account.

Inspect the actual selection and reasons with:

```bash
devagentics task status <task>
```

Each worker exposes `agent`, `selected_program`, `fallback_reasons` and
`unavailable`. If all candidates fail, the leader explains why and retains
pending work. Restore availability or fix the reported problem, then run:

```bash
devagentics task resume <task>
```

Resume rechecks the candidates. Authentication, configuration and unknown
startup errors require diagnosis; they do not cause repeated model retries.
The leader's own two-program handover remains unchanged.

## Finished workers are forgotten

Each worker's source snapshot is recorded with a hash of every file, and
every read of the task state parses them all. When a candidate check or
preflight ends, or an agent is stopped, its record, its `source-*`
snapshots and its `scratch/` folder are removed; its `control/` folder stays.

A read-only agent (a planner, a reviewer) has a writable `/scratch` in its
container (`workers/<agent>/scratch/`), kept across the commands of one order
and emptied for the next. Each command runs in a fresh container whose `/tmp`
goes with it, so this is where it builds a prototype to prove a plan. Its
source stays read-only and it still has no network. Agents that are live,
lost, unavailable or waiting to restart on another account keep theirs.

A dispatcher prunes a task from before this when it starts, and says so in
the log. To do it by hand:

```bash
devagentics task prune <task>
```

## Report an exact order

Copy the order ID from the latest delivered `ORDER` message:

```bash
devagentics report done --order o12 "Implemented and tested login"
devagentics report blocked --order o12 "Need the schema decision"
```

Both commands require an ID. A closed, foreign, undelivered or obsolete order is rejected. A successful done report queues the work for leader review; it does not accept the step. If the order or checked inputs change during execution, retry against the current delivered assignment.

## Amend authorized files

From the leader pane, or an operator terminal with an explicit task:

```bash
devagentics order amend o12 --task feat-login \
  --allow tests/login.rs --allow tests/login-error.rs \
  --reason "Approved regression coverage for login errors"
```

The leader MCP tool is `amend_order` with `order`, `allowed` (an array), and `reason`. It preserves the order ID, original base and attempt count, records the amendment and queues the updated order. Prior evidence becomes invalid. The agent must receive the updated message before reporting. Worker panes cannot authorize their own CLI amendments.

Forbidden files and lane boundaries still apply. For lane globs, use an exact path, an existing lane pattern, or a pattern beneath a literal directory covered by the lane. An unrestricted order stays unrestricted. Repeated additions return a no-op result.

## Pause and resume

```bash
devagentics task pause feat-login             # stop new deliveries and starts
devagentics task pause feat-login --interrupt # also interrupt active turns
devagentics task resume feat-login
```

Interruption sends a single Ctrl-C to supported Claude/Codex panes and waits up to four seconds for a done/idle hook state. It preserves panes and worktrees. It exempts the invoking pane and includes the managed leader when safe. Do not use `devws agent stop` to pause: stop may close the pane.

A partial failure leaves the task paused. Inspect `task status <task>` under `recovery`; finish or interrupt a still-active turn, then retry resume. Acknowledged interrupted panes receive the authoritative current order; lost worker panes restart through the existing recovery path. Busy panes keep recovery paused. An agent without an open order gets no invented assignment. Repeating resume does not duplicate delivered recovery messages.

MCP: `pause_task` accepts an optional boolean `interrupt`; `resume_task` takes no arguments. An unacknowledged interruption may already have reached the terminal: inspect its state before sending another interrupt.

## Configure isolated checks

Use `configure_checks` with `repo` and a `configuration` object:

```json
{
  "repo": "app",
  "configuration": {
    "environments": {
      "sqlite-testing": {"values": {"DB_CONNECTION": "sqlite"}},
      "observer-testing": {"values": {"DB_CONNECTION": "pgsql"}}
    },
    "checks": {
      "sqlite": {
        "command": "php84 vendor/bin/pest --exclude-group=rabbitmq",
        "environment": "sqlite-testing",
        "timeout_seconds": 900
      },
      "postgres": {
        "command": "php84 vendor/bin/pest --group=pgsql --fail-on-skipped",
        "environment": "observer-testing",
        "requires": ["pg_isready -h localhost"],
        "timeout_seconds": 900
      }
    },
    "selected": ["sqlite", "postgres"]
  }
}
```

Set project-specific connection settings explicitly. `requires` contains shell preflight commands; unmet prerequisites produce `environment_failure`. Each profile gets a fresh environment containing only `PATH`, available `TMPDIR`/`TMP`/`TEMP`, and its configured values. `HOME`, database settings and other parent variables are not implicitly inherited. Define any additional tool settings your project needs.

`send_order` can select a different nonempty `profiles` array. The selected names are bound to the order; changing repository defaults affects future orders. Named profiles and legacy string checks cannot be mixed. Use `set_check` with an empty `command` to remove a repo's legacy check before enabling named checks. Existing orders carrying a legacy command remain legacy assignments.

The settings persist in the task's `task.toml`. For direct TOML editing, named settings live under `check_profiles.<repo>`; prefer the leader configuration tool for validation and coordinated updates.

### Secrets and required test evidence

Put secret input references in an environment's `sources`, mapping the child variable to the parent source name, for example `{"PGPASSWORD":"DEVAGENTICS_TEST_DB_PASSWORD"}`. Supply the actual secret to the runner process. Never place secret values in `values` or committed configuration. Missing sources fail before launch. Explicitly supplied secret values are redacted from completed logs and diagnostics. Profiles with source references always rerun, so changed secrets cannot reuse old evidence.

To classify required skipped tests separately, set `test_evidence: true` and have your test wrapper write this contract to the supplied `DEVAGENTICS_RESULT` path:

```json
{"tests": 42, "failures": 0, "required_skipped": 0}
```

No supported test framework adapter is assumed. Your wrapper must produce the counts from machine-readable runner output. Missing, invalid or zero-test evidence fails closed. Without this contract, a nonzero runner exit is `test_failure`; devAgentics does not infer skip counts from text. The Pest example above relies on the runner's own failure exit for `--fail-on-skipped`.

## Run checks and read results

```bash
devagentics check run --order o12
# Operator terminal:
devagentics check run --task feat-login --order o12
```

The leader MCP tool is `run_check` with `order`. The CLI prints JSON and exits unsuccessfully when checks fail. MCP returns the phase outcomes in the JSON result; inspect `passed` rather than relying on the transport error flag.

Results include order/revision/base/HEAD, `passed`, phase outcome, exit code, duration, a bounded diagnostic, log paths and a `run` evidence artifact. Outcomes are `passed`, `test_failure`, `required_tests_skipped`, `environment_failure`, `file_policy_failure`, `timeout`, and `runner_failure`. File-policy failure prevents test commands from running. A timeout kills the isolated shell process group and reaps its shell.

Done reports use the same runner. Passing named evidence can be reused only for the same order revision, base, HEAD, configuration and clean file policy. Legacy string commands retain inherited-environment behavior and rerun; migrate them to named profiles for isolation. Full logs remain in unique task check-run folders; inline results contain no raw full log.

## See also

- [Team usage](../../agentic/docs/usage.md)
- [Execution architecture](../architecture/devagentics-execution.md)

## Continue above the worker percentage threshold

When every compatible account the task may use reaches 95% of its weekly or
five-hour allowance, or is switched off for the task,
the leader shows their usage and asks whether it may continue using workers for
this task. Say yes to allow workers above that threshold, or no to keep the guard.
Approving the plan does not grant this separate permission.

After your explicit answer the leader records it with
`set_usage_limit_approval` (`approved: true` or `false`). After approval it calls
`resume_task` to start the existing blocked workers with their original IDs and
assignments. Approval lasts for this task, survives restarts and does not apply
to other tasks. To revoke it, tell the leader to stop allowing workers above the
threshold; it records `approved: false`. This affects future launches, leaving
running workers in place. After denial the leader asks again only if you request
reconsideration. Workers can still start normally after usage falls below the guard.

Provider-enforced quota errors and unavailable models still prevent startup.
Plan approval, worker capacity and worktree ownership rules still apply. The
leader's separate 95% switching policy is unchanged. `task_status` shows the
consent state, affected accounts and next action.

## Reviewed application planning and execution

New tasks use reviewed mode. Reopening an existing task preserves its mode;
`task start --legacy <folder>` deliberately creates a legacy task and never
downgrades an existing reviewed task. The current application team is not
automatically migrated.

Reviewed workers support Claude and OpenCode with native tools disabled and
only the scoped devAgentics MCP catalog enabled. OpenCode uses the existing
account, global and group switches and model settings. Its managed adapter
verifies OpenCode 1.19.6 on Linux and isolated configuration before readiness;
unsupported versions or platforms fall back before work is claimed. Failed claimed turns require explicit
recovery. The leader stays on Claude or on Codex with the existing task opt-in;
an OpenCode coordinator switch is refused.

Codex can run the read-only code reviewers, so a second model reviews what
the first wrote. It is off by default, because Codex cannot turn its own shell
off: in its read-only sandbox it cannot write, but it can read files on the
host. To allow it for one task:

```sh
devagentics task codex-reviewers <task> on    # off returns them to Claude
```

Reviewers started from then on (from the `code-reviewer` role, whose program
is Codex) run `codex exec` without your Codex config or rules, with the
worker tools as their only MCP server, in their read-only review snapshot.
Their conversation continues across turns. Writers and the leader always run
on Claude. Without the opt-in, a Codex reviewer falls back to Claude.
`application_workflow` `status` shows the reviewer adapter in use.

The whole team, leader included, can run on Codex too, when you accept
weaker isolation for that task:

```sh
devagentics task codex-team <task> on     # off: writers and the leader on Claude again
```

Workers then start on their Codex candidate (Claude stays behind it as a
fallback). A group with OpenCode switched on still starts on OpenCode first,
with Codex behind it. A writer's `codex exec` turns run in Codex's workspace-write sandbox
in its own runner snapshot: it can write only there (not `/tmp`, not the rest
of your home), with no network; checks and tests still run in the isolated
container. A reviewer's are read-only. The leader runs interactive Codex in
the read-only sandbox without approval prompts, with the leader tools as its
only pre-approved tools; restart it to move it (`devagentics task start
<folder> --leader codex`, then `devws agent run <pane> codex --agent-args
<file>`). What Codex cannot do is turn its own shell off: every Codex agent
can read files on the host.

### Prepare the isolated tool environment

Inspect available local images and project facts through the leader's
`application_workflow` tool (`runner_images` and `project_overview`). Select a
local image containing the required runtimes and offline dependencies. Images
are pinned by their full `sha256:` ID; the runner never pulls an image or changes
the Docker daemon configuration automatically.

The runner uses only a **rootless** Docker daemon of your own, never a root or
shared one, so a host's other containers (production ones included) stay out of
its reach. By default that is `$XDG_RUNTIME_DIR/docker.sock`; set the runner
configuration's `docker_host` (`unix://…`) to pin another rootless socket.
Every Docker command it runs goes to that socket whatever `DOCKER_HOST` or your
current Docker context say, and configuring or using the runner is refused when
the daemon there does not report itself as rootless. Set one up with
`dockerd-rootless-setuptool.sh install`. Its containers need the systemd
cgroup driver with your user's cpu, memory, pids and io controllers delegated,
so the runner's resource limits are enforced (once, as root, a
`/etc/systemd/system/user@.service.d/delegate.conf` with
`Delegate=cpu io memory pids`, then `systemctl daemon-reload` and a restart of
your `user@<uid>.service`). Containers run as the rootless daemon's root, which
is you on the host, without capabilities.

Two more runner settings fit a project's toolchain:

- `image_mounts` mounts folders of the image read-only inside the workspace,
  for dependencies the project keeps untracked: the snapshot holds tracked
  source only and the container has no network. For example
  `[{"path": "/node_modules", "target": "web/node_modules"}]` with an image
  that ran `npm ci` at `/`. A mount may not hide tracked source.
- A check gets its order's `targets` (the paths the leader named for the
  change, space-separated; empty for a full run) as `DEVAGENTICS_TARGETS`. A
  command that uses it runs only there, for example
  `cd web && npx vitest run $DEVAGENTICS_TARGETS`. When an order's checks ran
  on targets, its review candidate runs every selected profile in full before
  the step can be accepted.
- `memory_mb` (128–4096, default 4096: no container gets more than 4 GB, swap
  included) and `cpus` (1–4, default 1) size each
  container. A TypeScript check or a Next.js build needs about 2048 MiB.

A build that fetches from the network (Next's `next/font/google`, for one)
cannot pass in the runner; vendor what it fetches (`next/font/local`) or leave
it out of the reviewed checks.

If every container fails with "Interactive authentication required", runc did
not learn your host user: it reads the `OwnerUID=` line of `busctl --user
--no-pager status`, which some systemd versions (Debian 12's 252) do not
print, and then asks the system manager instead. A `busctl` wrapper ahead of
it in `docker.service`'s `PATH` (a user drop-in) that adds
`OwnerUID=$ROOTLESSKIT_PARENT_EUID` to that one query fixes it.

Configure the image, source paths, protected paths and timeout using a JSON file:

```json
{
  "configuration": {
    "image": "sha256:<64 hexadecimal characters from the local image ID>",
    "source_paths": ["src/**", "tests/**", "package.json", "package-lock.json", "AGENTS.md", "CLAUDE.md"],
    "protected_paths": ["data/**", "database/**"],
    "timeout_seconds": 300
  }
}
```

```bash
devagentics workflow <task> runner_configure --input-file runner.json
```

Through MCP, the leader decides `runner_configure` on its own. Only a change of
`docker_host`, another Docker daemon, uses the exact-call approval flow. Every
image is checked with a disposable container before the configuration becomes
active.

### A lane with its own image and checks

A project whose parts need different toolchains (a PHP backend beside a Node
frontend) gives a lane its own image. The runner configuration's `lanes` maps
a lane to `{"image": "sha256:…", "image_mounts": [...]}`, and that lane's
commands, checks, candidate checks and checkpoints run in it. Its checks are
named like the repo's, under the check configuration's `lanes`:

```json
{"lanes": {"back": {"test": {"command": "cd backend && php artisan test", "environment": "testing"}}}}
```

In the back lane, `test` runs that command. Its other profiles and the other
lanes keep the repo's. A milestone runs each check in the image of every
lane that defines it, and a check no lane defines in the repo's image: the
front lane's `build` never runs in the back lane's image. A change
that only adds or edits a lane's image waits for that lane's workers alone;
the other lanes keep working.

### Images the team builds

An `infra` senior (a provisioned role) builds the images a lane needs. It
writes a Dockerfile, and devAgentics builds it (`image_build`) on your
rootless daemon:
- the build context holds only the repo's tracked files that the request
  names;
- the build has network, for package downloads;
- it is capped at 4 GB of memory, 2 CPUs and 30 minutes;
- it is tagged `devws-runner/<repo>:<name>`.

The infra senior then runs each check command in the new image (`image_try`),
with no network and the source read-only, and reports the image ID, its
`image_mounts` and the commands. The leader puts them in the runner and in
the checks without asking you. Declare narrow source roots appropriate to your project;
broad whole-project globs are rejected. Use explicit migration source paths when
needed while keeping existing database files protected.

Tools operate on disposable copies of tracked source. No live project directory,
Git metadata, Docker socket, host home or database volume is mounted. Containers
have no external network, a read-only root filesystem, a non-root identity,
dropped capabilities and resource/time limits. Secret/configuration directories,
`.env` files, database files and declared protected paths are omitted. Symlinks
and hardlinks cannot be imported as source.

A specialist may modify or delete source/artifacts within its assigned scope.
Its checkpoint validates the assignment, source generation and every changed
path before importing and committing source changes in the implementation lane.
It cannot delete database data or mutate another project. Such actions remain
leader-only after explicit approval; this runner does not expose an arbitrary
host/database shell to either workers or the reviewed coordinator.

### Previews: an application for you to look at

When the team builds an application image you want to try, the leader starts
it for you (`preview_start`) after you approve that exact call:
- the image runs like a check container: no network, a read-only root, capped
  memory and CPUs, and the task branch's tracked files mounted read-only;
- the application listens on unix sockets in `/run/preview`
  (`$PREVIEW_SOCKET_DIR`), and a devAgentics forwarder on your machine opens
  one `127.0.0.1` port for each. Nothing listens on your network;
- it runs for the hours you approved (at most 8), until the leader calls
  `preview_stop`, or until the task is cancelled. Then the container and its
  data are gone.

The board's overview shows it under PREVIEW: its addresses and the time left.
Open them with `127.0.0.1`, not `localhost`, when the application only
accepts that address. When you work on that machine over SSH, forward the same
ports from your computer (`ssh -N -L 13000:127.0.0.1:13000 <host>`, one `-L`
per port) so the addresses the application gives out work there too.

### Plan the whole application with the planner

Have the planner interview you in grilling rounds: currently answerable questions,
recommendations, recorded answers and unresolved dependencies. Its scoped tools
include `plan_proposal`, `plan_amend` (only the changes for the next revision),
`application_read` (the baseline as an outline, or chosen items in full) and
`ask_user`; only the leader records your approval.
The baseline covers the known application before implementation staffing:
scopes, applicable backend/frontend/data work, interfaces, dependencies, tests,
acceptance criteria, required check definitions and milestones.

You approve the first baseline and every revision that changes the plan: the
goal, non-goals, decisions, scopes, items or milestones, or a check you
approved. A revision that only adds checks (a new lane's commands, a new
repo's) is the leader's to approve, with no confirmation round. It tells you in
its next summary, and the log records "the leader approved check-only
revision N". Workflow status shows which kind a pending proposal is
(`proposal_approval`).

The leader asks you only for plan changes, anything outside the project,
destroying data or files outside the approved source scope, spending past a
limit, and product questions. It decides the rest itself (staffing, retries,
recovery, lanes, runner images and checks, reviews, accepting steps) and
reports what it decided.

Inspect or submit the baseline through CLI or the equivalent leader tool:

```bash
devagentics workflow <task> status
devagentics workflow <task> status --input '{"section":"application","items":["S01-B01"]}'
devagentics workflow <task> propose --input-file baseline-proposal.json
devagentics workflow <task> amend --input-file changes.json
devagentics workflow <task> answer --input '{"question":"Q1","answer":"Use milestone releases"}'
devagentics workflow <task> confirm --input '{"revision":1}'
devagentics workflow <task> approve --input '{"revision":1}'
```

`status` gives the baseline and the proposal as outlines (each item's title,
scope, role, lane and dependencies), so it stays small enough for an agent's
tool call. `section` returns one part in full (`runner`, `ready_work`,
`reviews`, …), and `items` the full contracts of the items named. `amend`
takes `{"reason": …, "changes": {…}}` with only what changes, by id:
`items`, `milestones`, `scopes` and `check_configuration` (`null` removes an
entry), `decisions` (replacing the one with the same id), `goal` and
`non_goals`. It builds on the pending proposal, so amendments add up, and is
checked and approved like any proposal.

The proposal file contains `application` and `reason`. Every implementation item
specifies its scope, specialist, repository, lane, dependencies, file scope,
reuse references, interfaces, behavior/errors, tests, named profiles, acceptance,
open questions, data effects and standards version (`1`). An item may name
`protected_tests` (globs): the acceptance tests that judge it. They come from an
item of their own with `writes_tests: true`, which the implementation must
depend on, and every order for the implementation forbids them, so it can read
but not change the tests it is judged by. Required profile
definitions are captured in `check_configuration` for your review. Missing fields,
path escapes, unresolved decisions and cyclic dependencies cannot become ready.

One approval authorizes execution of that exact baseline. Material changes use a
new proposal and explicit approval; an unapproved proposal does not overwrite
the approved baseline. Approval of an amendment invalidates affected work and
checks while preserving independent unchanged steps and stable identifiers.
Changing an approved check definition also requires baseline approval. Routine
corrections within approved scope do not add user approval stages.

### Execute, review and integrate

Allocate specialists and disjoint lanes from the approved dependency graph.
Starter roles cover backend/data, API/integration, frontend/UI, documentation,
planning and independent code review. Existing custom roles are not overwritten.
Their shared standards include SOLID, Object Calisthenics applied idiomatically,
TDD, security, reuse and project/data boundaries; frontend/backend guidance is
adapted from orbit-front and maestro without requiring those repos on the server.

Writing orders must match the accepted specialist/lane/files/profiles contract.
Prerequisites run in a separate source snapshot before assignment. Missing
capabilities block that work, leave the gate outstanding and retain a reason;
unchanged failures are not automatically rerun. After correcting the environment,
use `preflight_retry` with `item` to request another attempt.

Managed worker panels have no editable model prompt. A supervisor consumes their
queued messages through piped provider turns and retains conversation context
between healthy turns. The panel draws each turn for reading: a header with the
agent, program, model, effort and order; the agent's messages as formatted
markdown; one line per tool call with a green ✓ or a red ✕ and its reason;
commands; and a footer with the time and tokens. Each turn's raw provider
lines are kept in `workers/<agent>/control/turns.jsonl`. Source commands, checkpoints, authoritative checks and
reports use the worker-only tools. Read-only reviewers cannot checkpoint code,
authorize actions or merge.

The leader reads `leader_inbox` and acknowledges handled IDs with `inbox_ack`.
Reviewed coordination never pastes a message's text into your prompt. When the
leader's turn is over and a message arrived since its last nudge, the board types
the fixed line `Check your inbox and continue.` so the team keeps moving without
you. Existing legacy
terminal delivery is unchanged until explicit adoption.

After the specialist reports passing checks, request independent review with
`review_request` (`order`, `reviewer`). The reviewer receives an immutable copy of
the proposed integration context. Blocking findings require correction and a
fresh review. A passing verdict binds the exact source/target commits and check
evidence; changed inputs invalidate it. Only leader acceptance integrates the
approved candidate. Divergent lane catch-up is blocked rather than inventing
unreviewed conflict-resolution code. Resolve it through a reviewed assignment.

Each lane passes its own checks, but two lanes can break each other once
merged. Name a fast build/smoke set as the repo's `candidate` profiles:

```json
{"checks": {"smoke": {"command": "make build smoke", "environment": "testing"}},
 "environments": {"testing": {}}, "selected": ["unit"], "candidate": ["smoke"]}
```

`review_request` then runs them on the merge candidate (the lane's work merged
with the task's branch) in a disposable runner snapshot before the reviewer is
asked. A failing candidate fails the step as an integration defect, and the
leader orders a correction. A passing result is bound to the review: the merge,
acceptance and recovery require it to be current, and a changed candidate or
candidate configuration needs a fresh review. Without candidate profiles,
`application_workflow` `status` warns that merges are first tested together at
the milestone check.

A step gets the task's `max_attempts` (5, in `task.toml`) in all: orders, and
reviews that ask for changes. When they are used, devAgentics waits 15 minutes
and allows one more attempt on its own. If that one fails too, the step is
**failed** (a red ✕ under NEEDS YOU), orders and review requests are refused,
and the leader escalates to you; `devagentics task attempts <task> <step>
<limit>` allows that step more and starts the retry over.

Required checks run in the isolated source environment. Use image-local runtimes
and dependencies; network downloads and host secret sources are unavailable.
Database tests must use explicitly disposable data in the isolated environment,
not an existing host/service database. The runner does not silently skip a
required test or substitute a weaker gate. Matching authoritative evidence is
reused when reporting unchanged work. Structured test counts use a separate
bounded artifact, so large console logs cannot hide required skipped tests.

### Code graph (pilot)

A code graph lets the leader and workers look up a symbol's definition,
callers and callees instead of grepping, which helps plans name real
interfaces and reviews catch callers in other lanes. It is
[Graphify](https://github.com/Graphify-Labs/graphify), run code only: no LLM
backend, no query log, no network (in a network namespace where the system
allows one), with its own `HOME`. devAgentics builds the graph of a repo's
tracked source per commit into the task folder (`graphs/<repo>/<tree>/`),
never into the project, and with a runner, only from what the runner lets
agents read. A graph is never reused for other code.

```sh
devagentics task graph <task> on      # off to stop
```

It is off by default. Turning it on checks that Graphify runs, from
`DEVAGENTICS_GRAPHIFY` or the pinned install at
`~/.local/share/devagentics/graphify` (a venv of `graphifyy`, double y; other
`graphify*` packages are unaffiliated). The leader's `graph` tool asks about a
repo's task branch; a worker's `graph` action asks about its lane, and a
reviewer's about the candidate it reviews. `explain` takes a name or
`path::name` and lists candidates for an ambiguous one; `path` takes two;
`query` is a bounded search. `devagentics bench` counts the queries, so a
series with the graph on can be compared with one without.

### Milestones, recovery and existing tasks

Use IDs such as `S01-B01` and `S01-F01` for work, and `M01` → `1.1.0` for a
milestone. Reordering and retries do not allocate application patch releases.
The board's completed milestone rows represent integration readiness, not a
published release. Validate the combined code after all required reviewed steps:

```bash
devagentics workflow <task> validate_milestone --input '{"milestone":"M01"}'
devagentics workflow <task> milestones
```

Missing/stale/failed integration evidence blocks publication readiness. Reserving
a number, accepting a step or opening a draft PR never tags, publishes or deploys.
Draft PR publication retains its separate external approval.

Inspect `preview_adoption` before adopting a legacy task. Adoption requires a
verified runner, a paused task, no active legacy agents and no open legacy orders.
It preserves historical acceptance without inventing past review verdicts.
Repository history exports retain approved application plans and completed review
provenance; runner credentials, local configuration and environment evidence are
not transferred. Reconfigure the local runner and revalidate final integration on
a new server.

Uncertain managed turns are retained for inspection, not automatically replayed.
Use `recover_turn` with the stopped/lost agent and `resend_confirmed: true` only
after deciding a resend is safe, then resume. Merge recovery recognizes an exact
approved integration candidate rather than arbitrary branch advancement.

Workflow status and leader board details expose ready work, staffing/capacity,
review state and measured delays. Aggregate check/turn seconds are separate from
elapsed intervals; missing historical timing remains unknown. No fixed speed-up
is promised by these measurements.
