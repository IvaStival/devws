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
| `devws memory web` | Open ai-memory's web page |
| `devws memory doctor` | Check ai-memory's setup |
| `devws memory wire` | Wire every agent account to ai-memory again |

## Reading

| Command | What it does |
| ------- | ------------ |
| `devws help [page]` | Open this guide: `shortcuts`, `workspaces`, `layout`, `agents`, `git`, `app`, `commands`, `configuration` (no page: the start) |
| `devws md [file.md]` | Read a Markdown file with Glow (no file: choose one below this folder). Mermaid diagrams open in the browser |

`~/.tmuxinator/editor_open.sh <path>[:<line>]` opens a file in the
workspace's Neovim, from the agent or the terminal.
