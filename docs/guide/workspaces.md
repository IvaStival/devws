# Workspaces

A workspace is one project folder in its own tmux session: the sidebar, the
editor, the agent and a terminal. The menu's **Workspace** tab (`C-b Space`)
opens, starts and closes them; the sidebar lists the open ones; click one,
or move to it and press `Enter`, to go there.

## The start screen

**⎇ Open or start a workspace** in the menu, or `devws` with no folder:

```
devws >
  ⎇ Branch from GitHub…
  ⌂ Open a local folder…
  ↓ Clone another repo (URL / owner/repo)…
  PhiPlanner   feat/login-page   ● changes · PR open   2h ago
  PhiPlanner   fix/crash         ✓ clean               yesterday
```

- **A recent folder** (newest first, then branch folders never opened)
  starts at once with the agent and editor it had, or switches to it when it
  is already open.
- **⎇ Branch from GitHub…**
  1. **Repo:** the owner first (**★ My repos**, then each organization),
     then the repo. Cloned ones come first, marked `●`. The lists are kept
     an hour; **↻ Refresh the list** asks GitHub again. `Esc` goes back.
  2. **Branch:** one already on GitHub, newest first, freshly fetched: it
     gets a folder of its own, tracking GitHub's branch, or its folder
     reopens when it has one (marked `● folder`). Or **+ New branch…**: its
     kind (feat, fix, chore, refactor, docs) and a few words: `feat` +
     "Login page" makes `feat/login-page`. **Custom name…** takes any name.
     A new branch starts from GitHub's default branch. `Esc` on the kind
     goes back to the branches.
  3. **Agent and editor.**
- **⌂ Open a local folder…** lists the folders in `~/Projects`
  (`DEVWS_PROJECTS`), each git repo with its branch, or **✎ Type a path…**
  for any other (Tab completes). The folder opens as it is, no branch made;
  devws asks for the agent and editor. Same as `devws <folder>`.
- **↓ Clone another repo…** takes any repo by URL or `owner/repo`.

It needs [gh](https://cli.github.com) logged in (`gh auth login`); the
installer offers it. Without it, the repo list fails with "gh could not
list your repos (is it logged in? gh auth login)"; recent folders still
open.

### Where branch folders live

```
~/.devws/                          DEVWS_HOME
  acme/my-app/
    .bare/                         the clone (hidden, fetched each time)
    .shared/                       untracked files every branch gets
    my-app-feat-login-page/        a folder per branch (a git worktree)
    my-app-fix-crash/
```

There is no main checkout to keep up to date: every task gets its own
folder.

### Untracked files (`.shared`)

A branch folder only has what git tracks, so each repo keeps the files git
ignores but the app needs (`.env`, `deploy/.docker-env`, a SQLite database)
once in `.shared/`, laid out like the repo. Every branch folder gets a link
to each, so editing `.shared/backend/.env` reaches every branch. Databases
(`*.sqlite`, `*.sqlite3`, `*.db`) are copied instead, so each branch has its
own data. A file already in the folder is never replaced.

A repo's first workspace asks where to import them from: your checkout of
the same repo in `~/Projects` (`DEVWS_PROJECTS`), another folder, or Skip.
It then lists that folder's ignored files, all ticked (packages, caches and
builds left out).

## Other workspaces

- **+ New folder workspace**: a workspace on any folder you type (Tab
  completes). Same as `devws <folder>`.
- **⑂ New worktree workspace**: a second checkout of the current repo, so
  two agents never edit each other's files. Pick a branch (local, or a
  remote one, then tracked) or **+ New branch…** off the current HEAD. The
  folder goes beside the repo (`~/Projects/my-app-feat-login`); a branch
  that already has one opens it again. Same as `devws worktree <branch>`.
- **⌂ Open folder in…**: the workspace's folder in Finder, Zed or VS Code.

## Closing and leaving

- **× Close workspace** closes the workspace you choose. Closing the one you
  are in moves you to the one you used last; only the last one leaves
  devws.
  - **A branch or worktree folder** asks whether to remove the folder too
    and keeps it unless you type `y`. One with uncommitted changes or
    untracked files is always kept, and devws lists them. The branch and
    its commits are never deleted.
  - **A branch running in jungleforge** asks whether to switch its apps
    back to main (the default), stop them, or leave them running.
- **⏏ Detach (workspaces keep running)** (`C-b d`, `devws detach`) leaves
  devws with everything still running; `devws` brings you back.
- **⌫ Tidy up merged branches** (`devws tidy`) lists the branch folders
  under `~/.devws` whose PR is merged or closed, with no changes and no
  workspace open on them, all ticked (`Tab` unticks one), and removes them
  as a background job. The branches stay.

## The sidebar

Each open workspace shows its folder, branch, agent and editor:

- dark grey: the workspace you are in; lighter grey: the selected row;
- yellow `●`: uncommitted or untracked changes; green `✓`: clean;
- `· PR draft`, `· PR open`, `· PR merged`, `· PR closed`: its pull request
  (checked every few minutes);
- `· ▶ running`: jungleforge runs its branch;
- the agent's state: `⠋` working, `● needs you`, `✓ done` ([Agents](agents.md));
- `· paused`: frozen while unused (below).

**☰ Menu** at its top opens the devws menu. `devws menu toggle` (or
`open`/`close`) shows or hides it.

## After a restart

Open workspaces are saved as they change (`~/.devws-state/sessions.tsv`):
their folder, agent and editor, the layout, extra panels, the database pane
and the sidebar. The first `devws` that finds no tmux server reopens them,
each agent continuing its last conversation, then opens what you asked for.

```sh
devws restore              # reopen them by hand, and go to the first
devws restore --no-attach  # reopen them in the background
devws save                 # save now
devws fresh                # forget them
```

`DEVWS_AUTO_RESTORE=0` turns the automatic restore off. A folder that no
longer exists is skipped with a message; an agent or editor no longer
listed falls back to the default; programs you started in a terminal are
not restarted (the terminal comes back at a prompt in its folder).

## Paused workspaces

A workspace you have not looked at for 15 minutes (`DEVWS_SUSPEND_AFTER`),
whose agent is idle or done, is frozen: its agents, editor and sidebar stop
using the CPU, and its sidebar row says `· paused`. Switching to it wakes it
at once. A workspace whose agent works or needs you is never frozen, and
terminals keep running (they may serve or watch something). Memory stays in
use.
