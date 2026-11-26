# How devAgentics works

This page explains how devAgentics runs an agent team today, and what each
agent does with each of its tools. [usage.md](usage.md) covers how *you*
use it (starting a team, the board, settings). [design.md](design.md) covers
why it was built this way.

It describes **reviewed** tasks, the workflow new tasks use. Legacy tasks are
covered briefly at the end.

## The pieces

```
 you ──▶ leader pane ─────────── MCP ──▶ devagentics mcp ─┐
          (Claude Code: reads     (leader tools, read-   │
           code, no shell)          only code access)     ▼
                                                     task state
 board pane ── devagentics board ── dispatcher ──▶ ~/.devagentics/tasks/<id>/
 (team-log)    (the screen)         (every 1 s)          ▲
                                                         │
 worker panes ── devagentics worker-panel ── turn ── MCP ┘
 (floor, seniors)  (a supervisor)       (claude/opencode/codex
                                         --print, one per message)
                                                │
                                                ▼
                                    runner: rootless Docker
                                    (snapshot of the project,
                                     no network, checks run here)
```

- **devws** gives the team its tmux workspaces: the main one holds the leader
  pane and the board. The `-floor` and `-seniors` workspaces hold the
  workers, four to a workspace.
- **`devagentics`** is one Rust program with several roles:
  - `devagentics mcp`: the leader's tool server.
  - `devagentics worker-mcp`: a worker's tool server, one per worker.
  - `devagentics worker-panel`: the process in each worker pane.
  - `devagentics board`: the board screen and the dispatcher.
  - The CLI commands you run (`task`, `role`, `opencode`…).
- **The task folder** `~/.devagentics/tasks/<id>/` is the only shared state:
  - `state.json`: the steps, orders, agents, messages, baseline and evidence;
  - `log.jsonl`: everything that happened, one line each;
  - `task.toml`: the repos, lanes and checks;
  - `lanes/`: one worktree per lane;
  - `checks/` and `candidate-checks/`: each check run's logs and evidence;
  - `workers/<agent>/control/turns.jsonl`: each worker's raw program output.

  Every change to `state.json` goes through one locked transaction
  (`Task::transaction`), so the leader, the dispatcher and the workers never
  overwrite each other.
- **The runner** runs worker commands and checks in containers on your
  rootless Docker, from a **snapshot** of the project: only the files
  `source_paths` lists, never secrets or data. There's no network, and the
  live project is never mounted.

## Who is on the team

Roles come from `~/.devagentics/roles/*.md` (`devagentics role list`). The
leader's role is built into the program.

| Group | Role | Writes? | What it is for |
|---|---|---|---|
| cto | `leader` | no | Plans with you, reads the code, finds why work fails, starts and stops agents, gives orders, reviews and accepts work |
| senior | `planner` | no | Writes the baseline and each step's execution plan |
| senior | `code-reviewer` | no | Reviews a step's merge candidate independently |
| senior | `infra` | no | Diagnoses the runner, builds and tries images |
| floor | `executor`, `backend-data`, `frontend-ui`, `api-integration`, `documentation` | yes | Implement one step at a time, in one lane |

Each role names its program (Claude, Codex, OpenCode or Antigravity), its
model and effort. When that account is off, used up or refused, the next
account that is on for its group is tried.

## How the leader runs

The leader is the team's CTO: it understands the project, finds why things
fail, and changes the approach or puts someone on it who can.

It is a Claude Code (or Codex) session started with **no built-in tools**: no
shell, no editing, no web. Its only tools are the devAgentics MCP tools, all
pre-approved (`leader_arguments` in `worker_tools.rs`). Through them it
**reads the code**: `read_file`, `search_code`, `git_log` and `git_diff`, on
the task branch, a lane or an order. These read committed objects only, and
never what a snapshot leaves out (secrets, data, protected paths). It writes
nothing: the workers write all code, so reviews stay independent.

Why no shell: every guarantee of a reviewed task holds only because the
leader can act *only* through these tools, and each tool enforces the rules:

- nothing writes before you approve the plan;
- each order is limited to its files and lane;
- protected tests can't be changed by the work they judge;
- nothing merges without an independent review and a passing merge check;
- every action is in the log.

**How a turn starts.** The leader never polls. The dispatcher types a fixed
line, "Check your inbox and continue.", into its pane when a new message
arrives and the leader isn't busy. The leader then reads its inbox
(`application_workflow` → `leader_inbox`) and acknowledges what it handled.
At the start, after a move and after compaction, it first learns the
project: `project_overview`, then CLAUDE.md, AGENTS.md, README and the docs
they point to, and the team's knowledge in memory.

## How a worker runs

A worker pane runs `devagentics worker-panel`, a **supervisor**, not an
interactive prompt. For each message (an order, an answer, a nudge) it runs
one non-interactive turn of the agent's program (`claude --print`,
`opencode run` or `codex exec`) and resumes the same conversation every
time. The program's only tool is the worker MCP server, with one tool:
`worker`. The pane shows the turn's events as they come.

A worker never touches the project folder. Its `execute` commands run inside
a container on its own snapshot. Its edits become real only through
`checkpoint`, which validates them and commits them to its lane.

## A step from start to finish

1. **Baseline.** The leader interviews you, and a planner writes the
   application baseline: the goal, non-goals, decisions, scopes, items (one
   contract per step) and milestones. You confirm your understanding and
   approve it (`application_workflow` → `confirm`, `approve`).
2. **Runner and checks.** The leader configures the runner
   (`runner_configure`) and the named checks (`configure_checks`): lint,
   typecheck, test, per repo, and per lane where a lane needs its own. An
   infra agent builds lane images when needed (`image_build`, `image_try`).
3. **Lanes.** `add_lane` gives each area (back, front, docs…) its own
   worktree and branch. Lanes never share files, so hammers build side by
   side.
4. **Agents.** `spawn_agent` starts a role in a lane. It answers "ready",
   then takes orders.
5. **Order.** `send_order` names the agent, the step (a baseline item), what
   to do, `read` (files to read first) and `targets` (where its checks run).
   The order carries the team's notes on the step and its knowledge of the
   area (its lane, the folders of its files), so the worker reads only what
   they don't cover. After a failure it also needs the leader's diagnosis.
   Before delivering it, devAgentics catches the lane up with the task
   branch. The dispatcher types the order in when the worker is free.
6. **Work.** The worker reads and edits through `worker` → `execute` in its
   snapshot, and saves with `checkpoint`.
7. **Check.** `worker` → `check` runs every selected check profile in a
   container on the committed work. Only the order's `targets` get checked
   when the order names them. The result is stored as **evidence** for that
   order, at that exact commit and configuration.
8. **Report.** `worker` → `report` (done or blocked). devAgentics verifies on
   its own: committed work, only allowed files, passing evidence. The step
   becomes **reported** and the leader is told.
9. **Merge check and review.** `application_workflow` → `review_request`
   first builds the **merge candidate** (the lane merged onto the task
   branch) and runs its candidate checks. If the order ran only on targets,
   it runs every selected profile on the whole repo, once. If that fails, the
   leader gets INTEGRATION FAILED with the failing log lines. If it passes, a
   code reviewer reads the candidate (`review_diff`) and answers
   `review_submit`: approved, or changes needed.
10. **Accept.** `accept_step` merges the approved candidate into the task
    branch and commits the team's history
    (`.devagentics/history/<task>/`). Changes needed, or a failure: the
    leader finds the cause and orders the step again with its diagnosis
    (see [How the leader handles a failure](#how-the-leader-handles-a-failure)).
    Only failures of the work use the step's attempts.
11. **Milestone.** `validate_milestone` checks the integrated code of a
    milestone's items together.
12. **Pull requests.** `open_prs`, once every step is accepted: one draft
    PR per repo, each linking the others.

## Who decides what

| Decision | Who | How |
|---|---|---|
| The first baseline | you | the leader asks; `confirm` + `approve` after your yes |
| Goal, non-goals, decisions, scopes; a milestone added, removed or renamed, or an item taken out of one | you | same; the board shows what the revision changes under NEEDS YOU |
| Removing an item, taking protected tests away, changing whether an item writes tests | you | same |
| Weakening a check (changing or removing one you approved) | you | same |
| How a step is carried out (its files, behavior, acceptance details, tests), new steps, more work in a milestone, new checks | leader | approves itself, tells you in its next summary |
| More attempts for a step | leader | `allow_attempts` with the cause; you can also run `devagentics task attempts` |
| Retrying a step for the same cause with the same approach | you | `devagentics task attempts` lets one order through; otherwise the leader changes the approach or closes the step |
| Workers past the 95% usage guard | you | `set_usage_limit_approval` records your answer |
| Anything outside the project (another repo, pushing, managed storage) | you | `authorize_action`, one exact call at a time |
| Accounts on or off, models, OpenCode switches | you | the board, `devagentics opencode` |
| Everything else: orders, agents, reviews, accepting | leader | its own tools |

`application_workflow` → `status` → `proposal_approval.needs_user` tells the
leader which way a pending revision goes.

## The leader's tools

| Tool | What it does | When the leader uses it |
|---|---|---|
| `task_status` | The plan, repos, agents and states, steps, accounts and limits, what waits for you, the recent log | First, after a pause, whenever unsure |
| `application_workflow` | The reviewed workflow, by `action`: `status`, `propose`/`amend` a revision, `answer`, `confirm`/`approve`, `milestones`, `review_request`, `runner_configure`, `image_build`, `runner_images`, `leader_inbox`/`inbox_ack`, `recover_turn`, `validate_milestone`, `project_overview`, `preflight_retry`, `provision_roles`, `preview_adoption`/`adopt` | Planning, revisions, its inbox, reviews, the runner |
| `approve_plan` | Records your approval of the plan | Only after your yes |
| `spawn_agent` | Starts a role in a repo or lane | When a lane or a review needs an agent |
| `stop_agent` | Ends an agent; its worktree is free | When its work is done, or to replace it |
| `add_lane`, `add_repo` | New lane worktree; another repo on the task branch | Setting up the work |
| `send_order` | One order to one agent, with its step, files, `read`, `targets`; after a failure, its `diagnosis` | Every piece of work |
| `amend_order` | Widens an open order's allowed files, keeping its identity | A worker needs one more file |
| `run_check` | Runs an order's checks itself | Re-checking delivered work |
| `configure_checks`, `set_check` | The named checks; a legacy check command | Setting up or changing checks |
| `read_plan` | A senior's execution plan for a step | Before accepting a planning step |
| `answer_question` | Answers a worker's question | A question came to the leader |
| `accept_step` | Accepts a reported step; merges a lane's approved candidate | Work you keep |
| `reject_step` | Sends a reported step back with the reason | Work to redo |
| `close_step` | Ends a waiting read-only step whose report was used or isn't needed; nothing merges | Used preflights, superseded plans |
| `read_check_log` | A check run's outcome per profile and the end of its logs; `of: candidate` for the merge check | A check failed and its message isn't enough |
| `snapshot_path` | Whether a path is in the containers' copy, and which rule decides it | A worker or check misses a file |
| `read_file` | A file with numbered lines, or a folder's entries, at the task branch, a lane, an order or a commit | Learning the project; reading where a failure points |
| `search_code` | The matching lines, with file and line | Finding callers, uses, conventions |
| `git_log` | The commits of the task branch, a lane or an order | What moved |
| `git_diff` | An order's work, a lane against the task branch, or between two names | Reviewing what a worker did; diagnosing a failure |
| `read_log` | The task's log, filtered by kind | Looking back |
| `memory_save`, `memory_search` | Notes for whoever comes next (decision, gotcha, handoff, finding), and knowledge of an area of the code | After each accepted step, change of direction, or investigation of an area |
| `graph` | The code graph: where a symbol is, its callers, paths between two | Reuse, interfaces, impact (when you turned it on) |
| `probe_worker` | Checks an idle OpenCode worker answers, without giving it work | Verifying an OpenCode worker |
| `pause_task`, `resume_task` | Stops deliveries; resumes them and restarts lost agents | Recovery |
| `allow_attempts` | Gives a step more attempts, with the cause in the log | The cause is found and the next attempt can succeed |
| `set_usage_limit_approval` | Records your answer on the usage guard | After you answered |
| `authorize_action` | Records your answer for one call outside the project | After you answered |
| `escalate` | Puts a decision to you and marks the leader as waiting for you | Only what it can't decide |
| `switch_leader` | Moves the leader to its other program after this turn | You asked for another model |
| `open_prs` | Pushes and opens one draft PR per repo | Every step accepted |

## The workers' tool

Every worker has one MCP tool, `worker`, with an `action`:

| Action | Who | What it does |
|---|---|---|
| `status` | all | Its order, allowed files, targets, plan |
| `execute` | all | Runs a command in its snapshot container (no network); reading, searching and editing happen here |
| `checkpoint` | writers | Validates its edits (allowed files, protected tests) and commits them to its lane |
| `check` | writers | Runs its order's checks; the result becomes the order's evidence |
| `report` | not reviewers | Done or blocked, with a summary; devAgentics verifies it before the leader hears |
| `plan_write`, `plan_read` | planners and infra write; anyone reads | Stores or reads a step's execution plan |
| `plan_proposal`, `plan_amend`, `application_read` | planners and infra propose; anyone reads | Proposes a baseline revision; reads the baseline |
| `ask`, `answer` | floor asks, seniors answer | A question about the order goes to the senior who planned it, or the leader |
| `ask_user` | planners and infra | A product question for you, through the leader |
| `review_diff`, `review_submit` | code reviewers | Reads the candidate diff, its plan and check results; submits the verdict |
| `graph` | all | The code graph, when on |
| `memory_save`, `memory_search` | all | The team's notes; knowledge of an area (`kind: knowledge`, `area`) |
| `image_build`, `image_try` | infra | Builds an image (with network, capped); tries a command in an image on its read-only snapshot |

No worker has the leader's tools, task state, databases or outside actions.

## The dispatcher: what happens every second

The board pane runs the dispatcher (`Dispatcher::tick` in `watch.rs`). Every
second it:

- **delivers** queued messages: types them into a free legacy agent, or lets
  a managed worker's next turn claim them;
- **nudges the leader** when a new message waits and it's idle;
- **watches each agent**:
  - a worker that ends its turn without reporting, or stalls past a
    2-minute grace, is reported to the leader;
  - a pane that's gone is marked lost, and the leader hears why: the error
    the worker's last turn ended with, such as a used-up account;
  - a worker an account refused before it did anything restarts on the
    next account, and that account takes no new agents for 30 minutes;
- **retries spent steps**: a step whose work failed as often as its
  attempts allow gets one more after 15 minutes; if that fails too it is
  **exhausted**, and the leader settles it (more attempts with the cause,
  another approach, or closing it);
- **reminds the leader** of steps waiting for it longer than 10 minutes:
  once per wait, in one message (accept, reject or close them).

Every minute, or at once when one is asked for, it **hands the leader over**
to another account if its own ran out or you asked. The old leader saves a
handoff first (up to 10 minutes), and the new one starts with the task's
digest and that handoff.

## Failure history and diagnosis

Every step keeps why it failed (`failures.rs`), shown in `task_status`
(`failures`: the newest, `work_failures`, `needs_diagnosis`).

| Failure | Counts as an attempt? |
|---|---|
| Its tests or file rules failed; a review asked for changes | always |
| A check timed out, the merge check failed, a review was blocked, the agent was blocked, the leader rejected it | when the leader's diagnosis names a cause of the work |
| Only the environment or runner failed; evidence went stale; a revision changed its contract; its agent was stopped | never |

Ordering a step again after a failure that was, or might be, the work's needs
a **diagnosis**: the `cause` (implementation, test, plan, missing context,
integration conflict; or other work, the environment, devAgentics), the
`evidence` that shows it, and the `change` the new order makes. The same
cause twice with the same agent, contract, plan, files and targets is
refused: the leader must change the approach or close the step. Only
`devagentics task attempts` (you) lets one order past that rule.

A revision resets only the items whose contract it changed (a reset costs
no attempt and needs no diagnosis). Work that depends on them keeps its
status, and the leader is told to check it. A removed item is closed.

## How the leader handles a failure

An example: hammer-28 reports B3 done, but its `test` check fails.

1. **The failure is recorded.** B3 becomes **failed**, `task_status` shows
   it under `failures` (kind `check_failed`, which counts as an attempt) with
   `needs_diagnosis: true`, and the leader's inbox says where to look.
2. **The leader looks.**
   - `read_check_log o672` shows the failing test and its assertion.
   - `git_diff at=o672` shows what hammer-28 changed.
   - `read_file` and `search_code` show the test and the code it calls.
   - `git_log` shows whether something else moved underneath.
3. **It decides the cause.**
   - **The work's fault** (`implementation_defect`, `test_defect`,
     `plan_defect`, `missing_context`, `integration_conflict`): it
     re-orders with a diagnosis, naming the cause, the evidence it found
     and what the new order changes (for example a `read` file the worker
     missed). That uses an attempt.
   - **Not the work's fault** (`other_work`, `environment`,
     `devagentics`): the same, but no attempt is used.
   - **Needs someone else:** an environment or image problem goes to an
     infra senior, and a wrong contract or interface to a planner (an
     amendment). A devAgentics bug goes to you, with what the tools showed.
4. **If it fails again for the same cause,** the same agent, contract,
   plan, files and targets are refused. The leader must change something:
   another agent, an amended contract or plan, other `read` or `targets`, or
   a split. Or it closes the step.
5. **It shares what it learned.** What it found about the area goes into
   memory as `knowledge`, so the next order there carries it.

## Shared knowledge

What an agent learns about an area of the code (how it works, its key
files, conventions, traps, how to test it) is saved in ai-memory as
`knowledge` with its `area`: a lane (`back`) or a folder
(`backend/app/Services/`). Every order whose lane or files fall in that area
carries it, so the next worker starts from what the team knows instead of
reading the code again. The leader curates it: after investigating an area
it saves what it learned, and replaces what is outdated.

## Steps

| Status | Meaning | Who acts |
|---|---|---|
| ordered | An agent works on it | the agent |
| reported | Done and verified; waits for review | leader |
| failed | Its check or the merge check failed | leader |
| blocked | The agent couldn't go on | leader |
| rejected | Sent back | leader: order it again or close it |
| accepted | Kept; merged when it was a lane's work | — |
| closed | Ended without its work: report used, or not needed | — |
| exhausted | Failed after every attempt and the retry | leader: more attempts, another approach, or close it |

The board's "waiting for the leader" list is every reported, failed, blocked,
rejected and exhausted step.

## Checks, snapshots and lanes

- **Snapshot.** A container sees only files that match `source_paths` and
  none of:
  - `protected_paths`;
  - the built-in rules: `.git`, `.env` and `.env.*`, `node_modules`,
    `vendor`, `storage`, database and key files…

  A dotenv template (`.env.example`, `.env.sample`, `.env.dist`,
  `.env.template`) gets in only when `source_paths` names that exact path.
  `snapshot_path` says which rule applies to a path.
- **Profiles.** Each repo's checks are named profiles (`lint`, `typecheck`,
  `test`…), each with a command and an environment. A lane can override a
  profile (the back lane's `test` runs `back-check test`). An order runs its
  lane's view of the checks.
- **Targets.** An order's `targets` reach the check as `DEVAGENTICS_TARGETS`.
  A command that uses it checks only those files. The merge candidate then
  runs the whole suite once.
- **Evidence.** A check result belongs to one order, one commit and one
  configuration. When any of them changes, it's stale and runs again.
- **Output.** A container's output is kept to 64 KiB: its start and its end,
  where test runners print failures and the summary.
- **Test-first items.** An item that writes acceptance tests (`writes_tests`)
  fails a whole-suite check on its own: its tests wait for their
  implementation. Once its own checks passed, the implementation item can
  start in the same lane on top of it, though it isn't accepted yet. The
  two merge together, so the task branch never holds failing tests. Every
  other dependency must be accepted first.
- **Catching up.** Before each order, a lane takes the task branch: by
  fast-forward, or by a merge when the branch's changes and the lane's
  changes touch different files. When both changed the same file, it's
  refused: git would combine them into code nobody reviewed.

## Accounts and usage

- A new agent tries its role's programs, then every account that is on for
  its group.
- An account at **95%** of its 5-hour or weekly window takes no new agents.
  You can approve going past that for the task. An account at **100%** is
  always skipped, since it can't run at all.
- An OpenCode account with no credit left takes no new agents.
- Switching an account off on the board's ACCOUNTS list stops new agents on
  it, for the whole task or one group.

## Memory and history

- **Memory** (ai-memory):
  - agents save short notes with `memory_save` (decisions, gotchas,
    handoffs, findings), and devAgentics saves each report;
  - **knowledge** notes describe an area of the code and go with every order
    there (see [Shared knowledge](#shared-knowledge));
  - when the leader moves, devAgentics writes a digest of where the task
    stands. The next leader reads the digest and the latest handoffs, then
    re-learns the project.
- **History** in the repo: each accepted step commits the accepted plan,
  execution plans and decisions to `.devagentics/history/<task>/` on the
  task branch, so the work can continue from another machine.

## When something goes wrong

What the leader can see:

- the INTEGRATION FAILED message, with the failing log lines;
- `read_check_log`: any check run's outcomes and the end of its logs;
- `read_file`, `search_code`, `git_log`, `git_diff`: the code, its history and
  what an order changed;
- `task_status` `failures`: why each open step failed before;
- `snapshot_path`: why a file is or isn't in a container;
- a lost worker's last error, in its "panel is gone" message;
- `task_status`, `read_log`, `leader_inbox`.

When those point at devAgentics itself, the leader escalates. Then read
`~/.devagentics/tasks/<id>/log.jsonl`, the run folders under `checks/` and
`candidate-checks/`, and `workers/<agent>/control/turns.jsonl`.
`devagentics task status <id>` prints what the leader sees.

## Legacy tasks

Tasks from before the reviewed workflow keep working the old way:

- agents run as interactive panes, with their program's own tools;
- they report with `devagentics report`, ask with `devagentics ask`, and
  seniors store plans with `devagentics plan write`;
- a step's check is one command (`set_check`) run on the host;
- accepting a step merges its lane directly, without a separate review;
- the leader keeps its program's own tools (it can run `git diff` itself),
  but the failure history, the diagnosis rule and shared knowledge work the
  same way.
