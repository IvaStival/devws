# Agent teams

An agent team is a workspace for working with several agents at once. You talk
to one of them, **the leader**; the leader opens the other agents (a backend
one, a frontend one, tests…) as it needs them, gives each its orders and
reads what they report. devws shows the panels; the team itself is
`devagentics`, the Rust program in this repo's `agentic/` folder, which devws
asks to make the task and to run the board. `./install.sh` builds it when Rust
is installed; without it, `devws agentic` says so and opens nothing. How a team
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
│ ☰ Menu│ ▾ Leader · claude│ Team board               │
│       │                  │ Who does what            │
│ my-app│                  │ Seniors                  │
│ ▸ team│                  │ ui-designer  1 ● ui-de…  │
│ my-app│                  │ Factory floor            │
│ -floor│                  │ hammer-codex 2 ● hamme…  │
│  ◦ ha…│                  │                          │
└───────┴──────────────────┴──────────────────────────┘
```

- **The leader** is where you work: the team's CTO. It is focused when the
  workspace opens. Its team has **seniors**, who plan and answer questions
  and never write the repo, and the **factory floor** (the hammers), who build
  only what a senior planned. When its account has used 95% of its week, the
  leader moves to Codex Sol (medium): at start, or once it has finished its
  turn; the new leader catches up from the task and tells you.
- **The board** beside it, in a column of its own, shows where the team stands: the accounts' limits,
  one line of state, the open questions, **Who does what** (every role there is, by group, running or not,
  with what each running agent is doing and what comes next), the steps to
  review, the orders waiting, and the steps done. When the pane is short
  it shows a compact view; `z` zooms it to the whole board, `j`/`k` or the mouse wheel scroll (`PgUp`/`PgDn` a page, `g`/`G` top and end), `l` for the log of events (scrollable the same way), `Enter` to go back to the leader, `1`–`9` to an agent, `p` and a number to peek one, `q`
  to close. It also runs the dispatcher that types orders into the agents.
- **The agents** open in workspaces of their own, one per group, beside the
  team's: **Seniors** (`my-app-seniors`) and **Floor** (`my-app-floor`), four
  agents to a workspace, tiled beside a sidebar of their own; a fifth floor
  agent opens `my-app-floor-2`. In the sidebar they are a tree under the
  team's workspace, each group with its agents (`└ Floor ●` while one waits,
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
  workspaces of the team's other repos.

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
