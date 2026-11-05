# Spec: Change jungleforge settings from the menu

- **Date:** 05-11-2026
- **Branch:** feat/agentic
- **Status:** Executed

## Context

Changing an app's port, or another jungleforge setting, meant finding and
hand-editing the right file. The devws menu's App tab gets a Settings item.

## Requirements

1. Settings lists the repo's apps (their `deploy/.docker-env` in the
   workspace's folder, the one a branch runs with) and the shared services
   (jungleforge's `.env`).
2. A setting is picked from a list showing its value and comment, and changed
   by typing the new value; the line keeps its comment and the file its
   permissions.
3. A port must be a number from 1 to 65535 not used by another registered app
   or shared service; the error names who has it.
4. The registry (`*_PATH`) is not listed (Register handles it); password and
   secret values are masked.
5. After a change, an app running from this workspace's folder is offered a
   restart (recreated, so a new port applies); a shared change offers
   restarting the services.

## Constraints

- jungleforge's own files and targets are not changed; only its public
  targets are used.
- No new settings are added; only active lines are changed.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/jungleforge.sh` | Modify — `settings` and `set` |
| `config/tmuxinator/workspace_actions.sh` | Modify — `change_app_setting` |
| `config/tmuxinator/devws_menu.sh` | Modify — the App tab's Settings |
| `scripts/test_jungleforge.sh`, `scripts/test_devws_menu.sh` | Modify — tests |
| `docs/guide/app.md` | Modify — Settings |

## Success Criteria

- [ ] `scripts/test_jungleforge.sh` and `scripts/test_devws_menu.sh` cover
      listing, setting, refusing a taken port and offering a restart.
- [ ] `scripts/check.sh` passes.
