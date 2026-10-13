# Plan: Database pane in devws workspaces

- **Date:** 12-10-2026
- **Branch:** feat/database-pane
- **Status:** Executed

## Context

The user wants a database interface inside devws, as a separate pane next to
the editor (not inside Neovim). Their projects use MySQL and PostgreSQL via
Laravel-style `.env` files (`DB_CONNECTION=mysql|pgsql`, `DB_HOST`, …).

Decisions taken with the user:
- Tool: **lazysql** (TUI, MySQL/Postgres/SQLite; `lazysql [url]`, `q` quits,
  config supports `${env:VAR}`), opened **writable**.
- Placement depends on the layout:
  - full-height editor (agent/terminal on the right) → database **below** the
    editor;
  - default (agent/terminal below) → database **to the right** of the editor.
- The database pane gets a header and hides **independently** of the
  agent/terminal (same bar/placeholder mechanism).
- First commit the pending strip/placeholder fix and README on
  `feat/pane-headers`, then branch `feat/database-pane`.

## Approach

### 0. Housekeeping
- Commit pending work on `feat/pane-headers` (two commits), branch
  `feat/database-pane`, save spec/plan under
  `specs/plans/<timestamp>_database-pane/` (spec-plan workflow).

### 1. Connection from the project's `.env` — `config/tmuxinator/database_url.sh` (sourced)
- `database_url <project_root>` prints a lazysql URL, or nothing.
- Safe parse (awk, no `source`): `KEY=value`, `export` prefix, quotes,
  comments. `DATABASE_URL`/`DB_URL` win; else `DB_CONNECTION`:
  `mysql`/`mariadb` → `mysql://`, `pgsql` → `postgres://`, `sqlite` →
  `DB_DATABASE` path (default `database/database.sqlite`).
- Percent-encode user/password; default host `127.0.0.1`, ports 3306/5432.
- `database_kind` prints `mysql`/`postgres`/`sqlite` for the header.

### 2. Pane runner — `config/tmuxinator/database_runner.sh`
- Tags the pane `@devws_role database` + `@devws_db_kind`, then `exec lazysql`.
- Keep the password out of `ps`: export `DEVWS_DB_URL` and point lazysql at
  a private (0700, temp) config with `URL = "${env:DEVWS_DB_URL}"`; verify at
  execution which form connects directly, falling back to the URL argument
  only if needed (and say so in the README).
- No `.env` DB settings → start lazysql with no URL (its own saved
  connections) and show a one-line notice. lazysql missing → message to run
  `./install.sh --deps`.

### 3. Open/focus — `config/tmuxinator/open_database.sh [session]`
- Already open → focus it (restore it first if minimized).
- Else split relative to the editor using `is_side_layout` from
  `workspace_panes.sh`: side → `split-window -v -l 35%` below editor;
  default → `split-window -h -l 40%` right of editor. Then `save_state.sh`.

### 4. Independent hide — generalise `minimize_panes.sh`
- Add a group argument: `minimize_panes.sh [session] [toggle|restore] [agents|database|all]`
  (default `agents`, so `C-b _` and existing callers keep working).
- Placeholders/hidden window shared (`devws-minimized`; killed only when
  empty). Database placeholder role `minimized-database`, label "Database".
- Bar per group: agents as today; database → height 1 (below editor) or
  9-column strip (right of editor).
- Restore per group by stored slot size (`@devws_restore_<group>` = `x:N` /
  `y:N`) instead of the whole-window layout string, so restoring one group
  never re-expands the other; fall back to default proportions.

### 5. Layout toggle — `toggle_layout.sh`
- Restore `all` first; after moving agent/terminal, move the database pane
  with `join-pane` to below (side) or right of (default) the editor.

### 6. Headers, bindings, picker
- `tmux.conf`: header format adds `▾ Database · <kind>` / minimized `▸ DB`;
  click regex adds `(minimized-)?database` and passes the group
  (`database` vs `agents`) to `minimize_panes.sh`; `C-b B` opens/focuses the
  database (B is free after the default-key reset).
- `session_picker.sh`: header command `⛁ Database` → `open_database.sh`
  (shift click-header line numbers).

### 7. Save/restore
- `save_state.sh`: `minimized-database` → `database` (existing sub covers it).
- `restore_state.sh`: a `database` P record reopens the pane via
  `open_database.sh` before the saved layout is applied (pane count must
  match).

### 8. Install, docs
- `Brewfile`: `brew "lazysql"`. README: new "Database pane" section,
  shortcuts table (`C-b B`), configuration list, `.env` support and limits
  (Docker service hostnames such as `mysql` aren't reachable from the host).

## Files

Create: `database_url.sh`, `database_runner.sh`, `open_database.sh` (all in
`config/tmuxinator/`), `scripts/test_workspace_database.sh`.
Modify: `minimize_panes.sh`, `minimized_placeholder.sh` (label only),
`toggle_layout.sh`, `session_picker.sh`, `save_state.sh`, `restore_state.sh`,
`config/tmux/tmux.conf`, `Brewfile`, `scripts/check.sh`,
`scripts/test_workspace_minimize.sh`, `scripts/test_workspace_state.sh`,
`README.md`.

## Verification

- `scripts/test_workspace_database.sh` (real isolated tmux, fake `lazysql`):
  URL building for mysql/pgsql/sqlite/`DATABASE_URL`, special characters,
  missing config; placement in both layouts; open twice focuses; independent
  hide/restore of database vs agents (sizes, processes kept); layout toggle
  repositions the database; header labels and a real mouse click on the
  database header.
- Extended state test: saved `database` pane is reopened on restore.
- Existing layout/minimize tests still pass; `./scripts/check.sh` passes.
- Manual: real lazysql against a MySQL and a Postgres project.

## Execution Notes

- lazysql only resolves `${env:...}` from a config file (with `Provider`), which
  opens its connection picker; the runner presses Enter once the picker is drawn.
- `break-pane` without a target created the hidden window in tmux's most recent
  session; it now targets the workspace's session (also fixed for agents, with a
  regression test).
- A HUP/TERM trap in the runner kept bash waiting on lazysql after the pane
  closed, leaving processes running; only EXIT is trapped now, and a test checks
  that no runner survives.
- Group restore uses the stored slot size instead of the window layout, so
  restoring one group never expands the other.
- SQLite: lazysql cannot open Laravel's `sqlite:///path` URLs and silently
  creates a missing file, so SQLite URLs are turned into paths and a missing
  file is reported in the pane instead.
- The runner first waited for the picker's "Connect" button, which lazysql
  truncates in narrow panes; it now waits for the status line, which starts at
  the left edge.
- Settings in a subfolder (`backend/.env`) are found automatically, and the
  workspace menu's **⛁ Database folder** command picks the folder explicitly;
  the choice is remembered per project in `~/.devws-state/database-folders`
  (removed by `delete.sh`), and `open_database.sh --reconnect` restarts an
  open pane on the new folder.
- Look: the generated lazysql config sets the `tokyo-night` theme preset
  (LazyVim's palette), turns off the update banner, and makes Enter open the
  cell in the word-wrapped JSON viewer. Checked against real lazysql 0.7.0.
