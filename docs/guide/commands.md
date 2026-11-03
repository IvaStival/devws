# Commands

`devws` is a command on your PATH (`~/.local/bin/devws`, a link to the
checkout's `bin/devws`), so every pane runs the current version. Each run
first links into `~/.tmuxinator` any script an update added. Most commands
act on the workspace you run them in, or on the git repo of the current
folder.

## Workspaces

| Command | What it does |
| ------- | ------------ |
| `devws` | The start screen: reopen a recent folder, start a new branch from GitHub, or clone another repo ([Workspaces](workspaces.md)) |
| `devws <folder> [agent] [editor]` | A workspace on a folder (`devws .` for this one). Without an agent or editor, asks |
| `devws worktree <branch> [agent] [editor]` | A workspace on a git worktree of this repo's branch, in `../<repo>-<branch>`; a new branch off HEAD when it does not exist |
| `devws tidy` | Remove the branch folders whose PR is merged or closed, clean and not open (asks, all ticked) |
| `devws detach` | Leave devws; every workspace keeps running. `devws` brings you back |
| `devws menu [open\|close\|toggle] [workspace]` | Show or hide the sidebar, here or in another workspace |

## For programs: agent commands

[devAgentics](https://github.com/IvaStival/devAgentics) runs teams of agents
on top of devws through these commands, so they are a stable interface: their
output and exit codes change only with a note here. A pane is named by its
tmux id (`%12`); a state is `working`, `waiting` (needs you), `done` or `idle`.

| Command | What it does |
| ------- | ------------ |
| `devws open [--no-attach] [--task <id>] [--agent-args <file>] <folder> [agent] [editor]` | A workspace on a folder, as `devws <folder>`, for a program. `--no-attach` starts it detached, never asks (the defaults apply) and prints the session's name. `--task` lists it under that task in the sidebar and keeps it out of the start screen's recent folders. `--agent-args` adds arguments for the agent's program, after devws's own |
| `devws worktree ensure <repo or folder> <branch>` | Prints the folder of the branch's worktree, and opens nothing. A GitHub repo (`owner/repo`, a URL) is cloned when needed and the branch starts from its fresh default branch; a folder gets its worktree beside it |
| `devws agent spawn <session> <agent> [--beside <pane>] [--position right\|left\|below\|above] [--agent-args <file>]` | Adds an agent panel to the workspace ([New panel](layout.md#new-panels)) and prints its pane id. The focus stays where it was |
| `devws agent run <pane> <agent> [--agent-args <file>]` | Starts the agent in an agent pane, in place of the one running there. The pane keeps its place and role; a main agent's name is updated |
| `devws agent state <pane>` | Prints the pane's state; `gone` when the pane no longer exists |
| `devws agent send [--force] <pane> <text>` | Types the text and Enter into the pane when its agent is `done` or `idle` (`-` reads the text from stdin). Text with line breaks is pasted, so it stays one message. `--force` sends whatever the state is |
| `devws agent mark <pane> waiting\|done` | Sets the agent's state as its own hooks would: the sidebar shows it and a notification goes out |
| `devws agent stop <pane>` | Asks the agent to exit (`Ctrl-C` twice), closes the pane if it did not, and updates the workspace's state |
| `devws agent limits <agent>` | The share of that account's plan limits used, `<5-hour> <week>` in per cent (`42 63`), a `-` for what is not known |

Arguments files (`--agent-args`) hold the arguments NUL-separated, so one can
be a prompt with line breaks. A team agent always starts a new conversation.

Exit codes: `0` done, `1` failed (the message says why), `2` wrong use,
`3` `send` refused because the agent is busy (`busy (working)`), `4` the pane
is gone.

A task's workspaces are saved with their task. After a restart the task comes
back, but the panels a program started (`@devws_team`) are left to it: only
your own agents and extra panels are restarted ([Workspaces](workspaces.md#after-a-restart)).

## Restore after a restart

| Command | What it does |
| ------- | ------------ |
| `devws restore [--no-attach]` | Reopen the saved workspaces (and go to the first one) |
| `devws save` | Save the open workspaces now (devws does it on every change) |
| `devws fresh` | Forget the saved workspaces |
| `devws reload` | After an update: reload tmux.conf and restart the sidebars, keeping agents and editors running |

## Git

| Command | What it does |
| ------- | ------------ |
| `devws commit` | The workspace's agent commits (with its `/commit` skill when it has one) |
| `devws commit --manual` | Tick the files, type the message |
| `devws push` | Asks about the pull request, then pushes as a background job and opens the PR |

## jungleforge

| Command | What it does |
| ------- | ------------ |
| `devws run` | Run this branch's apps in jungleforge, as a background job |
| `devws run <branch\|main>` | Run another branch folder of the repo, or `main` |
| `devws stop` | Stop the repo's apps |

## Background jobs

| Command | What it does |
| ------- | ------------ |
| `devws job "<title>" [--lock <name>]... -- <command>` | Run any command as a background job: the header shows it, the log panel opens, a notification at the end. Jobs with the same lock run one after another |
| `devws note "<text>" [--error]` | Show a message in the header's jobs line |
| `devws jobs` | The recent jobs: id, state, title |
| `devws jobs log [id]` | Page a job's log (no id: the latest, a failure first) |

## Agent memory (with ai-memory)

| Command | What it does |
| ------- | ------------ |
| `devws memory search "<question>"` | Ask this workspace's agent to search its memory |
| `devws memory recent` | What was done recently in this repo |
| `devws memory web` | Open ai-memory's web page (the first time, offers an admin login) |
| `devws memory doctor` | Check ai-memory's setup |
| `devws memory wire` | Wire every agent account to ai-memory again |

## Reading

| Command | What it does |
| ------- | ------------ |
| `devws help [page]` | Open this guide: `shortcuts`, `workspaces`, `layout`, `agents`, `git`, `app`, `commands`, `configuration` (no page: the start) |
| `devws md [file.md]` | Read a Markdown file with Glow (no file: choose one below this folder). Mermaid diagrams open in the browser |

`~/.tmuxinator/editor_open.sh <path>[:<line>]` opens a file in the
workspace's Neovim, from the agent or the terminal.
