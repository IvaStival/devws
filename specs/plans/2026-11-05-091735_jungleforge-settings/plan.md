# Plan: Change jungleforge settings from the menu

- **Date:** 05-11-2026
- **Branch:** feat/agentic
- **Status:** Executed

## Context

See `spec.md`: a ⚙ Settings item in the App tab for an app's
`deploy/.docker-env` and jungleforge's shared `.env`.

## Step 1 — jungleforge.sh `settings` and `set`

**Files:** `config/tmuxinator/jungleforge.sh`, `scripts/test_jungleforge.sh`

**Actions:**
- `setting_rows` (KEY, value, comment), `settings_file` (the folder's copy, or
  shared), `settings` (registry hidden, passwords masked).
- `port_owners` (every registered app's and shared port; `next_port` uses it),
  `check_port`, `set_setting`, `rewrite_setting` (`register` uses it).

**Verification:** `scripts/test_jungleforge.sh`.

## Step 2 — ⚙ Settings in the menu

**Files:** `config/tmuxinator/devws_menu.sh`,
`config/tmuxinator/workspace_actions.sh`, `scripts/test_devws_menu.sh`

**Actions:** `change_app_setting`: whose → which → value (asked again while
refused) → restart the app running from this folder, or the services.

**Verification:** `scripts/test_devws_menu.sh`.

## Step 3 — Docs

**Files:** `docs/guide/app.md`

## Final Verification

1. `scripts/check.sh` passes.
2. By hand: change an app's `APP_HTTP_PORT`, restart, see it in Info.

## Commits

- (pending)
