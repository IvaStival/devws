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
5. **Check.** When the agent reports (`devagentics report done --order <order-id> "…"`), devAgentics
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

The column beside the leader runs `devagentics board <task>`. Above every
view, a rounded frame holds the task and branch with how many things need you
in its top edge (`● 2 need you`, hidden at none), the views, a progress bar of
accepted steps (braille dots, `⣿` done and `⣀` left, which every terminal
draws as a texture), the team's cost at API prices with its input, output and
cache tokens (every agent the task has had), and a row per account with its
5-hour and week limit bars (green, yellow from 60 %, red from 85 %; narrow
panes keep the 5-hour bar). A pane too short for the frame keeps its top row.

The default **Overview** is one list of sections, each after a blank line and
left out when empty: **Needs you** (unanswered questions, leader calls
waiting for your approval — `⚠ p7 open_prs · external · {…}`, with the
exact arguments you approve — steps the leader left, agents waiting or gone,
a dispatcher problem); **CTO**; **Leader reviewing** (steps reported,
failed, blocked or sent back, which the leader acts on); **Seniors** and
**Floor**, each with what its running agents cost and their tokens in all, and
each agent with its program (and lane), state, cost and tokens in columns,
over a dim line with its current step, the steps it got accepted and its
retries; the number of queued orders; and **Done**, finished work by spec:
one row per spec whose steps are all accepted, newest first, with its step
count and retries (Enter lists its steps). A step's spec is the `spec` its
order named (`send_order`); without one, its id up to the last numbered
part (`H2f.3` and `H2f-execution` are `H2f`, `RP-B3-support` is `RP-B3`), or
the step alone. Specs read as code releases: the task is major version 1,
each spec a minor release in the order it started (`1.1.0`, `1.2.0`…), each
of its steps a patch (`1.2.1`, `1.2.2`…), with the leader's own names in the
details. The leader names new work this way: `task_status` gives it
`next_version`, and a spec already named as a release (`1.87.0`) keeps it.
Done has a fixed height of 15 rows and scrolls through the rest. The leader
is the CTO's first row; Enter on it shows its pane and state, effective
permissions, fallback (active or standby), whether the plan is approved and
the task paused, and its pending permission requests. A step the leader has
left, waiting more than 10 minutes while the leader is not working, moves to
Needs you as `leader hasn't reviewed <step>` with how long it has waited:
tell the leader to accept or reject it. The
frames use the devws menu's muted blue accent, while green marks successful
states. Selection skips section titles, second lines and blank lines, and the wheel
scrolls the whole list, down to the oldest done step. Panes under 60 columns
drop the cost and token columns.

The board reads the team (states, usage, limits) in the background, so slow
readings never hold up keys or the wheel; every pending wheel turn is applied
before the board draws again.

Use Tab / Shift-Tab to move between five views:

- **Overview** shows what needs you, the team and the work done.
- **Agents** lists running and lost agents by group. `c` also shows the inactive
  role catalog. Each row distinguishes pane activity from its current order.
- **Steps** shows unfinished work, each row marked (`✓` accepted, `⠋` ordered,
  `●` to review, `⚠` failed, blocked or sent back). `c` includes completed steps.
- **Activity** shows events newest first; Enter reveals the complete event text.
- **Usage** shows account limits and the leader's and agents' token usage,
  including the team total and estimated API cost (`+` means an unpriced model).

The mouse works too: click a row to select it and click it again to open its
details, click a view's name in the header to show that view, and click
outside the details to close them. Use `j`/`k`, arrows, or the mouse wheel to select rows, PgUp/PgDn (`b`/space)
to move a page, and `g`/`G` or Home/End for the first/last row. Enter opens full,
wrapped details in a titled dialog inside the info panel, as tall as its
content (up to the pane), just below the selected row (above it when there
is no room below), with the board visible around it. The same movement keys scroll within the dialog; Esc or
`q` closes it and returns to the selected row. Selection and dialog scrolling
stay on the same item when the team refreshes; if that item disappears, its
dialog closes. Short panes keep attention counts and scrolling; `z` zooms the
pane and `?` shows all shortcuts in the same kind of dialog.

On an agent row, `o` goes to its workspace and `v` peeks it under the board;
peek again to send it back. The first nine agents also have direct `1`–`9`
shortcuts and `p` followed by a number to peek. Selected-row navigation works
with any number of agents. `L` returns to the leader by pane identity, `l`
toggles Activity, and `r` refreshes. `q` closes details/help, or quits the board.
The board runs the dispatcher while open, so quitting the board stops its
dispatcher.

The popup menu's **Team** tab opens these views or the leader, including from a
group workspace. It focuses the existing board and never starts another
dispatcher. Team sidebar entries show name and status on the first line and the model in a
dim line below when known. Long model identifiers wrap; narrow sidebars use
compact status icons. Normal workspaces retain branch, Git/PR, agent, and editor
details, with known models below the agent line. The current team opens
its tree; Right/Left expands or collapses it. Working and
attention counts remain visible on collapsed teams. Search includes hidden
agents and reveals their team path without replacing manual expansion choices.

`devagentics board <task> --once` prints an overview without a dispatcher;
`--view overview|agents|steps|activity|usage` selects another view, and
`--width`/`--height` set the printed dimensions.

## Rules devAgentics keeps

- One agent that writes per worktree: per lane when the repo has lanes, and
  then only in a lane.
- No agent that writes before you approve the plan; a new plan needs approval
  again.
- At most `max_agents` running (6), the seniors included. Agents stay between
  steps; the leader stops them at the end, near a limit, or to make room.
- No new agents on an account at 85% of its week or five-hour limit. Workers
  automatically try available alternatives, including Claude → Codex, without
  asking you again. Their role, lane and queued work stay the same. See
  [worker availability and configuration](../../docs/user/devagentics-execution.md#continue-when-a-provider-or-model-is-unavailable).
- Project-local agent start, stop and replacement require no extra approval.
  Adding another repository and publishing pull requests still ask first.
- **The leader has two programs**: its role's and its role's `[fallback]`
  (the built-in leader's are Claude, and Codex `gpt-6.1-sol`, effort medium).
  It moves from the one it runs on to the other, either way:
  - **on its own**, when the account it runs on has used 95% of its 5-hour
    window or of its week, unless the other's has too (then it stays);
  - **when asked**: tell the leader (it calls `switch_leader`), pick
    **⇄ Switch leader model** in the devws menu's agent tab, or run
    `devagentics leader switch <task>`. Refused when the other account has
    run out.

  At start the leader starts on its role's program, or on the other when
  that one has run out. While it runs, the board hands it over once it has
  finished its turn: its pane restarts on the other program, which catches up
  from `task_status` and the log (the conversation does not go with it) and
  tells you. The board's leader row says `· fallback` while it runs on the
  fallback, `task_status`'s `leader` shows both programs, and the log says
  why each move happened. The leader keeps its two-program policy; your own
  `leader.md` sets its fallback, or none (then it never moves):

  ```toml
  [fallback]
  agent = "codex"
  model = "gpt-6.1-sol"
  effort = "medium"
  approvals = "auto"   # Codex's reviewer answers, not you
  ```
- A senior changes no files; the factory floor runs only an accepted plan's
  steps.
- Agents talk to the leader, and a hammer to the senior whose plan it
  follows. They cannot push, open pull requests, or start agents.

## Commands

| Command | What it does |
| ------- | ------------ |
| `devagentics task start <folder> [--leader <agent>]` | Make the task for a worktree, or find it; prints its id, the leader's agent and its arguments file (what `devws agentic` calls). `--leader` names the devws agent that leads, kept for the task |
| `devagentics task leader <folder>` | The devws agent that leads the worktree's team now, or the leader role's with no task yet |
| `devagentics task approve <task>` | Approve the plan (the leader does it when you say yes) |
| `devagentics task status <task>` | Where the team stands, as JSON |
| `devagentics task pause\|resume\|cancel <task>` | Stop new deliveries and agents; start again the agents whose panels went; stop every agent |
| `devagentics task list` | The tasks |
| `devagentics leader switch <task> [--reason "<why>"]` | Move the leader to its other program once its turn is over (the devws menu's Switch leader model) |
| `devagentics role list [--repo <path>]` | The roles and where each comes from |
| `devagentics role set <role> [--model M] [--effort E] [--approvals A] [--repo <path>]` | Changes those settings in the role's file; an empty value removes one |
| `devagentics role choices <role> model\|effort\|approvals [--repo <path>]` | The values the role's program takes for a setting |
| `devagentics plan write` | Store a plan from standard input (a senior runs it): the overall plan until you approve one, then the execution plan of the step it was ordered |
| `devagentics plan show <step>` | Print a step's execution plan (a hammer runs it) |
| `devagentics ask "<question>"` | A hammer asks about its order; the senior who planned it is told |
| `devagentics answer <q…> "<answer>"` | A senior answers; the answer is typed into the hammer |
| `devagentics report done\|blocked --order <order-id> "<text>"` | An agent reports (run by the agent, in its pane) |
| `devagentics board <task> [--once] [--view <view>]` | The team board (and the dispatcher): the workspace's team pane. `--once` prints it as text |
| `devagentics watch <task> [--once]` | The dispatcher alone, with the log as plain lines |
| `devagentics mcp <task>` | The leader's tools, as an MCP server on stdio |

The leader's tools: `task_status`, `approve_plan`, `spawn_agent`, `add_lane`, `add_repo`,
`send_order`, `read_plan`, `answer_question`, `accept_step`, `reject_step`, `stop_agent`, `set_check`, `read_log`,
`escalate`, `switch_leader`, `open_prs`.

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
                              # mode, Codex's --approve-for-me). Without it,
                              # Claude runs the leader in auto mode, a role
                              # that writes in acceptEdits, the rest in default
description = "Writes the tests"   # optional: the board's line for the role
+++
You write the tests for the step you are given…
```

The factory floor runs on Claude Sonnet or Codex Luna only (`gpt-6-luna`):
a floor role's model, and its alternatives' when the account runs out, stay
only when they are a Sonnet or a Luna; anything else (Opus, Claude's
default) becomes one of them.

Only the `leader` is built in. Every other role is a file of yours: put them in
`~/.devagentics/roles/<name>.md`, or in a repo's `.devws/roles/`; a file with
a file named `leader` replaces the built-in one, and a repo's beats yours.

To change a role's model, effort or approvals without opening its file, use
**⚙ Team roles** in the devws menu's Agent tab, or `devagentics role set`. It
rewrites only those lines of the file the role comes from, and takes only the
values the role's program knows (Claude: effort `low` to `max`, approvals
`default`, `acceptEdits`, `plan`, `auto`, `bypassPermissions`; Codex: effort
`minimal` to `xhigh`, approvals `untrusted`, `on-request`, `never`, `auto`);
any model name may be typed. Agents started from then on use it; running ones
keep theirs. The leader is not changed this way: Switch leader model moves it.

Codex asks "Trust this folder?" in a folder its settings do not trust, and a
task's lanes are new folders. So when devws starts a Codex agent in a folder
inside `~/.devagentics/tasks` (`$DEVAGENTICS_HOME/tasks`), it adds that folder
as trusted to the account's `config.toml`, as answering the question would.
It never trusts another folder for you.
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

## Reliable execution

See the [order, recovery and isolated-check guide](../../docs/user/devagentics-execution.md) for required report IDs, audited amendments, interrupt/resume, named check configuration and result formats.
