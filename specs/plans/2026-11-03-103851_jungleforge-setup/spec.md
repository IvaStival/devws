# Spec: set up jungleforge for users who don't have it

- **Date:** 03-11-2026
- **Branch:** feat/jungleforge-setup (off feat/memory-web-account)
- **Status:** Executed

## Context
devws's App tab (Run this branch, Switch, Stop, Services, Migrate, Shell, Logs)
drives jungleforge, the Docker control plane, through `config/tmuxinator/jungleforge.sh`.
Today it assumes a checkout at `~/Projects/jungleforge` with a hand-made `.env`
registry; without one every action dies with "jungleforge not found", and a repo
that isn't registered gets "Not a jungleforge project: register it in .env".
The public generic version is `https://github.com/IvaStival/jungleForge` (the
same repo the local checkout tracks; it already has the non-interactive
`start` / `stop` / `deps` / `services-start` targets devws uses, and `start`
builds the shared base image itself). Goal: a new user gets jungleforge from
the installer or the menu, and can register a repo from the menu.

Decisions (03-11-2026): clone into `~/.devws/jungleforge`; set up = clone +
`.env` with generated passwords + Docker check (no services started); a menu
action registers a repo and scaffolds `deploy/` from a jungleforge template;
without jungleforge the App tab shows one **Set up jungleforge** item instead of
the jungleforge actions.

Branch `feat/jungleforge-setup` off `feat/memory-web-account`; spec/plan saved
under `specs/plans/<timestamp>_jungleforge-setup/` (spec-plan workflow).

## Requirements

1. devws finds jungleforge without configuration: an explicit `DEVWS_JUNGLEFORGE`, else `~/.devws/jungleforge`, else `~/Projects/jungleforge`.
2. `./install.sh` and the App tab offer to set it up: clone the public repo, write `.env` with generated passwords, check Docker.
3. Without jungleforge, the App tab shows **Set up jungleforge** instead of the run actions.
4. **Register in jungleforge** adds a repo's apps to the registry, scaffolding `deploy/` from a jungleforge template when missing.
5. Running an unregistered repo offers to register it.

## Constraints

- jungleforge's own files are only cloned and given a `.env`; its Makefile and templates are used as they are.
- An existing `.env`, registry line of another app, or `deploy/` is never overwritten.
- Tests stub git remotes, `docker` and `make`; no network.

## Success Criteria

- [ ] Folder resolution order tested.
- [ ] Setup clones from a local remote, writes `.env` without `change_me`, never overwrites one.
- [ ] Register adds then replaces the app's line; scaffold copies the template with the name and a free port.
- [ ] Installer offer: no → remembered; yes → setup; `--jungleforge` asks again.
- [ ] App tab items follow jungleforge's presence.
- [ ] `scripts/check.sh` passes.
