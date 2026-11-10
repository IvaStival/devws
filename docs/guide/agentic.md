# Agent teams

An agent team is a workspace for working with several agents at once. You talk
to one of them, **the leader**; the leader opens the other agents (a backend
one, a frontend one, tests…) as it needs them, gives each its orders and
reads what they report. devws shows the panels; the team itself is
`devagentics`, the Rust program in this repo's `agentic/` folder, which devws
asks to make the task and to run the board. It needs Rust 1.85 or newer.
When `devagentics` is missing, `devws agentic` (and the menu's Open an agent
team) says what it needs and asks: one Y installs Rust (rustup) or updates it
(`rustup update stable`) as needed, builds `devagentics`, and opens the team.
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
```

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
  Sol (medium) and moves to the other when its account has used 95% of its
  5-hour window or week, or when you ask: tell it, or pick
  **⇄ Switch leader model** in the menu's Team tab. It moves once it has
  finished its turn; the new leader catches up from the task and tells you.
- **⚙ Team roles** in the menu's Agent tab changes a role's model, effort or
  approvals (the Floor on Sonnet, a Codex senior that never asks): pick the
  role, the setting, then a value its program takes, or Default. Agents
  started after that use it. Codex agents in the team's own lane folders start
  without asking whether to trust the folder.
- **The board** beside it opens on **Overview**, under a header with the
  progress bar and each account's limit bars: what needs you, the team (the
  leader first) and the steps done, newest first; the wheel scrolls all of it.
  Tab / Shift-Tab switches to **Agents**, **Steps**, **Activity**, and **Usage**.
  Completed work stays in Steps (`c` includes it); inactive roles stay in the
  Agents catalog (`c` shows it). Use `j`/`k`, arrows, or the wheel to select a
  row; Enter opens full details and Esc returns. `o` goes to a selected agent,
  `v` peeks it, and `L` returns to the leader. Number keys `1`–`9` and `p` then
  a number still work. Short panes preserve attention counts and scrolling;
  `z` zooms, `l` opens Activity, and `?` shows shortcuts. `q` closes details or
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
- **Reaching an agent.** On the board each running agent has a number: press
  it to go to its workspace, focused on it; Enter on the team's workspace in
  the sidebar brings you back. `p` and the number **peeks** it: its panel
  comes under the board for a look; `p` and the number again send it back.
  Enter on an agent's row in the sidebar takes you to it too.
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
