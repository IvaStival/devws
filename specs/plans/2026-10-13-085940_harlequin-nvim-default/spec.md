# Spec: Harlequin database pane, Neovim as default editor, LunarVim removed

- **Date:** 13-10-2026
- **Branch:** feat/database-pane (continues the database pane work)
- **Status:** Executed

## Context

After trying Harlequin on publisherProvider, the user wants it as the
database pane's client instead of lazysql: a cleaner, LazyVim-like look
(Tokyo Night, rounded borders) with a catalog, SQL editor and results grid.
They also want Neovim as the default text editor, and LunarVim removed from
the machine (devws stopped using it in the LazyVim switch).

Findings:

- Harlequin 2.16.1 is installed with `uv tool install
  'harlequin[postgres,mysql]'` (sqlite and duckdb built in). There is no
  MongoDB adapter.
- Its MySQL/Postgres adapters take the password only as `--password`, which
  shows in `ps`; but its config file resolves `${VAR}` placeholders in a
  profile, so the password can travel in the environment, as with lazysql.
- `EDITOR`/`VISUAL` are unset (tools fall back to vim); git's
  `core.editor` is `vim`.
- `dev-ESP32` and `dev-devws` still run LunarVim. LunarVim occupies
  `~/.local/share/lunarvim` (91 MB), `~/.local/share/lvim` (841 MB, not
  removed by its uninstaller), `~/.local/state/lvim`, `~/.cache/lvim`,
  `~/.config/lvim` (links into personalWorkspace) and `~/.local/bin/lvim`.

## Requirements

1. The database pane runs Harlequin by default, with the Tokyo Night theme,
   connected from the same `.env` / folder choice as today:
   - SQLite → the database file;
   - MySQL / Postgres → host, port, user, database from `.env`, password
     through an environment variable referenced from a throwaway Harlequin
     config profile (never in `ps`; config removed once read).
2. lazysql is removed: Harlequin is the only database client (runner,
   Brewfile, tests, README, and the Homebrew package devws installed).
3. `install.sh --deps` installs Harlequin with the MySQL/Postgres adapters
   (via `uv`, added to the Brewfile).
4. Neovim is the default text editor: devws's shell integration exports
   `EDITOR=nvim` and `VISUAL=nvim` unless the user already set them.
5. LunarVim is removed from this machine — after the two workspaces running
   it are switched to `nvim` — including the 841 MB data folder its
   uninstaller leaves behind.
6. Tests, README and docs reflect the change.

## Constraints

- No password on a command line or left on disk.
- Running editors are not killed: LunarVim is removed only once no
  workspace runs it.
- `personalWorkspace` files are untouched (only links to them are removed).

## Decisions

- lazysql: removed (user, 13-10-2026).
- LunarVim editors in dev-ESP32 / dev-devws have no unsaved changes (user),
  so they are switched to nvim directly.
- git's global `core.editor` (vim) is left alone until the user decides.

## Success Criteria

- [ ] Opening the database pane shows Harlequin connected, for SQLite (real
      check on publisherProvider) and MySQL/Postgres (tested with a fake).
- [ ] No password in Harlequin's arguments or remaining config.
- [ ] No lazysql left in devws or installed by it.
- [ ] New shells have `EDITOR=nvim`/`VISUAL=nvim`.
- [ ] `lvim` is gone and its folders removed; no workspace uses it.
- [ ] `scripts/check.sh` passes.
