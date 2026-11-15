# Spec: Deploy folders in a repo of their own

- **Date:** 06-11-2026
- **Branch:** fix/jungleforge-root-app (devws), off feat/extra-port (jungleforge)
- **Status:** Draft

## Context

jungleforge (`~/.devws/jungleforge`, `IvaStival/jungleForge`) reads each app's
container config from `<app folder>/deploy/` (`Makefile:39`
`DEPLOY := $(PROJECT_PATH)/deploy`). For company apps (ekko, pusherService,
orbit-back, satis…) that folder must not be committed to the company repos.
Left untracked, it lives only in the one checkout where it was made: every
devws branch folder of the same repo starts without it and cannot run.

The deploy folders move to one private repo for the company, one folder per
app, cloned next to jungleforge. jungleforge stays a generic tool with no
company content; its lookup falls back to `<app>/deploy` so apps that keep
their deploy folder in-repo work as today.

## Requirements

### Where an app's deploy folder is
1. jungleforge's `.env` may set `DEPLOYS_PATH` (a folder of deploy folders).
2. An app's deploy folder is `$DEPLOYS_PATH/<app>` when that folder exists,
   else `<app folder>/deploy`. `<app>` is the registry name in lowercase
   with dashes (`PUSHER_SERVICE_PATH` → `pusher-service`), as `make … <app>`
   already takes it.
3. One implementation of that rule, `scripts/deploy-path.sh <app> <app folder>`,
   used by the Makefile, `scripts/pick.sh` and devws.

### jungleforge reads the deploy folder from anywhere
4. The Makefile's `DEPLOY` comes from `deploy-path.sh`, and is passed to compose
   as `DEPLOY_PATH`.
5. Compose files never name `${PROJECT_PATH}/deploy`: bind mounts and
   `env_file` use `${DEPLOY_PATH}`.
6. Every app build gets a named build context `deploy: ${DEPLOY_PATH}`
   (`additional_contexts`); the Dockerfiles in `build/` read deploy files with
   `COPY --from=deploy …` and `RUN --mount=type=bind,from=deploy,…`. The build
   context stays the app folder (its code and `.dockerignore`).
7. An app with an in-repo `deploy/` and no `DEPLOYS_PATH` builds and runs
   exactly as before.

### devws
8. `.env` setting `DEVWS_JUNGLEFORGE_DEPLOYS_REPO` (the company repo's clone
   URL, empty by default). When set, `jungleforge.sh setup` clones it to
   `$DEVWS_HOME/jungleforge-deploys` (or pulls it if there) and writes
   `DEPLOYS_PATH` into jungleforge's `.env` when it has none.
9. `container_of`, `next_port` read `.docker-env` through `deploy-path.sh`.
10. `scaffold` (new deploy folder from a template) writes to
    `$DEPLOYS_PATH/<app>` when `DEPLOYS_PATH` is set, else `<app folder>/deploy`
    as today. It never overwrites an existing deploy folder.

### Company repo
11. Private repo `UniglovesBR/jungleforge-deploys` (name to confirm): one
    folder per app at its root (`ekko/`, `pusher-service/`, `orbit-back-dev/`,
    `satis/`), each with the files a template gives (`.docker-env`,
    `.docker-env.example`, `nginx/`, `php/`, `supervisor/`, entrypoints).
12. `.docker-env` holds no secrets (ports, container names, versions only), so
    it is committed. App secrets stay in the app's `.env` / `.shared`.

## Constraints

- jungleforge's uncommitted work on `feat/extra-port` is not touched: the
  changes go on a new branch from it after that work is committed by the user.
- jungleforge's `.env` is only written by `setup` and only to add
  `DEPLOYS_PATH` when missing — never to change an existing value.
- No company names, URLs or app configs in `IvaStival/jungleForge`.
- Tests stub `docker`, `make` and `git clone`; no containers are started.
- Needs Docker Compose ≥ 2.17 for `additional_contexts` in dev files (prod
  already uses it).

## Files Involved

| File | Action |
| ---- | ------ |
| `~/.devws/jungleforge/scripts/deploy-path.sh` | Create — the lookup rule |
| `~/.devws/jungleforge/Makefile` | Modify — `DEPLOY` from the script, pass `DEPLOY_PATH`; scaffold-related targets |
| `~/.devws/jungleforge/docker-compose.*.yml` | Modify — `${DEPLOY_PATH}`, `additional_contexts: deploy` |
| `~/.devws/jungleforge/build/*.Dockerfile` | Modify — `COPY --from=deploy`, `--mount … from=deploy` |
| `~/.devws/jungleforge/scripts/pick.sh` | Modify — use `deploy-path.sh` |
| `~/.devws/jungleforge/.env.example`, `README.md`, `CLAUDE.md` | Modify — document `DEPLOYS_PATH` |
| `config/tmuxinator/jungleforge.sh` | Modify — `container_of`, `next_port`, `scaffold`, `setup` |
| `config/tmuxinator/.env` (+ example) | Modify — `DEVWS_JUNGLEFORGE_DEPLOYS_REPO` |
| `scripts/test_jungleforge.sh` | Modify — tests for the above |
| `README.md` | Modify — the deploys repo |
| `UniglovesBR/jungleforge-deploys` | Create — ekko and pusher-service deploy folders moved in |

## Success Criteria

- [ ] `deploy-path.sh ekko ~/Projects/ekko` prints `$DEPLOYS_PATH/ekko` when it exists, else `~/Projects/ekko/deploy`.
- [ ] `make start ekko` builds and runs with ekko's `deploy/` deleted from its checkout.
- [ ] A devws branch folder of orbit-back runs with no deploy folder in it.
- [ ] An app with an in-repo `deploy/` and no `DEPLOYS_PATH` still builds (php and node stacks checked).
- [ ] `jungleforge.sh setup` clones the deploys repo and sets `DEPLOYS_PATH` once.
- [ ] `scaffold` writes into the deploys repo when `DEPLOYS_PATH` is set.
- [ ] `scripts/check.sh` passes.

## Out of scope

- Keeping each app on the host port it had under phpdocker — next task, set
  per app in its `.docker-env` once the deploy folders live in the repo.
- A per-stack default `.dockerignore` for apps without one.
