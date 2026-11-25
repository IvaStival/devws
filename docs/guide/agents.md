# Agents

Each workspace runs an agent (Claude Code, Codex, opencode or Antigravity) in
the pane under the editor. The menu's **Agent** tab handles it.

## Agents and accounts

An agent is a program in a config folder, so one program can run several
accounts, such as a personal Claude in `~/.claude` and a company one in
`~/.claude-work`. They are listed in `DEVWS_AGENTS`:

```sh
DEVWS_AGENTS="claude codex work"
DEVWS_AGENT_work="claude ~/.claude-work"
```

Plain `claude` and `codex` need no line: they run as the bare command does,
with no folder set. An installed program that no listed name runs (Claude
Code, Codex, opencode, Antigravity) is offered too, by its bare name, in
**⇄ Switch agent**, new workspaces and new panels, so it needs no
registering. Claude Code keeps a separate login for each folder set
and one for none, so `CLAUDE_CONFIG_DIR=~/.claude claude` can be signed into
another account than plain `claude`. A name with a line always runs with its
folder set, even `DEVWS_AGENT_claude="claude ~/.claude"`.

The account items are in the Agent tab's **☰ Accounts…** dialog (typing in
the menu finds them too):

- **＋ Register agent** asks for the program, offers the `~/.claude*` or
  `~/.codex*` folders not registered yet (or a path you type) and suggests a
  name (`work` for `~/.claude-work`). It edits `.env` and the header shows
  it at once. Log the account in once in its folder first, e.g.
  `CLAUDE_CONFIG_DIR=~/.claude-work claude`.
- **⌂ Change agent folder** picks a registered agent, then one of its
  program's folders (or a path you type), and writes its line in `.env`.
  Its running agents keep the old folder until restarted.
- **− Remove agent** takes one off the list; its folder is kept.
- Start a workspace with one by name (`devws ~/Projects/my-app work`), or
  change with **⇄ Switch agent**.

## The agent pane

- **⇄ Switch agent** restarts the pane with another agent; the workspace
  remembers it.
- **↻ Restart agent** starts the same agent again in a new conversation
  (`/resume` brings the old one back). Use it after an update changed
  devws's hooks.
- In an agent team the agent pane is the leader, so neither touches it:
  **⇄ Switch agent** opens the agent in a new panel instead, and **↻ Restart
  agent** points you to **Team › ⇄ Change leader**, which moves the leader.
- **◇ Initialize agent** seeds the project for the agent: `CLAUDE.md` and
  `.claude/skills/` for Claude, `AGENTS.md` and `.agents/skills/` for Codex
  and Antigravity,
  from devws's template (TDD, SOLID, naming, testing and planning rules).
  Existing files are never overwritten.
- **◎ Agent follow** (`C-b F`): how the editor follows the agent
  ([Layout](layout.md#following-the-agent)).

## Agent status

| State | Shown as | When |
| ----- | -------- | ---- |
| Working | `⠋` (turning) | From your prompt until its turn ends |
| Needs you | yellow `● needs you` | A permission prompt or a question waits |
| Done | green `✓ done` | Its turn ended; until your next prompt |
| Idle | nothing | Just started or restarted, or its turn was interrupted |

It shows in the agent's header, the minimized strip, the status bar and the
sidebar, for every workspace. Interrupting a turn (Esc) sends no "turn ended"
event, so devws reads it from the agent's transcript (Antigravity's from its
screen): within about 10 seconds the agent turns idle.

- **! Go to waiting agent** (`C-b a`) goes to the workspace whose agent has
  waited longest and focuses it (restoring it when minimized); again for the
  next. With none waiting, it goes to one that is done.
- When an agent in a workspace you are not looking at needs you or is done,
  macOS notifies you (workspace, branch, agent). Clicking it goes there. An
  agent team's agents only notify when they need you; the team notifies once
  when it stops ([Agent teams](agentic.md)).
  `DEVWS_NOTIFY=waiting` keeps only needs-you, `0` none. Clicking needs
  `terminal-notifier` (`./install.sh --deps`).
- Codex asks once to review devws's hooks; approve them, or its state
  stays empty.

## Antigravity

Google's Antigravity CLI (`agy`) runs as the kind `antigravity`. It keeps one
account, in `~/.gemini/antigravity-cli`, so it registers once. The Agent tab's
**◆ Antigravity** screen registers it, opens `agy` to sign in, and shows its
Gemini quota, this month's tokens and the models it can run.

agy takes no hooks or MCP servers on its command line, only from plugins in
`~/.gemini/config/plugins`. devws starts agy from its own plugin there,
`devws`, which it installs each time (the screen removes it). The plugin:

- reports the agent's state and the files it changes;
- adds devws's editor prompt;
- serves the team leader's tools.

It does nothing in an agy session that devws did not start. The status line
devws passes records the 5-hour and weekly quota for the header. The
header's cost column shows its month in tokens, since its models have no API
price there.

## Adding a program

Each program is a module: a folder in `config/tmuxinator/agents` for devws
and a provider in `agentic/src/providers` for agent teams. Adding a program
means adding those two; tests check that nothing else needs to change
([AI programs as modules](../architecture/agent-modules.md)).

## Agent memory (ai-memory)

[ai-memory](https://github.com/akitaonrails/ai-memory) gives the agents a
long-term memory: its hooks record each session, and its tool lets any agent
search what was done before, across agents, accounts and branch folders.

- **Install:** `./install.sh` offers it (`[y/N]`). Yes runs its installer
  and `ai-memory setup` (which shows its plan and asks), then devws wires
  every agent account it runs, `~/.claude-work` too. A no is remembered;
  `./install.sh --ai-memory` asks again. Registering an agent later wires
  it too.
- With it installed, the Agent tab adds a **⁘ Memory…** dialog:
  - **⁘ Search memory**: your question goes to the workspace's agent, which
    searches its memory and answers in its pane;
  - **◷ Recent memory**: what was done recently in this repo;
  - **↗ Open memory web UI**: ai-memory's page in the browser. It needs a
    login, which ai-memory does not create: the first time, devws offers
    an admin (root) account named after you and shows its temporary
    password once; the page then asks for your own. Later, add users with
    `ai-memory user add-human --username <name>` (`--role root` for an
    admin), or promote one with `ai-memory user patch <name> --role root`;
    the agents need no login;
  - **✚ Memory doctor**: checks its setup.
- The header's agent column ends with `mem ●`: green while the memory
  service answers, red when not (sessions are then not recorded; run
  **Memory doctor**).

```sh
devws memory search "how did we fix the login redirect?"
devws memory recent
devws memory wire        # wire every agent account again
```
