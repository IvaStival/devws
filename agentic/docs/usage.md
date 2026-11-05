# Using devAgentics

devAgentics runs a team of agents on one task. You talk to one of them, the
**leader**; the leader opens the others (a planner, executors, any role you
write), gives each one order at a time, checks the results, and opens linked
draft pull requests for you to review. devws, the repo this folder is in,
shows it all: the panels, the states, the worktrees.

## Install

`./install.sh` in the devws checkout builds `devagentics` from this folder when
Rust is installed (`cargo install --path agentic --locked`, by hand if you
prefer). `devws agentic` and the `devws agent …` commands are in devws itself.

## Start a team

```sh
devws agentic                         # choose the repo and the branch
devws agentic owner/repo feat/login   # a branch of a GitHub repo
devws agentic ~/Projects/my-app       # a folder as it is
```

devws makes the worktree, calls `devagentics task start <folder>`, and opens
the workspace: the leader on the left, the team board beside it, the agents
under the board. Tell the leader your goal.

## The groups

Every role is in one of three groups (`group` in its file):

- **CTO**: the leader. It talks to you, starts the agents, sends every order
  and decides what is done.
- **Seniors** (a planner, a designer, a QA…) never write the repo. They plan
  an execution and answer questions about it. Every file is forbidden in
  their orders, so a change fails their report.
- **Factory floor** (the hammers) write the repo, and only run steps from a
  senior's execution plan that the CTO accepted.

## What happens

1. **Plan.** The leader has a senior write the overall plan, goes through it
   with you, and asks whether you approve. When you say yes in the leader's
   chat it records it (`approve_plan`) and goes on; you can also run
   `devagentics task approve <task>`. Agents that write the repo cannot start
   before.
2. **Execution plans.** For each part, the leader orders a senior to plan its
   execution: steps for the factory floor, with their files and what done
   means (`devagentics plan write` again, now for its step). The leader reads
   it (`read_plan`) and accepts that step, which approves the plan; you are
   not asked again.
3. **Lanes.** The leader splits the work by area with `add_lane`: back
   (`backend/**`), front (`frontend/**`), infra, docs… Each lane is a worktree
   of its own (`lanes/<name>` in the task's folder) on a branch off the
   task's (`feat/x--back`), and one hammer writes it, so hammers of different
   lanes build at the same time. Before each order a lane takes in what the
   other lanes merged; accepting a step merges its lane into the task's
   branch. Lanes must not share files: a clash is undone and named.
4. **Work.** The leader keeps one hammer per lane and gives it the next step
   of its lane when it finishes: an order naming the plan (`plan`), the step,
   what done means, and its files (the lane's, unless `allowed` narrows them;
   `forbidden` excludes some). An agent has one order at a time. The order is typed into the agent when it
   is done or idle, never in the middle of its work. The hammer reads the plan
   (`devagentics plan show <step>`); when something is unclear it asks
   (`devagentics ask "…"`) and the senior who planned it answers
   (`devagentics answer q… "…"`), or the leader (`answer_question`) when that
   senior is gone. The answer is typed back into the hammer.
5. **Check.** When the agent reports (`devagentics report done "…"`), devAgentics
   checks the work itself, not the agent's word:
   - the tree is clean (the work is committed);
   - only the allowed files changed, none of the forbidden ones;
   - the repo's check command passes (`make test`, set with `set_check` or in
     the order; `true` for an order that changes no files).
   What it finds goes to the leader. A step that fails any of them is failed;
   one that passes waits for the leader's review (`accept_step`/`reject_step`).
6. **Pull requests.** When every step is accepted, `open_prs` pushes each
   repo's branch and opens a draft pull request per repo, each linking the
   others. You review them.

The leader asks you (`escalate`) for what it cannot decide.

## The board

The column beside the leader runs `devagentics board <task>`. The agents are
not on that screen: devws opens each in a workspace of its group
(`my-app-seniors`, `my-app-floor`, four agents each), listed under the task in
the sidebar, and the board gives each running agent a
number to reach it.

- **Accounts**: how much of each running agent's 5-hour and weekly limit is
  used.
- **Usage**: what the leader and each agent have used: input, output and
  cache tokens, and what they cost at API prices (a plan is not billed so; it is
  what the tokens would cost, a `+` when a model has no price), with the team's
  total. Read from each agent's own log, so it needs devws's `devws agent usage`.
- **State**: the plan (none, written, approved), agents working, steps
  accepted, what waits to be told to the leader, paused.
- **Questions**: what a hammer asked and who has to answer.
- **Who does what**: every role there is, built in or yours, under its group
  (CTO, Seniors, Factory floor), whether it runs or not. Each running agent
  has a number, and a hammer its lane (`hammer-codex-1 [back]`): press the
  number to go to the agent's workspace; `p` and the number brings
  its panel under the board for a look, and the same again sends it back. A running agent shows its state and the step it has an order for;
  `next` lists the orders it has not been told yet. A role that is not running
  shows its program and its `description`.
- **Review**, **Queue** and **Done**: the steps the leader must act on, the
  orders waiting to be typed in, and the accepted steps (a box with nothing
  in it is left out).

A short pane starts compact. `z` zooms it to the whole board;
`j`/`k` (or the arrows, or the mouse wheel) scroll, `PgUp`/`PgDn` (`b`/space)
a page, `g`/`G` to the top and the end. `l` shows the log of events, newest at
the bottom, with the lines on screen; scrolled back, it stays on them as new
ones come in, and `G` returns to the newest; `1`–`9` goes to an agent and
`p` with a number peeks it; `Enter` goes back to the leader,
`r` reads the team again, `q` quits. The board also runs the dispatcher while
it is open; `devagentics board <task> --once` prints it as text, and does not.

## Rules devAgentics keeps

- One agent that writes per worktree: per lane when the repo has lanes, and
  then only in a lane.
- No agent that writes before you approve the plan; a new plan needs approval
  again.
- At most `max_agents` running (6), the seniors included. Agents stay between
  steps; the leader stops them at the end, near a limit, or to make room.
- No new agents on an account past 85% of its 5-hour limit.
- A senior changes no files; the factory floor runs only an accepted plan's
  steps.
- Agents talk to the leader, and a hammer to the senior whose plan it
  follows. They cannot push, open pull requests, or start agents.

## Commands

| Command | What it does |
| ------- | ------------ |
| `devagentics task start <folder>` | Make the task for a worktree, or find it; prints its id, the leader's agent and its arguments file (what `devws agentic` calls) |
| `devagentics task approve <task>` | Approve the plan (the leader does it when you say yes) |
| `devagentics task status <task>` | Where the team stands, as JSON |
| `devagentics task pause\|resume\|cancel <task>` | Stop new deliveries and agents; start again the agents whose panels went; stop every agent |
| `devagentics task list` | The tasks |
| `devagentics role list [--repo <path>]` | The roles and where each comes from |
| `devagentics plan write` | Store a plan from standard input (a senior runs it): the overall plan until you approve one, then the execution plan of the step it was ordered |
| `devagentics plan show <step>` | Print a step's execution plan (a hammer runs it) |
| `devagentics ask "<question>"` | A hammer asks about its order; the senior who planned it is told |
| `devagentics answer <q…> "<answer>"` | A senior answers; the answer is typed into the hammer |
| `devagentics report done\|blocked "<text>"` | An agent reports (run by the agent, in its pane) |
| `devagentics board <task> [--once]` | The team board (and the dispatcher): the workspace's team pane. `--once` prints it as text |
| `devagentics watch <task> [--once]` | The dispatcher alone, with the log as plain lines |
| `devagentics mcp <task>` | The leader's tools, as an MCP server on stdio |

The leader's tools: `task_status`, `approve_plan`, `spawn_agent`, `add_lane`, `add_repo`,
`send_order`, `read_plan`, `answer_question`, `accept_step`, `reject_step`, `stop_agent`, `set_check`, `read_log`,
`escalate`, `open_prs`.

## Roles

A role is a Markdown file: settings between `+++` lines, then the agent's
instructions.

```markdown
+++
agent = "codex"        # a DEVWS_AGENTS name: claude, codex, claude_work…
model = "gpt-5"        # optional
effort = "high"        # optional
writes = true          # whether it changes the repo it works in
group = "floor"        # cto, senior or floor; by default floor if it writes, else senior
sandbox = "workspace-write"   # optional (Codex)
approvals = "on-request"      # optional: Codex's -a, or Claude's permission mode;
                              # "auto" works without asking you (Claude's auto
                              # mode, Codex's --approve-for-me)
description = "Writes the tests"   # optional: the board's line for the role
+++
You write the tests for the step you are given…
```

Only the `leader` is built in. Every other role is a file of yours: put them in
`~/.devagentics/roles/<name>.md`, or in a repo's `.devws/roles/`; a file with
a file named `leader` replaces the built-in one, and a repo's beats yours.
`roles/samples/` in this repo has a planner (a senior) and an executor (the
factory floor) to copy from; the
leader sees the roles there are in `task_status` and uses only those.

## Where things are

`~/.devagentics/tasks/<task>/` (`DEVAGENTICS_HOME` changes it) holds:

| File | What |
| ---- | ---- |
| `task.toml` | The branch, the workspace, the repos, their checks |
| `state.json` | Agents, steps, orders, plans, questions, what waits to be delivered |
| `log.jsonl` | Everything that happened, written by devAgentics only |
| `plan.md` | The plan |
| `plans/<step>.md` | The seniors' execution plans |
| `lanes/<name>/` | Each lane's worktree |
| `leader.args`, `agents/*.args`, `mcp.json` | How the agents were started |
| `checks/<step>-<n>.log` | Each check's output |

The state is exact and kept under a lock; ai-memory is for what the team
learned, not for where it stands.

## After a restart

`devws restore` leaves an agent team alone. Run `devws agentic <folder>` to
get the leader back (it finds the same task), then `devagentics task resume
<task>` to start the agents whose panels went, each told its open order again
in a new conversation.

## Settings

| Variable | Default | What |
| -------- | ------- | ---- |
| `DEVAGENTICS_HOME` | `~/.devagentics` | Where tasks and your roles are |
| `DEVAGENTICS_DEVWS` | `devws` | The devws command |
| `DEVAGENTICS_TMUX` | `tmux` | The tmux command (for pane markers) |
| `DEVAGENTICS_GH` | `gh` | The GitHub CLI |
| `DEVAGENTICS_CHECK_TIMEOUT` | `900` | Seconds a check may run |

## Known limits

- Claude and Codex only; other programs (OpenCode, OpenRouter models) are not
  there yet.
- A message for the leader is typed in when the leader is done or idle. If you
  are typing in its input at that moment it lands in your draft.
- A planner or executor starts by answering "ready"; its first order waits for
  that answer.
