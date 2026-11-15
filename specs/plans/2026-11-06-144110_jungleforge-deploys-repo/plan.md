# Plan: Deploy folders in a repo of their own

- **Date:** 06-11-2026
- **Branch:** fix/jungleforge-root-app (devws), feat/deploys-path (jungleforge)
- **Status:** Draft

## Context

Implements `spec.md` in this folder. Order: jungleforge's lookup first (the
contract devws relies on), then the build changes it needs, then devws, then
moving the real deploy folders into the company repo.

## Step 0 — Preconditions

- The user commits jungleforge's pending work on `feat/extra-port`.
- Confirm the company repo's name and org; the user creates it (private).

## Step 1 — jungleforge: `deploy-path.sh`

**Files:** `scripts/deploy-path.sh`, `scripts/test_deploy_path.sh` (new)

**Actions:**
- Branch `feat/deploys-path` from `feat/extra-port`.
- Test first: `$DEPLOYS_PATH/<app>` exists → printed; missing or
  `DEPLOYS_PATH` unset → `<app folder>/deploy`; `~` expanded; reads
  `DEPLOYS_PATH` from the environment, else jungleforge's `.env`.
- Write the script to pass.

**Verification:** `bash scripts/test_deploy_path.sh` passes.

## Step 2 — jungleforge: Makefile and pick.sh use it

**Files:** `Makefile`, `scripts/pick.sh`

**Actions:**
- `DEPLOY := $(shell scripts/deploy-path.sh $(PROJECT) $(PROJECT_PATH))`.
- Export `DEPLOY_PATH=$(DEPLOY)` to every `$(COMPOSE)` call.
- `pick.sh:76` → `docker_env="$(scripts/deploy-path.sh "$slug" "$path")/.docker-env"`.

**Verification:** `make -n start ekko EKKO_PATH=/tmp/x DEPLOYS_PATH=/tmp/d`
uses `/tmp/d/ekko/.docker-env` when it exists, `/tmp/x/deploy/.docker-env`
when not.

## Step 3 — jungleforge: compose files and Dockerfiles

**Files:** `docker-compose.{dev,prod,frankenphp.*,next.*,node.*,rust.dev}.yml`, `build/*.Dockerfile`

**Actions:**
- Replace `${PROJECT_PATH}/deploy` with `${DEPLOY_PATH}` (dev nginx mount,
  frankenphp Caddyfile mount, next prod `env_file`).
- Add `additional_contexts: { deploy: ${DEPLOY_PATH} }` to every app `build:`.
- Dockerfiles: `COPY deploy/X` → `COPY --from=deploy X`;
  `--mount=type=bind,source=deploy,…` → `--mount=type=bind,from=deploy,…`.
- README / CLAUDE.md / `.env.example`: document `DEPLOYS_PATH`.

**Verification:** `docker compose … config` resolves for each stack; a real
`make start ekko` with ekko's deploy folder moved to `/tmp/d/ekko` builds and
serves on its port; an example app with in-repo `deploy/` (php and node)
still builds without `DEPLOYS_PATH`.

## Step 4 — devws: read and scaffold through the lookup

**Files:** `config/tmuxinator/jungleforge.sh`, `scripts/test_jungleforge.sh`

**Actions:**
- Tests first: `container_of` / `running` and `next_port` read the
  `.docker-env` in `DEPLOYS_PATH`; `scaffold` writes to `$DEPLOYS_PATH/<app>`
  when set, refuses an existing one, else `<app folder>/deploy`.
- New `deploy_folder <app> <folder>` calling jungleforge's `deploy-path.sh`;
  `container_of`, `next_port`, `scaffold` use it. `container_of` takes the
  app name too (callers already have it).

**Verification:** `bash scripts/test_jungleforge.sh` passes.

## Step 5 — devws: setup clones the deploys repo

**Files:** `config/tmuxinator/jungleforge.sh`, `.env` / `.env.example`, `scripts/test_jungleforge.sh`, `README.md`

**Actions:**
- Test first: with `DEVWS_JUNGLEFORGE_DEPLOYS_REPO` set, `setup` clones it to
  `$DEVWS_HOME/jungleforge-deploys` (pulls if present) and adds
  `DEPLOYS_PATH=` to jungleforge's `.env` only when missing; unset → nothing.
- Implement; document in README.

**Verification:** tests pass; `scripts/check.sh` passes.

## Step 6 — Move the real deploy folders

**Actions:**
- Clone the company repo; copy `~/Projects/ekko/deploy` → `ekko/`,
  `~/Projects/pusherService/deploy` → `pusher-service/`; check `.docker-env`
  for secrets before committing.
- Scaffold `orbit-back-dev/` and `satis/` (devws menu › App) into it.
- Set `DEVWS_JUNGLEFORGE_DEPLOYS_REPO`, run setup; `make start` each app.
- Only after each runs: delete the untracked `deploy/` from the old checkouts.

**Verification:** every registered app starts; a fresh orbit-back branch
folder runs from devws with no deploy folder in it.

## Step 7 — Commit

- jungleforge: commit on `feat/deploys-path`.
- devws: commit on the current branch.
- Company repo: commit the deploy folders.
