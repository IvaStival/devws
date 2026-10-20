# Spec: devws workspace power features

- **Date:** 16-10-2026
- **Branch:** one per feature, off `main` (see Delivery)
- **Status:** Specified

## Context

devws now runs several agents across several accounts in several workspaces. Things that were
easy with one agent in one workspace are now harder:

- The ghost shows that *some* agent needs you, but not which one, and you only notice if you
  are looking at the header.
- The header shows each account's 5h / week usage, but nothing happens when one is about to
  run out, even though another account could take over.
- Two agents on the same repo edit the same checkout.
- Follow mode shows one edit at a time. There is no way to review everything an agent changed.
- Running a project (dev server, log tail) means typing it into a terminal pane by hand, and it
  is lost on restore.
- Every workspace has the same panes. A project that needs a log tail instead of an agent has
  no way to say so.

This spec covers six features to fix that. Each one ships on its own.

## Requirements

### F1 — Jump to the agent that needs you

1. `C-b a` switches the client to the workspace whose agent is `waiting` and focuses its agent
   pane. With several, the one that has waited longest goes first; pressing again cycles on.
   With none waiting, it goes to one that is `done`; with none, it shows "No agent needs you".
2. The same action is in the menu (Agent tab): **! Go to waiting agent** (`C-b a`).
3. When an agent becomes `waiting` or `done` and its workspace is not the one on screen, devws
   sends a macOS notification ("devws-api: claude needs you"). `DEVWS_NOTIFY=0` in `.env` turns
   it off; `DEVWS_NOTIFY=waiting` notifies only for `waiting`.
4. The state records when it was entered (`@devws_agent_state_since`), so "longest" is
   well defined.

### F2 — Limit warnings and account switch

1. When an agent's 5h or week usage reaches `DEVWS_LIMIT_WARN` (default 90%), its header row
   turns to the stress color and devws shows a tmux message once per crossing:
   `claude: 5h at 92% — C-b Space › Agent › Switch account`.
2. A new menu action **⇄ Switch account** (Agent tab) lists the registered agents of the same
   kind as the workspace's agent, each with its 5h / week usage, sorted by most room left.
   Choosing one restarts the workspace's agent on that account, as Switch agent does.
3. The warning says that switching starts a new conversation (conversations belong to an
   account's folder).
4. No switch ever happens without the user choosing it.

### F3 — Worktree workspaces

1. A menu action **⑂ New worktree workspace** (Workspace tab), and `devws worktree <branch>
   [agent] [editor] [layout]` from the shell, in a git repo.
2. It offers the existing local branches (fzf) or takes a new name, which branches off the
   current workspace's HEAD.
3. The checkout goes next to the repo: `../<repo>-<branch>`, with `/` in the branch turned
   into `-`. An existing worktree for that branch is reused, not created again.
4. The new workspace opens on that folder like any other, and save/restore keeps it.
5. Nothing is ever deleted without asking. **× Close workspace** on a worktree workspace asks,
   before closing it, whether to remove the worktree folder too, with **Keep** as the default
   answer (anything but `y` keeps it). The question names the folder and the branch. A
   workspace is a worktree one when its folder is a linked worktree (not the repo's main
   folder), so restored workspaces need no extra state.
6. A worktree with uncommitted changes or untracked files is never removed: the question is not
   asked; devws says why it was kept and lists what is uncommitted.
7. The branch itself is never deleted, nor any commit on it. Removing a worktree uses
   `git worktree remove` without `--force`, so git refuses anything unsafe as well.

### F4 — Review the agent's changes

1. diffview.nvim is added to the LazyVim config.
2. A menu action **± Review changes** (Agent and Editor tabs) and `C-b G` open diffview in the
   workspace's editor pane, with two choices:
   - **Since the agent's last prompt**: the changes made during the agent's current or last
     turn.
   - **All uncommitted**: everything against HEAD.
3. For "since the last prompt", the state hook snapshots the working tree at
   `UserPromptSubmit` into a commit object that no branch points to (temporary index,
   `git write-tree`, `git commit-tree`), in the background. It records it in
   `@devws_turn_base`. It never touches the real index, the stash or any ref.
4. Untracked files are part of the snapshot, as are gitignored files that are not ignored by
   git's own rules (that is: the same set `git add -A` would add).
5. Only for editors that devws can reach (Neovim). With another editor, the action says so.
6. Not in a git repo: the action says so.

### F5 — Run panes and the dev server

1. A **run pane** is a pane that runs a command in the project root, with the role `run` and
   the command in `@devws_run_command`. It shows its own header (`▾ ▶ <command>`), like the
   database pane, and minimizes the same way.
2. A menu action **▶ Server** (Workspace tab) and `C-b S` open the project's server command in
   a run pane, or focus it when open. The first time, it asks for the command (with a suggestion
   from the project: `composer run dev` / `php artisan serve`, `npm run dev`, `make run`). The
   command is remembered per project, like the database folder.
3. **▶ Server command** changes the remembered command.
4. When the command exits, the pane stays open with its exit status and `Enter` to restart.
5. tmux's own line shows the workspace's run panes: `▶ server :8000` while running (the port
   from the process's listening sockets, when it has one), `■ server exit 1` when stopped.
6. Run panes are saved and restored with their command.

### F6 — Custom layouts

1. A layout says what goes in the two slots below (or beside) the editor. The default layout is
   `agent` and `terminal`. Each slot holds `agent`, `terminal` or `run:<command>`.
2. Layouts are named presets in `config/tmuxinator/.env`:
   ```sh
   DEVWS_LAYOUTS="default logs"
   DEVWS_LAYOUT_logs="run:tail -F storage/logs/laravel.log | terminal"
   ```
   `default` is built in and needs no line. A layout with no `agent` slot starts no agent.
3. The menu (Workspace tab) has **＋ New layout**: it asks for a name, then for each slot
   offers agent / terminal / command (typed). It also has **− Remove layout**, which shows the
   layout's slots and asks before removing it (default: keep). Both edit `.env` the way
   Register agent does.
4. **New workspace** asks for a layout after the editor, only when there is more than
   `default`. `devws <path> [agent] [editor] [layout]` takes it from the shell.
5. Toggle layout, minimize, save/restore and the header work the same with any layout. A
   run slot is a run pane (F5).
6. The workspace remembers its layout (`@devws_layout`) and save/restore keeps it.

## Constraints

- Plain bash and tmux like the rest of devws; the only new dependency is diffview.nvim (F4).
  macOS notifications use `osascript`, already on every Mac.
- No behavior change for anyone who sets nothing: the default layout, the default agents and
  the current menus stay as they are, apart from the new entries.
- Hooks must stay fast and silent: anything slow (F4's snapshot, F1's notification) runs in
  the background, and the state hook still always exits 0.
- devws asks before deleting anything (a worktree, a layout, a run pane's command), with "keep"
  as the default answer. It never deletes a branch or uncommitted work.
- No automatic account switching.
- Out of scope: switching an open workspace to another layout (only new workspaces take one),
  layouts with a different pane shape (still editor + two slots), cost history, and fixing the
  existing `test_workspace_minimize.sh` failure (a separate fix).

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/agent_status.sh` | Modify — record `@devws_agent_state_since`, send notifications (F1) |
| `config/tmuxinator/jump_agent.sh` | Create — find and focus the waiting agent (F1) |
| `config/tmuxinator/agent_state.sh` | Modify — turn snapshot on `UserPromptSubmit` (F4) |
| `config/tmuxinator/turn_snapshot.sh` | Create — snapshot the working tree to a commit object (F4) |
| `config/tmuxinator/header_agents.sh` | Modify — stress color at the warning threshold (F2) |
| `config/tmuxinator/header_ticker.sh` | Modify — warn once per crossing (F2); run-pane status (F5) |
| `config/tmuxinator/workspace_actions.sh` | Modify — switch account, worktree, review, server, layouts (F2–F6) |
| `config/tmuxinator/devws_menu.sh` | Modify — new menu entries (F1–F6) |
| `config/tmuxinator/worktree.sh` | Create — create/reuse/remove worktrees (F3) |
| `config/tmuxinator/run_pane.sh` | Create — run a command, keep the pane on exit, restart on Enter (F5) |
| `config/tmuxinator/open_run_pane.sh` | Create — open or focus a run pane in its place (F5) |
| `config/tmuxinator/layouts.sh` | Create — read layouts from `.env`, like `agents.sh` (F6) |
| `config/tmuxinator/register_layout.sh` | Create — add/remove layouts in `.env`, like `register_agent.sh` (F6) |
| `config/tmuxinator/slot_runner.sh` | Create — start what a layout puts in a slot (F6) |
| `config/tmuxinator/dev.yml` | Modify — slots run `slot_runner.sh`, record `@devws_layout` (F6) |
| `config/tmuxinator/workspace_panes.sh`, `toggle_layout.sh`, `minimize_panes.sh` | Modify — treat run panes as slot panes (F5, F6) |
| `config/tmuxinator/save_state.sh`, `restore_state.sh` | Modify — keep layout, run commands, worktree roots (F3, F5, F6) |
| `config/tmuxinator/.env` | Modify — document `DEVWS_NOTIFY`, `DEVWS_LIMIT_WARN`, `DEVWS_LAYOUTS` |
| `config/tmux/tmux.conf` | Modify — `C-b a`, `C-b G`, `C-b S`; run-pane status; run-pane header |
| `config/nvim/lua/plugins/diffview.lua` | Create — diffview.nvim (F4) |
| `config/nvim/lua/devws/review.lua` | Create — open diffview against a base (F4) |
| `shell/devws.zsh` | Modify — `devws worktree`, layout argument (F3, F6) |
| `scripts/test_*.sh`, `scripts/check.sh` | Create/modify — tests for each feature |
| `README.md` | Modify — document each feature |

## Delivery

One branch and one PR per feature, in this order, because later ones build on earlier ones:

1. `feat/jump-to-agent` (F1)
2. `feat/limit-warnings` (F2)
3. `feat/custom-layouts` (F6): adds run panes in slots and the slot runner
4. `feat/server-pane` (F5): builds on F6's run panes
5. `feat/review-changes` (F4)
6. `feat/worktree-workspaces` (F3)

`feat/agent-registry` is merged first; F2 depends on it.

## Success Criteria

- [ ] F1: with two workspaces, one agent waiting, `C-b a` from the other lands on the waiting
      agent's pane; a notification appeared when it started waiting; tests cover order and
      cycling.
- [ ] F2: crossing 90% turns the row red and shows the message once; Switch account lists the
      same-kind agents by room left and restarts on the chosen one.
- [ ] F3: a worktree workspace opens in `../<repo>-<branch>` and survives save/restore. Closing
      it asks before removing a clean worktree, keeps it on Enter/Esc, never asks for a dirty one,
      and never deletes the branch.
- [ ] F4: after a prompt, Review changes › since last prompt shows exactly that turn's edits in
      diffview, untracked files included; `git status`, the index and refs are unchanged.
- [ ] F5: `C-b S` starts the server, the status line shows its port, it restarts on Enter after
      exiting, and it comes back after a restore.
- [ ] F6: a `logs` layout made from the menu opens a workspace with a log tail instead of the
      agent; toggle, minimize and restore work with it.
- [ ] `scripts/check.sh` passes, apart from the existing minimize test failure.
- [ ] README documents each feature.
