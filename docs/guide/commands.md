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
| `devws agentic [--editor] [--leader <agent>] [<repo or folder> [<branch>]]` | A workspace for an agent team: the leader, a log and room for the agents it opens ([Agent teams](agentic.md)). Without arguments, the start screen. `--leader` names the registered agent that leads; without it, a terminal is asked |
| `devws worktree <branch> [agent] [editor]` | A workspace on a git worktree of this repo's branch, in `../<repo>-<branch>`; a new branch off HEAD when it does not exist |
| `devws tidy` | Remove the branch folders whose PR is merged or closed, clean and not open (asks, all ticked) |
| `devws detach` | Leave devws; every workspace keeps running. `devws` brings you back |
| `devws menu [open\|close\|toggle] [workspace]` | Show or hide the sidebar, here or in another workspace |

## For programs: agent commands

[`devagentics`](../../agentic/docs/usage.md) runs teams of agents
on top of devws through these commands, so they are a stable interface: their
output and exit codes change only with a note here. A pane is named by its
tmux id (`%12`); a state is `working`, `waiting` (needs you), `done` or `idle`.

| Command | What it does |
| ------- | ------------ |
| `devws open [--no-attach] [--task <id>] [--agent-args <file>] [--template <name>] [--set <key=value>] <folder> [agent] [editor]` | A workspace on a folder, as `devws <folder>`, for a program. `--no-attach` starts it detached, never asks (the defaults apply) and prints the session's name. `--task` lists it under that task in the sidebar and keeps it out of the start screen's recent folders. `--agent-args` adds arguments for the agent's program, after devws's own. `--template` starts another tmuxinator project (`agentic`), with `--set` settings for it |
| `devws worktree ensure <repo or folder> <branch>` | Prints the folder of the branch's worktree, and opens nothing. A GitHub repo (`owner/repo`, a URL) is cloned when needed and the branch starts from its fresh default branch; a folder gets its worktree beside it |
| `devws agent spawn <session> <agent> [--beside <pane>] [--position right\|left\|below\|above] [--agent-args <file>] [--label <text>] [--place team] [--group <name>] [--folder <dir>]` | Adds an agent panel to the workspace ([New panel](layout.md#new-panels)) and prints its pane id. The focus stays where it was. `--label` is what its header says in place of "Agent". `--group <name>` puts it in an [agent team](agentic.md)'s group: a workspace of its own beside this one (`dev-app-floor`), four agents to a group (the next in `<name> 2`). `--folder` starts it in that folder instead of the workspace's. `--place team` puts it in the team area: a grid right of the leader |
| `devws agent go <pane>` | Shows a group's agent: its workspace, focused on it |
| `devws agent peek <pane>` | Brings a group's agent under the team board for a look; again, or peeking another, sends it back to its group |
| `devws agent run <pane> <agent> [--agent-args <file>]` | Starts the agent in an agent pane, in place of the one running there. The pane keeps its place and role; a main agent's name is updated |
| `devws agent state <pane>` | Prints the pane's state; `gone` when the pane no longer exists |
| `devws agent send [--force] <pane> <text>` | Types the text and Enter into the pane when its agent is `done` or `idle` (`-` reads the text from stdin). Text with line breaks is pasted, so it stays one message. `--force` sends whatever the state is |
| `devws agent mark <pane> waiting\|done` | Sets the agent's state as its own hooks would: the sidebar shows it and a notification goes out |
| `devws agent stop <pane>` | Asks the agent to exit (`Ctrl-C` twice), closes the pane if it did not, and updates the workspace's state |
| `devws agent kind <agent>` | The program that agent runs, `claude` or `codex` (an account such as `claude_work` runs `claude`); exit 1 when it is not registered |
| `devws agent limits <agent>` | The share of that account's plan limits used, `<5-hour> <week>` in per cent (`42 63`), a `-` for what is not known |
| `devws agent usage <pane>` | What the pane's agent has used, `<input> <output> <cache> <US$> <unpriced>`: tokens, and the cost at API prices; `-` until the agent has said which conversation it is in. Exit 4 when the pane is gone |

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

## Network

Run `devws network devices` from a terminal or choose **Tools → List network devices**.
The command shows the default network interface and
its connected IPv4 subnet, then lists responding devices. Names and MAC
addresses appear when available. Sleeping or isolated devices, and those
that block discovery probes, may be absent.

Discovery uses Nmap ARP requests on macOS and Linux to find devices on the
local subnet, including devices whose web ports ignore connection probes.
It requests administrator access through `sudo` for Nmap. Enter your
system password when prompted.

Install Nmap with `brew install nmap` on macOS or your Linux package manager.
After discovery, the menu displays the results in a pager when `less` is
available. Press `q` to close it.

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
