# Plan: set up jungleforge for users who don't have it

- **Date:** 03-11-2026
- **Branch:** feat/jungleforge-setup
- **Status:** Executed (not committed yet)

Notes from execution:
- `jungleforge.sh` honors `DEVWS_ENV_FILE` (as `memory.sh` does), so its tests
  never read this machine's `.env`.
- The tracked `config/tmuxinator/.env` now has `DEVWS_JUNGLEFORGE` commented
  out, so the default order applies; this machine resolves to
  `~/Projects/jungleforge` through the `DEVWS_PROJECTS` fallback.
- `read -i` (bash 4) is not used: the app name prompt shows its default in
  brackets, so bash 3.2 keeps working.
- The register question is printed before `read`: `read -p` stays silent
  when stdin is not a terminal.
- Setup comments out `.env.example`'s placeholder registry entries; a template
  without `APP_HTTP_PORT` (node) reports no port.
- Checked against the real public repo in a scratch `DEVWS_HOME`: clone,
  `.env` (mode 600, no `change_me`), six templates, scaffold + register, and
  `make -n start <app>` accepts the registered app.

Implements `spec.md` in this folder.

## 1.Where jungleforge is — `jungleforge.sh`
- `jungleforge_folder`: an explicit `DEVWS_JUNGLEFORGE` (environment, then
  `.env`) wins; otherwise the first existing checkout of
  `$DEVWS_HOME/jungleforge` (default `~/.devws/jungleforge`) and
  `$DEVWS_PROJECTS/jungleforge` (the old default, so existing setups keep
  working); otherwise `$DEVWS_HOME/jungleforge` (where setup clones).
- Tracked `config/tmuxinator/.env`: the `DEVWS_JUNGLEFORGE` line becomes a
  commented example explaining that order (this machine resolves to
  `~/Projects/jungleforge` through the fallback, unchanged).
- New subcommands: `available` (folder has `Makefile` and `.env`), `folder`,
  `setup`, `templates`, `scaffold`, `register` (below). The existing
  "not found" error names `devws` menu › App › Set up jungleforge and
  `./install.sh --jungleforge`.

## 2. Setup — `jungleforge.sh setup`
- `git clone` `${DEVWS_JUNGLEFORGE_REPO:-https://github.com/IvaStival/jungleForge.git}`
  into the folder (refuses a non-empty folder that isn't a jungleforge checkout).
- `.env` from `.env.example` when missing (never overwritten), each
  `change_me` replaced by its own `openssl rand -hex 16`; nothing registered.
- Docker check: `docker` missing → "install Docker Desktop"; `docker info`
  failing → "start Docker Desktop". Warnings only; setup still succeeds.
- Prints next steps: register a repo from the App tab; the first Run builds
  the base image (minutes).

## 3. Installer — `install.sh`
- `ensure_jungleforge` after `ensure_ai_memory`, same shape: available → only
  the Docker warning; otherwise `Set up jungleforge (run branches in Docker)?
  [y/N]` → `jungleforge.sh setup`; a no is remembered
  (`~/.devws-state/jungleforge-declined`); `--jungleforge` asks again (usage text).
- `delete.sh` leaves the clone alone (it is the user's data, like `~/.devws`).

## 4. Menu — `devws_menu.sh`, `workspace_actions.sh`
- `shown()` gains `jungleforge` (available) and `no-jungleforge`.
- App rows: run-here, switch-run, stop-run, services, migrate, shell, logs get
  `|jungleforge`; new `app|setup-jungleforge|⚒ Set up jungleforge||setup_jungleforge|no-jungleforge`
  and `app|register-jungleforge|＋ Register in jungleforge||register_in_jungleforge|jungleforge`.
  Database, Database folder and Job log stay unconditional.
- `setup_jungleforge`: runs `jungleforge.sh setup` in the popup, then pause.
- Run this branch / Switch on an unregistered repo offer to register it
  (`[Y/n]`) instead of only printing the error.

## 5. Register a repo — `register_in_jungleforge` (+ `jungleforge.sh register|scaffold|templates`)
1. **Registry folder** (what jungleforge's "main" runs): a devws GitHub clone
   (`.bare` common dir) → its default-branch worktree, ensured with
   `worktree.sh ensure <dir> <default>` (`github_repo.sh default-branch`), since
   task folders come and go (tidy never removes it: it has no PR); any other
   folder → the workspace folder.
2. **App folder**: pick "(repo root)" or a first-level subfolder (hidden,
   `node_modules`, `vendor` left out); folders with `deploy/` marked; loop
   "Register another app of this repo?" for monorepos (backend/ + web/).
3. **Name**: suggested kebab-case repo name (`PhiPlanner` → `phi-planner`),
   `-<subfolder>` added for a second app; editable (`read -e -i`).
4. **No `deploy/.docker-env` in the workspace's folder** → offer scaffold: pick a
   template from jungleforge's `templates/` (deploy → php, deploy-node,
   deploy-next, deploy-frankenphp, deploy-rust, deploy-external); copy it to
   `<app folder>/deploy`, `.docker-env` from its example with `IMAGE_NAME` and
   `PHP_CONTAINER`/`APP_CONTAINER` = the name and `APP_HTTP_PORT` = the next
   free port (highest among registered apps' `.docker-env` + 1, at least 8090).
   When git ignores `deploy/.docker-env` and the repo has `.shared/`, it moves
   there and is linked (`shared_files.sh link`), like the other ignored files.
5. `jungleforge.sh register <app> <folder>`: writes or replaces
   `<APP>_PATH=<folder>` in jungleforge's `.env` (reuse `path_variable`).
6. Ends with what is left to the user: commit `deploy/` (main runs it once
   merged; Run this branch works now), and point the app's own `.env` at the
   shared services (`DB_HOST=postgres`, `REDIS_HOST=redis`…).

## 6. Docs
- `docs/guide/app.md`: "Set up jungleforge" and "Register a repo" sections;
  `configuration.md`: `DEVWS_JUNGLEFORGE` default/order, `DEVWS_JUNGLEFORGE_REPO`;
  `commands.md` if a `devws` subcommand is added (none planned);
  README install: the new offer and `--jungleforge`. `test_guide.sh` enforces
  the new labels and variables.

## Files
- Modify: `config/tmuxinator/jungleforge.sh`, `workspace_actions.sh`,
  `devws_menu.sh`, `config/tmuxinator/.env`, `install.sh`, `README.md`,
  `docs/guide/{app,configuration}.md`
- Tests: `scripts/test_jungleforge.sh`, `test_install_lifecycle.sh`,
  `test_devws_menu.sh`
- Reuse: `jungleforge.sh` `path_variable` / `registry` / `expand_home`;
  `worktree.sh ensure`; `github_repo.sh default-branch`; `shared_files.sh link`;
  `picker.sh` via `pick`; `install.sh` `ensure_ai_memory` pattern.

## Verification
- `test_jungleforge.sh`: folder order (explicit › `~/.devws` › `~/Projects`);
  `setup` from a local bare "remote" (`DEVWS_JUNGLEFORGE_REPO`) clones, writes
  `.env` with no `change_me`, never overwrites one; `register` adds then
  replaces a line; `scaffold` copies a template and sets name and the next port;
  stub `docker` for the warnings.
- `test_install_lifecycle.sh`: no jungleforge + n → nothing, remembered; y →
  setup called; `--jungleforge` asks again.
- `test_devws_menu.sh`: App tab without jungleforge = Set up + Database items +
  Job log; with it = the run actions + Register.
- `scripts/check.sh` passes.
- Manual: `DEVWS_HOME=/tmp/x` fresh setup from the menu → clone at
  `/tmp/x/jungleforge`; register a GitHub workspace's repo with a scaffolded
  `deploy/`; Run this branch builds and starts it.
