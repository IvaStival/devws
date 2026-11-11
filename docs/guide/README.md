# devws guide

devws turns each project into a workspace that survives restarts: an
editor, a coding agent (Claude Code or Codex) and a terminal side by side,
one tmux session per branch, with the actions you need around them in one
menu.

```
 [project] 1:workspace                                   conv 1.9M · ctx 115k   19:25
 clock │ CPU MEM DSK │ agents' limits │ network │ weather          ← the header
┌───────────┬──────────────────────────────────────────┐
│ ☰ Menu    │                 editor                   │
│           │                                          │
│ my-app    ├────────────────────┬─────────────────────┤
│ feat/login│ ▾ Agent · claude   │ ▾ Terminal          │
│ claude ⠋  │                    │                     │
└───────────┴────────────────────┴─────────────────────┘
  sidebar      the agent              a shell
```

- **The sidebar** lists the open workspaces: branch, changes, PR, agent
  state. Click one to go there.
- **The header** shows the time, the machine, the agents' plan limits, the
  network and weather, and the background jobs while they run.
- **The menu**, `C-b Space` (Ctrl+B, then Space) or **☰ Menu**, has every
  action in tabs. Typing searches all of them; each shows its key.

## The menu

| Tab | What is there |
| --- | ------------- |
| Workspace | Open, start, and close workspaces, open an agent team, or detach |
| Layout | Layouts, minimize, the header, panels, the editor |
| Agent | Switch, restart, and follow agents, manage accounts and team roles, and access memory |
| Team | Open team views or manage the leader in a team workspace |
| Git | Commit, push, open a PR, and tidy merged branches |
| JungleForge | Set up JungleForge, register apps, run branches, and manage services |
| Tools | Open the database or job log and list network devices |
| Help | This guide |

`←` `→` change tabs, `Enter` runs, `Esc` closes. Typing searches every
tab: `push` finds **Push / PR** wherever you are. The menu reopens on the
tab you used last. Every question devws asks from a list looks the same:
`Enter` chooses, `Tab` unticks in a ticked list, `Esc` goes back a step.

The menu and those questions are drawn by `devws-ui`, devws's own Rust
program (in `devws-ui/`, built by `./install.sh` with the agent team's
program). It keeps the terminal's cursor hidden and draws its own, so a
popup no longer flickers while agents work. Without it (no Rust), fzf asks
instead, as before.

## The first workspace

```sh
devws                       # the start screen: a recent folder, or a new
                            # branch of a GitHub repo
devws ~/Projects/my-app     # or any folder
```

A new branch gets its own folder under `~/.devws`, so each task is
separate and several agents can work at once. When the agent is done:
`C-b g` commits, `C-b G` pushes and opens the pull request, and closing the
workspace offers to remove the folder.

## Pages

The menu's **Help** tab opens each one, as does `devws help <page>`.

| Page | What it covers |
| ---- | -------------- |
| [Workspaces](workspaces.md) | The start screen, GitHub branches, worktrees, closing, detaching, tidy, restore, pausing |
| [Layout](layout.md) | Layouts, panels, minimize, the header, the editor, following the agent |
| [Agents](agents.md) | Agents and accounts, status, switch and restart, ai-memory |
| [Agent teams](agentic.md) | A leader that opens and drives other agents (`devagentics`) |
| [Git](git.md) | Commit, push and pull requests |
| [JungleForge, jobs and database](app.md) | Running branches in JungleForge, the database pane, background jobs |
| [Shortcuts](shortcuts.md) | Every key |
| [Commands](commands.md) | Every `devws` command |
| [Configuration](configuration.md) | Settings, where devws keeps things, where the code is |

When something goes wrong, devws says so where you are looking: a failed job
stays red in the header with its log a key away (`C-b j`), and a background
error shows as a note there. After an update, `devws reload` applies it to
the open workspaces.
