# Plan: Run and switch branches in jungleforge from devws

- **Date:** 02-11-2026
- **Branch:** feat/jungleforge-runs (devws), feat/devws-targets (jungleforge)
- **Status:** Executed
- **Commits:** devws dba4825, jungleforge cafb6eb

Notes from execution:
- jungleforge's branch is based on `fix/assets-stage-vendor`, which the local
  setup runs from (it is many commits ahead of jungleforge's `main`).
- `make services-start` is always called before a start: `up -d` is a no-op
  for running services, simpler than checking them.
- Shell and Logs open through `new_panel.sh` (a terminal panel below) with
  the command typed in, so the panel stays a shell after Ctrl-C.
- The sidebar's PR and running refreshes share one background job, at most
  every 30 s; Run and Stop clear its throttle.

## Context

Implements `spec.md` in this folder. Order: jungleforge's public targets first
(the contract), then devws's pure pieces with tests (`.shared`, app discovery,
running detection), then the interactive layer (Run tab, CLI, close, sidebar).

## Step 1 — jungleforge public targets

**Files:** `~/Projects/jungleforge/Makefile`, its `README.md` / `CLAUDE.md`

**Actions:**
- `start <project>` / `stop <project>`: the bodies of `_up` / `_down`
  (guard, ensure-network, ensure-base), non-interactive.
- `services-start`: `$(SERVICES) up -d` after `ensure-network`.
- `deps <project>`: installs the project's packages in its running container,
  per stack — `composer install` (php, frankenphp), `npm install` (node, next);
  a no-op for rust/external.
- Add them to `.PHONY` and the project-word swallowing list; document in `help`.
- Commit on `feat/devws-targets` in the jungleforge repo.

**Verification:** `make -n start phi-planner PHI_PLANNER_PATH=/tmp/x` prints a
compose `up -d` with `/tmp/x`; `make help` lists the targets.

## Step 2 — `.shared` files

**Files:** `config/tmuxinator/shared_files.sh`, `scripts/test_shared_files.sh`

**Actions:**
- `candidates <owner/repo>`: checkouts in `DEVWS_PROJECTS` (default
  `~/Projects`) with the same origin (any URL form → `github_repo.sh parse`).
- `ignored <folder>`: git-ignored files, minus `vendor/`, `node_modules/`,
  `storage/framework/`, `bootstrap/cache/`, `.idea/`, `.DS_Store`, `*.log`,
  `dist/`, `build/`, `.next/`, `target/`.
- `seed <repo dir> <folder> <file>…`: copies the files into `<repo dir>/.shared`.
- `link <repo dir> <worktree>`: symlink each `.shared` file into the worktree,
  copy `*.sqlite`, `*.sqlite3`, `*.db`; skip existing paths.

**Verification:** `scripts/test_shared_files.sh` — ignored list filtering,
seed, link (symlink vs copy, never overwrite), candidates by origin.

## Step 3 — wire `.shared` into GitHub workspaces

**Files:** `config/tmuxinator/github_workspace.sh`

**Actions:** after the first clone (no `.shared` yet): menu of candidates +
"Type a path…" + "Skip" → fzf multi of `ignored` (all ticked) → `seed`.
After a new worktree is created: `link`.

**Verification:** extend `scripts/test_github_repo.sh` — a new worktree gets
the seeded files linked.

## Step 4 — `jungleforge.sh` (apps, running folder, run/stop)

**Files:** `config/tmuxinator/jungleforge.sh`, `scripts/test_jungleforge.sh`

**Actions:**
- `apps <worktree>`: registry `<NAME>_PATH` entries of `DEVWS_JUNGLEFORGE/.env`
  whose checkout has the same origin → `<project>\t<subfolder>\t<main folder>`.
- `targets <worktree>`: main + every worktree of the repo, as
  `<label>\t<folder>`.
- `running <project>`: container from the app's `deploy/.docker-env`
  (`PHP_CONTAINER` / `APP_CONTAINER`), then its mount sources via
  `docker inspect`; prints the source when the container is running.
- `run <folder> <project>…`: `services-start` when the services are down,
  `make start <project> <NAME>_PATH=<folder>/<subfolder>` per app, then
  `make deps` when the lock files' hash differs from
  `<git dir>/devws-deps-<project>` (written after a successful install).
- `stop <worktree> <project>…`, `migrate`, `logs`, `shell` → public targets.

**Verification:** `scripts/test_jungleforge.sh` with stub `make` and `docker`
(logging calls): app discovery by origin (incl. subfolders), the exact
`make start … <NAME>_PATH=…` lines, services started only when down, deps
only when lock files changed, `running` from mount sources.

## Step 5 — Run tab, CLI, close

**Files:** `devws_menu.sh`, `workspace_actions.sh`, `shell/devws.zsh`,
`scripts/test_devws_menu.sh`

**Actions:**
- Tab `run`: run-here, switch, stop, services, migrate, shell, logs; app
  picker (all ticked) when a repo has several; "Not a jungleforge project…"
  when it has none.
- `devws run [branch|main]`, `devws stop`.
- `close_workspace`: when one of the repo's apps runs from the closing
  folder, ask back-to-main (default) / stop / leave (not offered when the
  folder is being removed).

**Verification:** `scripts/test_devws_menu.sh` — Run tab items and functions;
run-here and switch call `jungleforge.sh` with the right folder; close asks.

## Step 6 — sidebar `▶ running`

**Files:** `config/tmuxinator/session_picker.sh`

**Actions:** background refresh (throttled like the PR state) writes the
running folders to a cache file; rows whose root (or a subfolder) matches get
`▶ running`.

**Verification:** manual — run a branch, the sidebar row shows it.

## Step 7 — config, docs, check

**Files:** `.env` (`DEVWS_JUNGLEFORGE`, `DEVWS_PROJECTS`), `README.md`,
`scripts/check.sh`

**Verification:** `scripts/check.sh` passes.

## Final Verification

1. `scripts/check.sh` passes.
2. Manual with Docker running: `devws` → PhiPlanner → new branch (`.shared`
   seeded from `~/Projects/PhiPlanner`) → Run tab → Run this branch (both apps
   ticked) → app answers on its port; Switch → main → containers back on
   `~/Projects/PhiPlanner`; sidebar marks the running workspace; Close offers
   back-to-main.
