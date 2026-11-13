# Agent teams

An agent team is a workspace for working with several agents at once. You talk
to one of them, **the leader**; the leader opens the other agents (a backend
one, a frontend one, tests…) as it needs them, gives each its orders and
reads what they report. devws shows the panels; the team itself is
`devagentics`, the Rust program in this repo's `agentic/` folder, which devws
asks to make the task and to run the board. It needs Rust 1.85 or newer.
When `devagentics` is missing, `devws agentic` (and the menu's Open an agent
team) says what it needs and asks: one Y installs Rust (rustup) or updates it
(`rustup update stable`) as needed, builds `devagentics` (and `devws-ui`, the
menu's program), and opens the team.
`./install.sh` rebuilds it on every run, and asks before installing or
updating Rust. A Rust devws did not install without rustup (Homebrew's) is
left alone: update it yourself (`brew upgrade rust`). How a team
works (roles, orders, approval, limits) is in
[agentic/docs/usage.md](../../agentic/docs/usage.md).

## Start one

**◈ Open an agent team** in the menu, or:

```sh
devws agentic                        # the start screen: choose the repo and the branch
devws agentic owner/repo feat/login  # a branch of a GitHub repo
devws agentic ~/Projects/my-app      # a folder as it is
devws agentic --editor …             # with a Neovim panel too
devws agentic --leader claude_work … # the registered agent that leads
```

On a terminal, devws then asks which registered agent leads, the one leading
the folder's team now first. The pick stays with the task: opening it again
offers it first. An agent of the leader role's own program keeps the role's
model and effort; one of the other program runs with that program's defaults.
A picked leader starts even when its account is near its limit; it still
moves to the role's other program when it runs out.

The repo works as in [Workspaces](workspaces.md): a clone under `~/.devws`
and a worktree for the task, off the fresh default branch. The team's other
repos get their own worktrees the same way, as the leader opens them.

## The screen

```
┌───────┬──────────────────┬──────────────────────────┐
│ ☰ Menu│ ▾ Leader · claude│ [overview] agents steps  │
│       │                  │ Attention 1 · Done 12    │
│ my-app│                  │ Leader: claude           │
│ ▸ team│                  │ blocked s4 · add tests   │
│ my-app│                  │ 1 ● ui-designer · idle   │
│ -floor│                  │ hammer-codex 2 ● hamme…  │
│  ◦ ha…│                  │                          │
└───────┴──────────────────┴──────────────────────────┘
```

- **The leader** is where you work: the team's CTO. It is focused when the
  workspace opens. Its team has **seniors**, who plan and answer questions
  and never write the repo, and the **factory floor** (the hammers), who build
  only what a senior planned. The leader runs on Claude or Codex
  Sol (medium) and moves to the other when you ask (tell it, or pick
  **⇄ Switch leader model** in the menu's Team tab) or when its account runs
  out. The week comes first: with 95% of its week used, it moves when the
  other account's week has room, even if that one's 5-hour window is full.
  With only its 5-hour window used up, it moves only when the other has room
  in both, and stays when the other's week is used up. It moves once it has
  finished its turn; the new leader catches up from the task and tells you.
  Accounts at 85% of their week or five-hour window receive no new workers;
  workers automatically try compatible alternatives, including Codex when Claude
  is exhausted. See [worker availability](../user/devagentics-execution.md#continue-when-a-provider-or-model-is-unavailable).
  Starting, stopping and replacing this project's agents need no extra approval;
  actions outside the project, such as adding another repo or publishing PRs,
  still ask first.
- **⚙ Team roles** in the menu's Agent tab changes a role's model, effort or
  approvals (the Floor on Sonnet, a Codex senior that never asks): pick the
  role, the setting, then a value its program takes, or Default. Agents
  started after that use it. Codex agents in the team's own lane folders start
  without asking whether to trust the folder.
- **The board** beside it opens on **Overview**, under a header with the
  progress bar and each account's limit bars. Each part is a card: what needs
  you, the CTO (the leader first), the seniors, the floor, the queue, the usage
  (each agent's tokens and cost, then the team's total) and the steps done,
  newest first. Only the Done card scrolls, with its own scrollbar; the others
  stay put. Muted blue frames match the devws menu; green marks successful
  states. A pane too short for the cards shows them as one scrolling list.
  Tab / Shift-Tab switches to **Agents**, **Steps**, **Activity**, and **Usage**.
  Completed work stays in Steps (`c` includes it); inactive roles stay in the
  Agents catalog (`c` shows it). Use `j`/`k`, arrows, or the wheel to select a
  row; Enter opens full details in a dialog inside the info panel, as tall
  as its content and next to the selected row, with the board around it. Arrows, `j`/`k`, the wheel, PgUp/PgDn and Home/End scroll
  the dialog; Esc or `q` closes it and keeps the selection. With the mouse,
  click a row to select it, click it again for its details, click a view's
  name to show it, and click outside the details to close them. `o` goes to a selected agent,
  `v` peeks it, and `L` returns to the leader. Number keys `1`–`9` and `p` then
  a number still work. Short panes preserve attention counts and scrolling;
  `z` zooms, `l` opens Activity, and `?` shows shortcuts in the same dialog. `q` closes details or
  quits the board. The board also runs the dispatcher while it is open.
  The popup's **Team** tab offers **Team overview**, **Team agents**, **Team steps**,
  **Team activity**, **Team usage**, and **Return to leader**.
- **The agents** open in workspaces of their own, one per group, beside the
  team's: **Seniors** (`my-app-seniors`) and **Floor** (`my-app-floor`), four
  agents to a workspace, tiled beside a sidebar of their own; a fifth floor
  agent opens `my-app-floor-2`. In the sidebar they are a tree under the
  team's workspace, each group with its agents (`└ ▾ Floor ●1` while one waits,
  then `◦ hammer-codex-1 ● needs you`); a group whose last agent stops
  closes. They are never paused while you are away. The team's workspace
  keeps only the leader and the board. Each
  agent's header shows its role and state (working, `● needs you`,
  `✓ done`) as for any agent.
- **Notifications.** An agent that needs you notifies you, as anywhere. An
  agent finishing an order does not: the agents finish one after every
  order. Instead the team notifies once when it stops: `<task> ✓ team
  stopped` once nobody has worked or waited for 20 s and no order is queued
  (a paused task's queue does not count), saying why: all steps accepted,
  paused, or idle. Clicking it goes to the leader. Not while you are looking
  at one of the team's workspaces; `DEVWS_NOTIFY=waiting` or `0` turns it off.
- **Reaching an agent.** On the board each running agent has a number: press
  it to go to its workspace, focused on it; Enter on the team's workspace in
  the sidebar brings you back. `p` and the number **peeks** it: its panel
  comes under the board for a look; `p` and the number again send it back.
  A click (or Enter) on an agent's row in the sidebar takes you to it too.
- There is **no editor**: the agents work in the folder and you read the
  leader. `--editor` adds a Neovim panel under the board.
- The sidebar lists the workspace under its task (`▸ team`), together with the
  workspaces of the team's other repos. Agent-team entries show the name and
  status first, then the model in a dim line below when known. Narrow sidebars
  use status icons and less indentation. Normal workspaces retain their branch,
  Git/PR, agent, and editor details. The current team opens its tree. Right expands and Left collapses teams or groups,
  with working/attention counts visible while collapsed. Search finds hidden
  agents and reveals their parent path. Manual expansion survives refreshes.

Because there is no editor, the actions that arrange the screen around one
(Toggle layout, Minimize, Switch editor) and the database pane are not
offered here. **New panel**, **Move panel** and the header work as usual.

## What the leader uses

The leader opens and drives the agents with the
[agent commands](commands.md#for-programs-agent-commands): `devws agent spawn
… --place team --label <role>` for a panel, `send` for an order (it waits
until that agent is done or idle), `state`, `stop`. You can run the same
commands yourself.

## After a restart

An agent team is saved but not rebuilt by `devws restore`: its leader and
agents are `devagentics`'s to restart, and restore says so. Bring the task back
with it (`devagentics task resume <task>`).

## Settings

`DEVWS_AGENTIC_COMMAND` names the agent team program (default
`devagentics`).
