# Spec: devws guide and a Help tab in the menu

- **Date:** 02-11-2026
- **Branch:** feat/devws-guide (off feat/ai-memory, once that is committed)
- **Status:** Executed

## Context

devws now does a lot: GitHub workspaces, worktrees, layouts and panels,
agents and accounts, commit and push, jungleforge runs, background jobs and
their log panel, tidy, detach, ai-memory. Today all of it is documented in
one 1,000-line README. Nothing in devws itself says what it can do: the
only way to learn a feature, or a key that runs it, is to open the README
outside devws. The guide should be readable from inside devws, from the
menu, and it should stay true as devws changes.

## Requirements

1. **The guide**: a folder `docs/guide/` of Markdown pages, one per area,
   following the menu's tabs, so a page answers "what can I do here":
   - `README.md`: start here. What devws is, its screen (sidebar, header,
     agent, editor, terminal), the first workspace, and the list of pages.
   - `workspaces.md`: the start screen (recent, new branch from GitHub,
     clone), folder and worktree workspaces, closing, detaching, tidy,
     restore after a restart, paused workspaces.
   - `layout.md`: layouts, new and moved panels, minimize, the header, the
     editor and following the agent.
   - `agents.md`: agents and accounts, agent status, switch, restart,
     initialize, register and remove, ai-memory.
   - `git.md`: commit (agent or by hand), push and PRs, branch folders.
   - `app.md`: running a branch in jungleforge, switching, stopping,
     services, migrate, shell, logs, the database pane, background jobs and
     the log panel, failure hints, notifications.
   - `shortcuts.md`: every tmux key devws adds, and the menu's own keys.
   - `commands.md`: every `devws` subcommand.
   - `configuration.md`: every setting (`.env` and the `DEVWS_*` variables).
   Each feature says what it does, how to reach it (menu item, key,
   command), and what to expect, including what happens when it fails.
2. **The README** keeps what someone needs before devws runs: what it is,
   requirements, install, update, validate and uninstall. Its "Use",
   "Configuration" and feature sections move into the guide, and it links to
   `docs/guide/README.md`. No feature is described in both places.
3. **A Help tab** in the menu, last after App: one item per guide page
   (`? Start here`, `⌨ Shortcuts`, `⎇ Workspaces`…). Choosing one closes the
   menu and opens the page in a large popup (90% × 90%) with Glow's pager,
   where `/` searches and `q` closes. Typing in the menu searches the Help
   items like every other tab's.
4. **`devws help [page]`** opens the same page in the terminal (no page:
   start here); an unknown page lists the pages.
5. **The guide stays true**, checked by a test: every menu action's label
   appears in the guide, every `devws` subcommand appears in `commands.md`,
   every key devws binds in `tmux.conf` appears in `shortcuts.md`, every
   `DEVWS_*` variable the scripts read appears in `configuration.md`, and
   every guide page has a Help item. A new feature without its docs fails
   `scripts/check.sh`.

## Constraints

- The guide describes devws as it is now, read from the code, not from
  older plans.
- Glow is already a dependency (`devws md`). Mermaid diagrams are not
  needed.
- The menu's tab mechanics stay the same: one more tab in `tabs` and rows in
  `actions`.
- The pages are found from the devws checkout (the scripts' real location),
  not copied into `~/.tmuxinator`.

## Files Involved

| File | Action |
| ---- | ------ |
| `docs/guide/*.md` | Create — the nine pages |
| `README.md` | Modify — keep install and upkeep, link to the guide |
| `config/tmuxinator/devws_menu.sh` | Modify — `help` tab and its items |
| `config/tmuxinator/workspace_actions.sh` | Modify — open a guide page in a popup |
| `bin/devws` | Modify — `devws help [page]` |
| `config/tmuxinator/guide.sh` | Create — shows a guide page |
| `scripts/test_guide.sh` | Create — the coverage test |
| `scripts/test_devws_menu.sh` | Modify — the Help tab |
| `scripts/check.sh` | Modify — `test_guide` in the quick list |

## Success Criteria

- [ ] The menu has a Help tab whose items open each guide page in a large popup.
- [ ] `devws help`, `devws help shortcuts` open pages; `devws help nope` lists them.
- [ ] `scripts/test_guide.sh` fails when a menu item, subcommand, key or setting is missing from the guide.
- [ ] The README holds no feature description the guide also has.
- [ ] `scripts/check.sh` passes.
