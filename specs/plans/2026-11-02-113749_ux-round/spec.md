# Spec: UX round — menu search and keys, one picker style, tidy up, job alerts

- **Date:** 02-11-2026
- **Branch:** feat/ux-round (off feat/background-jobs)
- **Status:** Executed

## Context

After GitHub workspaces, the App tab and background jobs, the interface has
grown unevenly: actions are found by knowing their tab, the frequent new ones
have no keys, every picker looks and cancels differently, branch folders pile
up under `~/.devws`, and a job that ends while you are elsewhere goes unseen.
Decisions come from a grilling session on 02-11-2026.

## Requirements

### Menu search and keys
1. With the menu's query empty it shows the current tab; once something is
   typed it lists matching actions of every tab, each tagged with its tab
   (`App · ⛁ Database`). Clearing the query shows the tab again.
2. New keys: `C-b e` Run this branch, `C-b E` Switch running branch,
   `C-b g` Commit (agent), `C-b G` Push / PR, `C-b j` Job log — shown beside
   the actions in the menu. They open the action in a popup like the menu's.

### One picker style
3. Every fzf question devws asks (owner, repo, apps, branch kind, agent,
   editor, close, start screen…) goes through one wrapper: same layout,
   prompt style, header line, footer `Esc back · Enter choose` (`Tab untick`
   on multi-select), and the self-repaint the menu has.
4. Esc goes back one step in multi-step questions (repo → owner, branch kind
   → repo, apps → branch list); at the first step it cancels.

### Tidy up
5. **⌫ Tidy up** (Workspace tab) and `devws tidy` list the branch folders
   under `DEVWS_HOME` whose PR is merged or closed, with no uncommitted
   changes and no workspace open on them — all ticked, Tab unticks. The
   ticked folders are removed (`git worktree remove`) in a background job.
   Local branches are kept.
6. The start screen shows a merged branch's state (`· PR merged`).

### Job alerts and progress
7. A job that fails always posts a macOS notification; one that succeeds
   does when it ran longer than 30 s. Clicked, it opens the job's log.
   `DEVWS_NOTIFY=0` turns them off. Same notifier as the agents'
   (terminal-notifier, else osascript).
8. A running job whose log shows Docker's step count (`[dev 4/9]`, `#7
   [4/9]`) gets a bar on its header row: `⏵ phi-planner ▓▓▓▓░░░░░ 4/9 0:42`.

## Constraints

- Keys must not take ones already bound (`r`, `R`, `L` are).
- The wrapper must keep each picker's own options (multi-select, delimiter,
  print-query) working.
- Tidy never forces a removal and never deletes a branch.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/devws_menu.sh` | Modify — search across tabs, `--run` from keys, new items |
| `config/tmuxinator/picker.sh` | Create — the shared fzf wrapper |
| `config/tmuxinator/workspace_actions.sh`, `github_workspace.sh`, `shared_files.sh`, `git_actions.sh` | Modify — use the wrapper; Esc back; tidy action |
| `config/tmuxinator/tidy.sh` | Create — candidates and removal |
| `config/tmuxinator/job.sh` | Modify — alerts on end; progress bar |
| `config/tmux/tmux.conf` | Modify — keys |
| `shell/devws.zsh` | Modify — `devws tidy` |
| `scripts/test_tidy.sh`, `scripts/test_picker.sh` | Create — tests |
| `scripts/test_devws_menu.sh`, `scripts/test_job.sh`, `scripts/check.sh`, `README.md` | Modify |

## Success Criteria

- [ ] Typing in the menu finds an action of another tab; an empty query shows the tab.
- [ ] The five keys open their actions.
- [ ] Every picker shows the shared footer; Esc in the repo list returns to the owners, in the apps list to the branch list.
- [ ] Tidy lists only merged/closed, clean, closed-workspace folders and removes the ticked ones in a job.
- [ ] A failed job notifies; a 5 s success does not, a 40 s one does.
- [ ] A Docker log `[dev 4/9]` shows a 4/9 bar on the job row.
- [ ] `scripts/check.sh` passes.
