# Spec: Run and switch branches in jungleforge from devws

- **Date:** 02-11-2026
- **Branch:** feat/jungleforge-runs (off feat/github-workspaces)
- **Status:** Executed

## Context

GitHub workspaces (d5b8e05) give every task its own worktree under
`~/.devws/<owner>/<repo>/<repo>-<branch>`. The PHP/Node apps are tested
locally through jungleforge (`~/Projects/jungleforge`), whose registry maps a
project name to one folder (`PHI_PLANNER_PATH=~/Projects/PhiPlanner/backend`)
and runs one container per project on shared MariaDB/Postgres/Redis/RabbitMQ.
A fresh worktree cannot run: it lacks the git-ignored files (`.env`,
`deploy/.docker-env`, SQLite DBs) and its packages, and jungleforge only knows
the `~/Projects` folder. Testing a branch should be one menu choice, and
changing which branch runs should be just as quick.

Decisions come from a grilling session on 02-11-2026.

## Requirements

### Shared untracked files
1. Each managed repo has `~/.devws/<owner>/<repo>/.shared/`, mirroring the
   repo's layout (`.shared/backend/.env`).
2. A new worktree gets every `.shared` file at the same relative path:
   **symlinked**, except SQLite databases (`*.sqlite`, `*.sqlite3`, `*.db`),
   which are **copied** so each branch has its own data. An existing file in
   the worktree is never replaced.
3. Seeding: the first time a repo is cloned, a menu offers folders to import
   from — the git checkouts in `DEVWS_PROJECTS` (`.env`, default
   `~/Projects`) whose `origin` is the same repo, first — plus "Type a
   path…" and "Skip". Each person's projects folder can differ. The chosen folder's
   git-ignored files are listed (vendor, node_modules, caches, build output and
   logs left out), all ticked; Tab unticks. They are copied into `.shared`.
4. Packages: none are copied; a new worktree starts without `vendor/` and
   `node_modules/`.

### jungleforge integration
5. Setting `DEVWS_JUNGLEFORGE` in `.env`, default `~/Projects/jungleforge`.
6. A repo's apps are the registry entries (`<NAME>_PATH` in jungleforge's
   `.env`) whose folder belongs to a checkout with the same git `origin`;
   each has a subfolder (`backend`, `web`, or the repo root). Its **main**
   target is that registry folder.
7. jungleforge gains public, non-interactive targets (one commit in the
   jungleforge repo): `make start <project>` and `make stop <project>` (what
   the picker's `_up` / `_down` do) and `make services-start` (the shared
   stack). devws calls only public targets: these plus `logs`, `sh` and
   `migrate`.
8. The running folder of an app is read from Docker: its container (the
   `PHP_CONTAINER`/`APP_CONTAINER` of its `deploy/.docker-env`, as jungleforge's
   picker does) and the source of its code mount.

### Run tab (devws menu) and CLI
9. A **Run** tab:
   - **▶ Run this branch** — runs the current workspace's worktree.
   - **⇄ Switch running branch** — lists the repo's worktrees and
     "main (<registry folder>)", the running one marked `▶`; choosing one
     runs it.
   - **■ Stop** — stops the repo's apps.
   - **⛃ Services** — starts the shared stack.
   - **⇡ Migrate** — `make migrate` for the running branch's apps.
   - **⌘ Shell** and **≡ Logs** — of an app of the repo.
10. Running a branch: asks which of the repo's apps to move (all ticked,
    Tab unticks; no question when there is one), starts the shared services
    if they are down, then runs `make start <project> <NAME>_PATH=<folder>`
    per app — jungleforge's `.env` is never edited.
11. After an app starts, `composer install` / `npm install` runs in its
    container only when `composer.lock` / `package-lock.json` differs from
    what that folder last installed (recorded per folder); a fresh worktree
    always installs.
12. Switching never migrates.
13. A repo with no registered apps: the Run items say
    "Not a jungleforge project: register it in <jungleforge>/.env".
14. CLI: `devws run [branch|main]`, `devws stop`.

### Workspace lifecycle
15. Closing a workspace whose worktree is running asks: switch the apps back
    to main (default) / stop them / leave them running. If the worktree folder
    is being removed, "leave them running" is not offered.
16. The sidebar shows `▶ running` on the workspace whose folder an app runs
    from, read in the background like the PR state.

## Constraints

- Never edit jungleforge's `.env`; never call its `_`-prefixed targets.
- Never overwrite a file in a worktree when linking `.shared`.
- The sidebar never waits on Docker.
- Tests stub `docker` and `make`; no containers are started.
- Only one branch per jungleforge project runs at a time (same container,
  port and database) — by design.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/shared_files.sh` | Create — seed `.shared`, link it into a worktree |
| `config/tmuxinator/jungleforge.sh` | Create — repo's apps, running folder, start/stop/services/migrate/install |
| `config/tmuxinator/github_workspace.sh` | Modify — seed on first clone, link `.shared` into new worktrees |
| `config/tmuxinator/devws_menu.sh`, `workspace_actions.sh` | Modify — Run tab; close asks about running apps |
| `config/tmuxinator/session_picker.sh` | Modify — `▶ running` |
| `config/tmuxinator/.env` | Modify — `DEVWS_JUNGLEFORGE`, `DEVWS_PROJECTS` |
| `shell/devws.zsh` | Modify — `devws run`, `devws stop` |
| `~/Projects/jungleforge/Makefile` (+ README) | Modify — `start`, `stop`, `services-start` |
| `scripts/test_shared_files.sh`, `scripts/test_jungleforge.sh` | Create — tests |
| `scripts/test_devws_menu.sh`, `scripts/check.sh`, `README.md` | Modify |

## Success Criteria

- [ ] A new worktree has `.shared` config symlinked and SQLite DBs copied; existing files untouched.
- [ ] Seeding lists only git-ignored config from the chosen folder and copies the ticked ones.
- [ ] A repo's apps and subfolders are found from jungleforge's registry by origin.
- [ ] Run calls `make start <project> <NAME>_PATH=<worktree>/<subfolder>` for the ticked apps, after `services-start` when services are down.
- [ ] Installs run only when lock files changed for that folder.
- [ ] The Switch list marks the running target; Close offers switching back to main.
- [ ] `scripts/check.sh` passes.
