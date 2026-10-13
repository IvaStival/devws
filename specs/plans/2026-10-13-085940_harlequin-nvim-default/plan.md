# Plan: Harlequin database pane, Neovim as default editor, LunarVim removed

- **Date:** 13-10-2026
- **Branch:** feat/database-pane
- **Status:** Executed

## Context

Implements `spec.md` in this folder.

## Step 1 — Move the two LunarVim workspaces to nvim

- Respawn the editor panes of dev-ESP32 and dev-devws with
  `editor_runner.sh nvim` and set `@devws_editor nvim` (what the picker's
  "Switch editor" does); save state.
- **Verify:** no `lvim` process left.

## Step 2 — Uninstall LunarVim

- LunarVim's `uninstall.sh --remove-config` (binary, runtime, cache, config
  links), then `~/.local/share/lvim` and `~/.local/state/lvim`.
- **Verify:** `command -v lvim` fails; folders gone; personalWorkspace intact.

## Step 3 — Harlequin runner

- `database_url.sh`: also expose host, port, user, password, database
  (`database_host`…), parsing them from a MySQL/Postgres `DATABASE_URL`.
- `database_runner.sh`: write a throwaway Harlequin config with a `devws`
  profile (adapter, connection fields, `password = "${DEVWS_DB_PASSWORD}"`,
  theme `tokyo-night`), export the password, run
  `harlequin --config-path <file> --profile devws`; SQLite passes the file.
  Remove the config once Harlequin has read it, and on exit.
- No database settings → message pointing at **⛁ Database folder**.

## Step 4 — Remove lazysql, install Harlequin

- Drop lazysql from runner, Brewfile and README; `brew uninstall lazysql`.
- Brewfile `brew "uv"`; `install.sh --deps` runs
  `uv tool install 'harlequin[postgres,mysql]'` when Harlequin is missing.

## Step 5 — Neovim as default editor

- `shell/devws.zsh`: `export EDITOR=nvim VISUAL=nvim` when unset.

## Step 6 — Tests and docs

- Database tests: fake `harlequin` records args/config/env; check
  SQLite/MySQL/Postgres invocations, no password in args or leftover config,
  Tokyo Night theme. Install test for the Harlequin step. README updated.

## Final Verification

1. `./scripts/check.sh` passes.
2. Real Harlequin opens publisherProvider's SQLite in the pane.
3. `lvim` gone; new shell has `EDITOR=nvim`.

## Execution Notes

- LunarVim: dev-ESP32 and dev-devws switched to nvim, an orphaned LunarVim
  language server stopped, then `uninstall.sh --remove-config` plus
  `~/.local/share/lvim` (841 MB) and `~/.local/state/lvim` removed;
  personalWorkspace's lvim files untouched.
- Harlequin's MySQL adapter refuses a DSN, so `database_url.sh` now yields
  separate fields (and splits a `DATABASE_URL`); a Postgres URL's query
  options such as `sslmode` are not carried over.
- The runner waits for Harlequin's footer ("Quit"), not a panel title, which
  is cut short in narrow panes.
- lazysql removed from devws and uninstalled from Homebrew; `install.sh
  --deps` installs Harlequin with `uv tool install 'harlequin[postgres,mysql]'`
  (installing uv with Homebrew only when it is missing).
- Verified with real Harlequin: SQLite on publisherProvider; MySQL and
  Postgres profiles accepted (connection refused, no server here).
