# Spec: Database pane in devws workspaces

- **Date:** 12-10-2026
- **Branch:** feat/database-pane
- **Status:** Executed

## Context

The user wants a database interface inside devws as a separate pane, not
inside Neovim. Their projects use MySQL and PostgreSQL through Laravel-style
`.env` files.

## Requirements

1. A workspace command (picker **⛁ Database**, `C-b B`) opens lazysql
   connected to the project's database from its `.env`; opening again
   focuses the existing pane.
2. Placement follows the layout: below the editor when agent/terminal are on
   the right; right of the editor when they are below.
3. The database pane has a header and hides to a bar independently of the
   agent/terminal group, with the same click behaviour.
4. Toggling the layout moves the database pane; save/restore reopens it.
5. The password never appears in the process list or on disk.

## Constraints

- lazysql opens writable; telemetry off.
- No change to the saved-state format.
- Agent/terminal minimizing keeps working as before.

## Success Criteria

- [x] URL built correctly for mysql/pgsql/sqlite/`DATABASE_URL`, special characters.
- [x] Placement, focus, independent hiding, layout toggle and header clicks tested on a real tmux server.
- [x] Restore reopens a saved database pane.
- [x] No password in argv or config; config and processes cleaned up when the pane closes.
- [x] `scripts/check.sh` passes.
