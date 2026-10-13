# Plan: Database pane runs devdb instead of Harlequin

- **Date:** 13-10-2026
- **Branch:** feat/devdb-pane
- **Status:** Executed

## Context

devdb (`~/Projects/devdb`) now browses and edits SQLite, MySQL, PostgreSQL and
MongoDB data in place, which Harlequin cannot. The user asked to remove
Harlequin and use devdb in the database pane.

## Step 1 — Connection URL for devdb

- `database_url.sh` builds one connection string again (`database_url`):
  SQLite → the file path; `DB_*` settings → `mysql://` / `postgres://` /
  `mongodb://` URL with encoded credentials; `DATABASE_URL` / `DB_URL` /
  `MONGODB_URI` / `MONGO_URL` passed through (devdb understands them).
- MongoDB: `DB_CONNECTION=mongodb` (Laravel's MongoDB package) and the URI
  variables count as database settings.
- Drop the per-field output and URL splitting Harlequin needed.

## Step 2 — Runner

- `database_runner.sh` runs `devdb` with the URL in `DEVDB_URL` (never in
  argv); no throwaway config, no footer wait.

## Step 3 — Install / remove Harlequin

- `install.sh --deps` installs devdb with `uv tool install` from
  `DEVWS_DEVDB_SOURCE` (default `~/Projects/devdb`), warning when it is
  missing; Harlequin is no longer installed. `uv tool uninstall harlequin`
  on this machine.

## Step 4 — Tests and docs

- Database tests use a fake `devdb` (records argv and `DEVDB_URL`); install
  test covers the devdb step; README describes devdb.

## Verification

- `./scripts/check.sh`; real devdb in the pane on publisherProvider (copy).

## Execution Notes

- Verified with the real devdb in the pane on publisherProvider: tables and rows
  shown, and `ps` shows only `devdb` (the URL travels in `DEVDB_URL`).
- Harlequin uninstalled from this machine (`uv tool uninstall harlequin`).
