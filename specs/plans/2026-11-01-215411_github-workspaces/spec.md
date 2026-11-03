# Spec: GitHub workspaces — GitHub as the source of truth

- **Date:** 01-11-2026
- **Branch:** feat/github-workspaces
- **Status:** Executed

## Context

Today every devws workspace points at a hand-managed local folder, switched by
hand between tasks. GitHub should be the single source of truth instead:
starting a workspace picks a GitHub repo, devws keeps its clone in a folder it
owns, and every task gets a fresh branch in a worktree of its own. Committing,
pushing and opening the PR happen from devws; closing the workspace may remove
the worktree (kept by default). Workspaces, including each agent's
conversation, come back after a reboot or a closed terminal.

Settled in a grilling session (plan: `~/.claude/plans/today-i-have-all-elegant-beaver.md`).

## Requirements

1. **Root folder:** `DEVWS_HOME` in `config/tmuxinator/.env`, default `~/.devws`.
2. **Layout:** `<DEVWS_HOME>/<owner>/<repo>/.bare` is a hidden bare repo (fetch
   refspec set, fetched on every start); worktrees sit beside it as
   `<owner>/<repo>/<repo>-<branch>` ("/" → "-"). No main checkout. The repo
   name stays in the folder name because tmux names the workspace after it.
3. **Entry:** `devws` with no folder opens the GitHub flow; `devws <folder>`
   keeps working as before. The menu's Workspace tab gains
   "New GitHub workspace". Nothing is migrated from `~/Projects`.
4. **Repo picker:** `gh repo list` for the user and every org they belong to,
   cached for an hour; repos already cloned first; a "Refresh" row and a
   "Clone URL / owner/repo…" row (owner/repo, https or ssh URL).
5. **Reopen:** a repo with worktrees asks for "+ New branch" or one of its
   worktrees (branch, clean/dirty, cached PR state).
6. **New branch:** off a freshly fetched `origin/<default branch>`, not tracking
   it. Name = a prefix (feat/fix/chore/refactor/docs) + a description turned
   into a slug, or a custom name typed in full.
7. **Commit:** a Git tab in the menu and `devws commit [--manual]`. By default
   it asks the workspace's agent to commit (`/commit` when the commit skill is
   there, else a plain request). A busy agent (working or waiting) falls back
   to the manual popup: changed files, all selected, Tab to deselect, and a
   conventional message suggested from the branch prefix. Never adds AI credits.
8. **Push:** Git tab and `devws push`: `git push -u origin <branch>`; with no PR
   yet, offers to create one with `gh pr create --fill` (draft optional) and
   opens it in the browser; an existing PR's link is shown.
9. **Sidebar:** each workspace's PR state (draft/open/merged/closed) next to
   its branch, read from a cache that a background `gh` refresh updates at
   most every five minutes.
10. **Close:** unchanged — the worktree folder goes only on `y`, dirty
    worktrees are always kept, branches are never deleted, `.bare` stays.
11. **Restore:** restored workspaces resume their agent's last conversation in
    the folder (`agent_runner.sh --resume`).

## Constraints

- Never force: no `--force` on push, worktree removal or branch deletion.
- The sidebar never waits on the network; all `gh` calls in it are background.
- Tests use local bare repos and a stubbed `gh` — no network.
- Local-folder workspaces and existing worktree workspaces keep working.
- No changes to the saved-state format.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/github_repo.sh` | Create — clone root, repo list cache, bare clone/fetch, default branch, worktrees, PR state cache |
| `config/tmuxinator/github_workspace.sh` | Create — the interactive repo → worktree flow; prints the folder |
| `config/tmuxinator/git_actions.sh` | Create — commit (agent / manual) and push + PR |
| `config/tmuxinator/worktree.sh` | Modify — folder rule for `.bare` repos, optional base ref, remove via the common dir |
| `config/tmuxinator/workspace_actions.sh` | Modify — GitHub workspace and Git actions |
| `config/tmuxinator/devws_menu.sh` | Modify — Git tab, New GitHub workspace |
| `config/tmuxinator/session_picker.sh` | Modify — PR state on the branch line |
| `config/tmuxinator/dev.yml` | Modify — `resume` setting for the agent pane |
| `config/tmuxinator/restore_state.sh` | Modify — start workspaces with `resume=1` |
| `config/tmuxinator/.env` | Modify — `DEVWS_HOME` |
| `shell/devws.zsh` | Modify — no-folder GitHub flow, `commit`, `push` |
| `Brewfile` | Modify — `gh` |
| `README.md` | Modify — GitHub workspaces, Git tab, resume on restore |
| `scripts/test_github_repo.sh`, `scripts/test_git_actions.sh` | Create — tests |
| `scripts/test_worktree.sh`, `scripts/test_devws_menu.sh`, `scripts/test_workspace_state.sh`, `scripts/check.sh` | Modify — tests |

## Success Criteria

- [ ] `github_repo.sh ensure` makes a bare repo with remote-tracking branches and `origin/HEAD`; a second call fetches.
- [ ] `worktree.sh` puts a `.bare` repo's worktree at `<owner>/<repo>/<repo>-<branch>`, off the base ref, without tracking it, and can remove it.
- [ ] Manual commit commits only the selected files with the typed message; push sets the upstream and offers a PR.
- [ ] The Git tab lists Commit, Commit manually, Push / PR, each with a defined function.
- [ ] Restore starts the agent with `--resume`.
- [ ] `scripts/check.sh` passes.
