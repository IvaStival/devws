# Using devAgentics

[How devAgentics works](how-it-works.md) explains the pieces, a step from
start to finish, and every tool the leader and the workers use.

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
accepted work (braille dots, `⣿` done and `⣀` left, which every terminal
draws as a texture): a reviewed task counts its baseline's items
(`4/56 · 7% · +24 helper steps`, the leader's plans, preflights and
corrections apart), a legacy task its steps; the team's cost at API prices with its input, output and
cache tokens (every agent the task has had), and a row per account with its
5-hour and week limit bars (green, yellow from 60 %, red from 85 %; narrow
panes keep the 5-hour bar). A pane too short for the frame keeps its top row.

The default **Overview** is one list of sections, each after a blank line and
left out when empty: **Needs you**, only what you must act on (the leader's
question to you, `● leader asks: …`, unanswered questions, leader calls
waiting for your approval — `⚠ p7 open_prs · external · {…}`, with the
exact arguments you approve — failed steps, agents waiting or gone,
a dispatcher problem); **Preview**, while the team runs an application for
you to look at (`◆ http://127.0.0.1:13000 · 3h40 left`, see
`preview_start`); **CTO**; **Leader reviewing** (steps reported,
failed, blocked or sent back, which the leader acts on); **Leader backlog**
(the steps it left); **Seniors** and
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
the **Leader backlog** card under the CTO, with how long it has waited. It is
the leader's work, not yours, so it stays out of Needs you; to remind the
leader, write to it from its details. In the details of a NEEDS YOU row, a
step or the leader, Enter (or `m`, a click in the "To the leader" box, or a
paste) starts a message, and Enter sends it; an empty message is never sent.
The
frames use the devws menu's muted blue accent, while green marks successful
states. Selection skips section titles, second lines and blank lines, and the wheel
scrolls the whole list, down to the oldest done step. Panes under 60 columns
drop the cost and token columns.

The board reads the team (states, usage, limits) in the background, so slow
readings never hold up keys or the wheel; every pending wheel turn is applied
before the board draws again.

Use Tab / Shift-Tab to move between six views:

- **Overview** shows what needs you, what is being built (the BUILDING card: the
  first part of the plan not done yet and its items still to do, in plain
  words), the team and the work done.
- **Agents** lists running and lost agents by group. `c` also shows the inactive
  role catalog. Each row distinguishes pane activity from its current order.
- **Plan** explains the plan's codes: each part (`D`: its release and what it
  delivers) with its progress, each item in plain words (`D4  Move reusable
  evidence inspectors and add system side panel`) and its state, and under it
  what serves it now (its plan step, the agent on it) and its tests (`D4-AT`,
  written first). Enter shows what the item does, when it is done and what it
  needs first. An agent's "now" line also names the item a helper step is for.
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

The **ACCOUNTS** card, under the leader, has a switch for every account
registered in devws (`devws agent accounts`): `[━━● on ]` or `[○━━ off]`, with
its 5-hour and weekly use. Select one and press **Space** to turn it on or off
for this task. No new agent starts on an account that is off: a role falls
through to its next program that is on, and the leader never moves to one.
Agents already running keep running (a leader on an account you turned off
says `leader still on it`). The last account that is on stays on. The leader
hears every change and sees the switches in `task_status` (`accounts`, with
`enabled` for the global switch and `leader`/`senior`/`floor` for each
group's, with each group's picked model).

These are the global switches. Under them, **LEADER ACCOUNTS**, **SENIOR
ACCOUNTS** and **FLOOR ACCOUNTS** have a switch per account for that group of
this task: an agent starts only on an account that is on both globally and for
its group (a group switch that is on while the global one is off reads `held
off globally`). Each group keeps at least one account on.

LEADER ACCOUNTS lists the accounts that can lead this task. The leader never
starts on, moves to or is picked onto an account that is off for it, even
when its own has run out, while the workers may keep using that account.
Turned off for the account it leads on now, the leader moves to its other
program once its turn is over; when that one cannot take over, the switch
stays on and the board says why.

OpenCode is offered to the floor only. The `opencode` row under ACCOUNTS is
OpenCode's global switch and the one under FLOOR ACCOUNTS its floor switch.
Both switches are the home-wide settings of the devws menu's OpenCode screen
(`opencode.toml`), so they hold for every task. The senior OpenCode switch
stays in that menu, off by default.

The accounts are the ones devws runs (`devws agent accounts`): those
registered in `DEVWS_AGENTS`, then each installed program (Claude Code, Codex,
Antigravity) that no registered name runs, by its bare name.

Each row under LEADER, SENIOR and FLOOR ACCOUNTS shows the model that group's
agents start on that account with. The leader starts on a model picked for it
the next time it starts on that account, at a handover or a restart, since a
restart loses its conversation. A model marked `*` is a default, nothing picked
for this task: each of the group's roles brings its own (the floor's rule
applied), and a role that names none runs its program's own (`devws agent
default-models`: the account's config, else Codex's first listed model,
Claude's Opus, agy's Gemini 4 Carbon). OpenCode's default is the devws menu's
model. **Enter** on it
drops a picker down under the row: **Default**, the models devws knows for the
program (for opencode and Antigravity, what `opencode models` or `agy models`
lists with your logins, `devws agent models <account>`), and **Type a model…**
to write any other. The pick
holds for this task only and replaces the role's model for that group on that
account; Default goes back. For opencode, Default is the devws menu's model.
The leader hears every change, and `task_status` shows the picks (`accounts`:
`senior_model`, `floor_model`). Agents already running keep their model.

Details are laid out for reading: `Label: value` lines with the label in
bold, JSON arguments over indented, tinted lines, inline numbered items and
long paragraphs one per line, and `code` and **bold** marked. A NEEDS YOU
row turns grey once you open its details and shows `✉` once you write to
the leader from it. The board keeps this in the task folder
(`board-seen.json`), and a row whose text changes is new again.

The details of every NEEDS YOU row (a permission, a question, an agent that
needs you, a step the leader left), of a step, and of the leader have a **To the
leader** box: press **m** to write in it, **Enter** sends, **Esc** cancels. The
leader gets it with what it is about (the permission's call and status, the
question, the step's status and attempts) and is woken to read it; a leader
waiting for you takes it as your answer.

`devagentics board <task> --once` prints an overview without a dispatcher;
`--view overview|agents|steps|activity|usage` selects another view, and
`--width`/`--height` set the printed dimensions.

## Rules devAgentics keeps

- One agent that writes per worktree: when the repo has lanes, only in a
  lane, one per seat. A lane's first writer takes the lane's worktree; another
  takes the lane's next seat, a worktree and branch of its own with the lane's
  files, image and checks, up to `max_seats` (2, in `task.toml`). Two seats
  never hold orders whose files may overlap: their `allowed` globs must part
  at a folder before a wildcard (`web/Team/**` and `web/History/**`), or the
  order is refused with the other order named.
- No agent that writes before you approve the plan; a new plan needs approval
  again.
- At most `max_agents` writers running (6), and as many read-only agents
  (planners, reviewers) apart, so an idle senior never keeps a writer from
  starting. An agent with nothing open (no order, message, question or review, and for a
  writer no step waiting to merge) is closed after `idle_close_minutes` (30, in
  `task.toml`; 0 keeps them), a reviewer after 5: idle, it uses no tokens but
  holds memory, and woken hours later it pays for its whole conversation again.
  It leaves a handoff note in the team's memory first, which the next agent
  finds with `memory_search`. The leader also stops agents at the end, near a
  limit, or to make room.
- No new agents on an account at 95% of its week or five-hour limit. Workers
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
  - **when asked**: tell the leader (it calls `switch_leader`), or run
    `devagentics leader switch <task>`. Refused when the other account has
    run out.
  - **to any account you pick**: **⇄ Change leader…** in the devws menu's
    Team tab, or `devagentics leader use <task> <account>`
    (`devagentics leader choices <task>` lists those that can lead). It
    takes over once the leader's turn is over and leads from then on.
    Refused for an account that is off, has run out, or cannot lead.

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
| `devagentics leader switch <task> [--reason "<why>"]` | Move the leader to its other program once its turn is over |
| `devagentics leader use <task> <account> [--reason "<why>"]` | Move the leader to that account once its turn is over (the devws menu's Change leader) |
| `devagentics leader choices <task>` | The accounts that can lead, and how each stands |
| `devagentics role list [--repo <path>]` | The roles and where each comes from |
| `devagentics role set <role> [--model M] [--effort E] [--approvals A] [--repo <path>]` | Changes those settings in the role's file; an empty value removes one |
| `devagentics role choices <role> model\|effort\|approvals [--repo <path>]` | The values the role's program takes for a setting |
| `devagentics plan write` | Store a plan from standard input (a senior runs it): the overall plan until you approve one, then the execution plan of the step it was ordered |
| `devagentics plan show <step>` | Print a step's execution plan (a hammer runs it) |
| `devagentics references url\|list <task>` | The design references page's address (with its key); what waits for you and what you answered |
| `devagentics references give <task> <image> [--note "…"]` | Give the designers an image of your own |
| `devagentics references answer <task> <step> --approve r1 --reject r2,r3 [--comment-on r1="…"] [--comment "…"]` | Answer a step's waiting references without the page: each one approved or rejected |
| `devagentics ask "<question>"` | A hammer asks about its order; the senior who planned it is told |
| `devagentics answer <q…> "<answer>"` | A senior answers; the answer is typed into the hammer |
| `devagentics report done\|blocked --order <order-id> "<text>"` | An agent reports (run by the agent, in its pane) |
| `devagentics board <task> [--once] [--view <view>]` | The team board (and the dispatcher): the workspace's team pane. `--once` prints it as text |
| `devagentics watch <task> [--once]` | The dispatcher alone, with the log as plain lines |
| `devagentics mcp <task>` | The leader's tools, as an MCP server on stdio |
| `devagentics task leader-cli <task> on\|off` | Opt a reviewed task in or out of direct Leader status reads, on any leader program whose shell can be held to that one command (Codex; Claude Code 2.1.331, the versions `scripts/probe_claude_leader.sh` passed; OpenCode 1.19.6); otherwise the leader stays on MCP |
| `devagentics leader-read <task> progress` | Compact JSON progress from the opted-in task's primary worktree root |
| `devagentics leader-read <task> order <id>` | Compact JSON order and check evidence from the same worktree root |
| `devagentics leader-read <task> inbox` | Pending Leader messages, roster and ready work. Acknowledge handled IDs through MCP. |
| `devagentics leader-read <task> guide <topic>` | A part of the leader's role its prompt leaves out (`accounts`, `reviewed`, `legacy`); the same text as `application_workflow action=guide` |

The leader's tools: `task_status`, `approve_plan`, `spawn_agent`, `add_lane`, `add_repo`,
`send_order`, `read_plan`, `design_references`, `answer_question`, `accept_step`, `reject_step`, `close_step`, `allow_attempts`, `read_check_log`, `snapshot_path`, `read_file`, `search_code`, `git_log`, `git_diff`, `stop_agent`, `set_check`, `read_log`,
`escalate`, `switch_leader`, `open_prs`.

## Roles

A role is a Markdown file: settings between `+++` lines, then the agent's
instructions.

```markdown
+++
agent = "codex"        # a DEVWS_AGENTS name: claude, codex, antigravity, claude_work…
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
references = false     # optional: shows the user design references and plans
                       # only from the ones they approve (a designer)
+++
You write the tests for the step you are given…
```

### The team's history in the repo

A task's state lives in `~/.devagentics/tasks/<task>/`, on the machine that
runs it. So that the work can go on elsewhere, every accepted step also
writes the team's history into the repo and commits it on the task's
branch: `.devagentics/history/<task>/` with `history.json` (the accepted
steps, in order, with their spec's release version; whether the plan was
approved), `plan.md` and the seniors' execution plans in `plans/`. The
commit (`chore(team): record 1.83.2 …`) takes only those files, so an
agent's work in progress in the same worktree is left alone, and the work
check ignores that folder. When saving fails, the step stays accepted and
the log says why.

Push the branch, start the team on it on another machine (`devws agentic`),
and the new task loads that history: Done and its release versions are as
they were, `next_version` goes on from the highest, the plan is still
approved, and the execution plans are back. The leader is told it continues,
reads `task_status` and the plan, and tells you what is done and what comes
next. Agents and open orders do not carry over: the leader starts new ones.

`devagentics history save <task>` writes and commits the history at any time,
for example before you push the branch to carry on elsewhere.

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
keep theirs. The leader is not changed this way: Change leader moves it.

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
learned, not for where it stands. With ai-memory installed, devAgentics keeps
each report, each step decision and, when the leader moves, where the task
stood and the outgoing leader's handoff, in the `devagentics-tasks` project
(`tasks/<task>/`). A new leader opens with them, and an order carries what the
agents before saved about its step. Agents add notes with `memory_save` and
find them with `memory_search`. See them in ai-memory's web UI (the devws
menu's Agent › Memory…).

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

## Antigravity agents

An Antigravity account (`agy`, kind `antigravity` in devws) runs any role in a
normal task, the leader too. Its arguments are `-i <first message>`,
`--model`, `--effort` and `--mode`. `--mode` is the role's `approvals`
(`accept-edits`, `auto` or `plan`), else `accept-edits` for a role that writes
and agy's own asking for the rest. A role that writes also gets `--add-dir`
for the folders it writes in outside its worktree.

Its session's `--settings` let the `devagentics report|plan|ask|answer`
commands run without asking, as Claude's `--allowedTools` does, and, for the
leader, its tools (`mcp(devws_devagentics/*)`). agy reads MCP servers only
from plugins in its config folder: devws's plugin
(`~/.gemini/config/plugins/devws`, installed on every agy start) serves the
leader's tools from the `--mcp-config` file devAgentics passes, which devws
takes off agy's command line.

The floor runs Gemini Flash (`gemini-3.8-flash-medium`, or the Flash the role
names). With no fallback named, an agent falls back to Codex. A reviewed task
refuses Antigravity: it has no locked-down worker adapter.

## OpenCode workers

Floor and senior agents can run on [opencode](https://opencode.ai) to save
Claude and Codex usage, on a cheap model such as Zen's DeepSeek Flash. The
devws menu's **◆ OpenCode** screen (Agent tab) does it all; the same is
`devagentics opencode`:

```bash
devagentics opencode show                          # switches, models, account
devagentics opencode switch global on              # mandatory: off, nothing runs on opencode
devagentics opencode switch floor on               # and/or: senior
devagentics opencode model floor opencode/<model>  # from `opencode models`
devagentics opencode switch leader on              # a reviewed leader, when Claude and Codex run out
devagentics opencode model leader opencode/<model> # a stronger one you pay for
```

- **Switches.** The global switch is mandatory: while it is off, the floor's
  and the seniors' switches are kept but ignored (`show` says `ignored`), and
  no agent runs on opencode, even one whose role names it. With both on, a
  group's agents try opencode first, on the group's model, and fall back to
  their role's Claude or Codex program when opencode cannot start them. These
  switches do not move the leader. A reviewed task's leader runs on opencode
  when you pick an opencode account to lead it, or as its last resort: with
  `devagentics opencode switch leader on` and `devagentics opencode model
  leader <provider/model>`, a leader whose program and fallback have both used
  95% of a window moves to opencode on that model when the account has credit
  left (the devws menu's balance; unknown or zero credit keeps it waiting), and
  moves back to its own program, or else the fallback, once one has room. On
  opencode it runs in an isolated configuration with only the leader's tools;
  legacy tasks never use it. Saved
  in `~/.devagentics/opencode.toml`;
  agents started from then on follow it.
- **Account.** The screen registers the `opencode` devws agent (its folder is
  opencode's data folder's parent, `~/.local/share`) and runs `opencode auth
  login`: pick OpenCode Zen and paste its key. devws keeps no key.
- **Usage and credit.** What opencode agents used is in the board's USAGE
  card, the header and `devws usage`, at what Zen billed. Zen has no balance
  API: enter the balance on the screen (Set balance) and devws counts what was
  spent since against it. An account with nothing left takes no new agents;
  a credit or limit error from the provider makes the agent fall back too.
- **What each may do.** Legacy floor agents run `devws-floor` (edits allowed),
  seniors `devws-senior` (no edit tools). Reviewed workers use the managed
  MCP-only adapter: reads, edits, commands, checkpoints and reports go through
  authenticated worker tools. Read-only roles remain read-only.
- **Reviewed teams.** OpenCode floor and senior workers use the existing
  switches and models, whatever program the leader runs: a task put on Codex
  tries Codex before Claude, but OpenCode switched on for a group stays first. The adapter
  verifies its isolated configuration before becoming ready; unsupported
  versions or inherited tool configuration cause startup fallback. Conversations
  continue by exact session ID. After a turn claims work, a failure retains it
  for explicit recovery instead of automatically replaying it.

- **A refused worker moves on by itself.** When a worker's account refuses
  it (no credit, a usage or rate limit) before the worker did anything, the
  dispatcher restarts that worker on its next account with the same work, and
  the account takes no new agents of the task for 30 minutes (`task_status`
  accounts show `held`). A turn that already used a tool still waits for an
  explicit recovery.

- **Pick the account.** A new agent tries its role's programs, then every
  other account that is on for its group. Ask the leader for a specific one
  ("start a hammer on opencode"): `spawn_agent` with `account` starts it
  there or fails with the reason, with no fallback.

- **Verify a reply.** Ask the leader to call `probe_worker` with the ID of a
  ready, idle managed OpenCode worker. The worker uses its selected model and
  sends the result to the leader's inbox. The probe checks an exact reply
  with worker tools disabled. It creates no order, execution item, or checks.
  Reviewed work still goes through `send_order` with an accepted execution item.

## Known limits

- OpenCode supports floor and senior workers in legacy and reviewed tasks;
  the leader remains on Claude or Codex. The reviewed adapter targets verified
  OpenCode 1.19.6 on Linux and refuses other versions or platforms until verified.
  OpenRouter models are not there yet.
- An opencode account's credit left is an estimate from the balance you
  entered.
- A message for the leader is typed in when the leader is done or idle. If you
  are typing in its input at that moment it lands in your draft.
- A planner or executor starts by answering "ready"; its first order waits for
  that answer.

## Risk routing and budgets (reviewed tasks)

Each baseline item has a `kind` (change, bugfix, feature, interface,
investigation), and devAgentics computes how much process it gets from facts
about the item. No agent judges it.

- **light**: a change or bug fix in at most 3 file patterns that no other
  lane builds on. The leader accepts it after a passing merge check, with a
  written reason and no reviewer agent.
- **strict**: anything touching auth, migrations, SQL, Docker, CI or
  dependency files, changing stored data, sharing an interface with another
  lane, or with 2 failures of its own work. It gets a test-first item and a
  reviewer.
- **standard**: everything else, which is today's review.

The leader may raise an item's rigor, never lower it below the computed one.
Lowering an approved item's rigor needs you.

In `task.toml`:

```toml
routing = "off"        # every item at least standard: the full review, as before

[budget]               # optional; nothing is limited without it
max_tokens = 3000000   # what the team's models processed, as the panes report it (an estimate)
max_hours = 4
```

At 80% of a limit the leader is told once. At the limit the task pauses,
nothing is lost, and you are asked. To go on, raise the limit and run
`devagentics task resume <task>`.

## Reliable execution

See the [order, recovery and isolated-check guide](../../docs/user/devagentics-execution.md) for required report IDs, audited amendments, interrupt/resume, named check configuration and result formats.

A step gets the task's `max_attempts` (5, in `task.toml`) failures of its
work: failed checks, reviews asking for changes, and failures the leader
diagnosed as the work's. Re-checks after stale evidence, revision resets,
stopped agents and the environment cost none, and every re-order after a
failure carries the leader's diagnosis (see
[How devAgentics works](how-it-works.md#failure-history-and-diagnosis)).
Tasks started before this count from zero; a step that had already failed
needs one diagnosis. When they are used, devAgentics waits 15 minutes and gives
the step one more attempt on its own; if that fails too, the step shows a red
✕ "failed" and waits for the leader. The leader settles it without asking
you: when the next attempt can succeed, it gives the step more with
`allow_attempts` (its reason in the log and its next summary); otherwise it
changes the approach or closes the step. You can still allow more yourself
with `devagentics task attempts <task> <step> <limit>`. In reviewed tasks,
candidate checks test each merge candidate before its review, system checks
(`system` in a repo's check configuration) start the application and run its
smoke or end-to-end tests at every milestone, protected tests cannot be changed by the work they judge, and `devagentics task
codex-reviewers <task> on` lets Codex run the read-only reviewers.

## Measuring a team

`devagentics bench report <task>` (`--json` to collect runs) and `devagentics
bench compare <a> <b>` measure finished runs: first-pass acceptance,
corrections by cause, waits, what you had to do and the cost.
`scripts/bench_team.sh notes <legacy|reviewed>` prepares a fresh run of the
fixed `notes` fixture. See [Measuring an agent team](../../docs/user/devagentics-bench.md).

## Worker usage-limit approval

When every worker account the task may use has reached the local 95% guard,
or is switched off for the task, the leader asks for your
explicit approval to continue above it for this task. It records the answer with
`set_usage_limit_approval`, then calls `resume_task` after approval to restart
existing blocked workers. During resume, an unavailable worker with no pane
or outstanding work can stay available for retry if a different worker is
running. The leader's inbox explains why each worker was skipped.
Approval survives restarts until revoked and applies
only to this task. Tell the leader to revoke it to guard future launches again;
running workers are left in place. Denial is not repeatedly prompted. Provider
quota errors remain enforced. See the [usage-limit guide](../../docs/user/devagentics-execution.md#continue-above-the-worker-percentage-threshold).
