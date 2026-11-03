# Plan: devws guide and a Help tab in the menu

- **Date:** 02-11-2026
- **Branch:** feat/devws-guide (off feat/ai-memory, 4a30a0d)
- **Status:** Executed — 4c9594f (guide and Help tab), 6a64e96 (dead tmux keys), bfe25e2 (memory test stub)

Notes from execution:
- `config/tmux/tmux.conf` sources `default-keys.conf`, which wipes every
  binding made before it and restores tmux's defaults: the custom keys above
  it (`C-b |`, `C-b r`, `C-b C-e`…) were dead. The test counts only the keys
  bound after it, and `shortcuts.md` documents tmux's own defaults instead.
- The pages are shown by a new `config/tmuxinator/guide.sh [page]`, which
  finds `docs/guide/` through its own links; both `devws help` and the Help
  items call it.
- Not settings, so left out of `configuration.md`: the test overrides
  (`DEVWS_*_NOW`, `DEVWS_AI_MEMORY`, `DEVWS_NOTIFY_COMMAND`…), values devws
  passes between its scripts (`DEVWS_ROOT`, `DEVWS_JOBS_DIR`…) and the
  header colors (from tmux.conf's `@devws_color_*`); the test lists them.

## Context

Implements `spec.md` in this folder. The coverage test comes first, so
writing the pages is driven by what it reports missing. Then the pages, the
README trim, and last the ways to open the pages (`devws help`, Help tab).

## Step 1 — coverage test

**Files:** `scripts/test_guide.sh`, `scripts/check.sh`

**Actions:**
- Collect from the code: menu labels (the `actions` table in
  `devws_menu.sh`, icon dropped), `devws` subcommands (the dispatch in
  `bin/devws`), keys (`bind-key`/`bind` lines in `config/tmux/tmux.conf`,
  as `C-b <key>`), `DEVWS_*` variables read in `config/`, `bin/`, `install.sh`.
- Fail, listing what is missing, unless each appears in its page
  (labels anywhere in `docs/guide/`, subcommands in `commands.md`, keys in
  `shortcuts.md`, variables in `configuration.md`); every page has a Help
  row in the menu, and every Help row names a page that exists.
- Add `test_guide` to check.sh's quick list.

**Verification:** run it now: it fails, listing everything (no guide yet).

## Step 2 — the pages

**Files:** `docs/guide/{README,workspaces,layout,agents,git,app,shortcuts,commands,configuration}.md`

**Actions:**
- Write each page from the code and the README's current sections, checking
  behavior against the scripts (not older plans).
- Each feature: what it does, how to reach it (menu item · key · command),
  what to expect and what failure looks like. Tables for keys, commands and
  settings.

**Verification:** `scripts/test_guide.sh` reports nothing missing except the
Help rows (Step 4). Read each page with `glow`.

## Step 3 — README trim

**Files:** `README.md`

**Actions:** keep the intro, Requirements (with Shift+Enter in iTerm2),
Install, Update, Validate, Uninstall; replace the rest with a short "Using
devws" section linking to each guide page and saying the guide opens from
the menu's Help tab and `devws help`.

**Verification:** every removed section's content is found in a guide
page (diff the headings); links resolve.

## Step 4 — `devws help` and the Help tab

**Files:** `bin/devws`, `config/tmuxinator/devws_menu.sh`,
`config/tmuxinator/workspace_actions.sh`, `scripts/test_devws_menu.sh`

**Actions:**
- `bin/devws`: `devws help [page]` → `devws-markdown
  $checkout/docs/guide/<page>.md` (no page: README); an unknown page
  prints the pages and exits 2.
- `devws_menu.sh`: `help` tab last; its rows' function column takes an
  argument (`open_guide shortcuts`), `run_action` splitting it into words.
- `workspace_actions.sh`: `open_guide <page>` opens, once the menu popup has
  closed (`run-shell -b`), a 90% × 90% popup on the same client running
  `devws help <page>`.
- Menu test: the Help tab lists every page; running a Help item opens the
  popup with the page (tmux stub).

**Verification:** `scripts/test_devws_menu.sh`, `scripts/test_guide.sh`
pass; `devws help nope` lists the pages.

## Final Verification

1. `scripts/check.sh` passes.
2. Manual: C-b Space → Help → Shortcuts opens the page large; `/` searches,
   `q` closes; `devws help git` in a terminal.
