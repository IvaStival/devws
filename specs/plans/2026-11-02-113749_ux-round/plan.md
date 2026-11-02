# Plan: UX round — menu search and keys, one picker style, tidy up, job alerts

- **Date:** 02-11-2026
- **Branch:** feat/ux-round
- **Status:** Executed (not committed yet)

Notes from execution:
- Esc on the branch's kind goes back to the start screen (not the repo
  list): the start screen is where the new-branch path begins.
- The menu itself now runs in `picker.sh` (prompt "Search"), so its own
  repaint loop is gone; tab switches reload with `--items-for {q}`, keeping
  a search going.
- `tidy.sh` with no arguments is the whole flow (list → picker → job), shared
  by the menu and `devws tidy`.
- Test suites that run real jobs set `DEVWS_NOTIFY_COMMAND=true`, so
  `check.sh` never posts notifications; `DEVWS_JOB_NOTIFY_AFTER` lowers the
  30 s threshold in the job test.

## Context

Implements `spec.md` in this folder. Order: the shared picker first (search
and the other pickers build on it), then the menu, then the independent
pieces — tidy, job alerts and progress.

## Step 1 — `picker.sh`, the shared fzf wrapper

**Files:** `config/tmuxinator/picker.sh`, `scripts/test_picker.sh`

**Actions:** `picker.sh <prompt> <header> [fzf options…]` — stdin to fzf with
the shared layout, colors, footer (`Esc back · Enter choose`, plus `Tab
untick` when `--multi`), and the menu's self-repaint (socket + clear-screen
loop, moved here from devws_menu.sh). Exit codes kept (130 = Esc).

**Verification:** `scripts/test_picker.sh` with a stub fzf: the options it
gets (footer, listen socket, the caller's own options), the selection and
exit code passed through, the repaint loop gone after.

## Step 2 — every picker through it, Esc goes back

**Files:** `workspace_actions.sh`, `github_workspace.sh`, `shared_files.sh`,
`git_actions.sh`, tests that stub fzf

**Actions:** replace each `fzf …` question with `picker.sh`; multi-step
flows loop so Esc returns to the previous step (owner ← repo ← branch kind;
branch list ← apps).

**Verification:** existing tests pass; new cases for Esc going back in
`test_github_repo.sh` and `test_devws_menu.sh`.

## Step 3 — menu search and keys

**Files:** `devws_menu.sh`, `config/tmux/tmux.conf`, `scripts/test_devws_menu.sh`

**Actions:**
- `--items-for <query>`: the tab's items when empty, else every tab's items
  tagged `<Tab> · `; fzf reloads on `change` with `--disabled` matching done
  by fzf's own filter over the full list (search mode keeps fuzzy ranking).
- Shortcut column for the new keys; `bind e/E/g/G/j` → `devws_menu.sh
  --popup-run <action> [client]` (the action alone in the menu-sized popup).

**Verification:** tests: an empty query lists the tab; "datab" lists `App ·
⛁ Database`; `--popup-run` runs the action's function; tmux.conf loads.

## Step 4 — Tidy up

**Files:** `config/tmuxinator/tidy.sh`, `workspace_actions.sh`,
`devws_menu.sh`, `shell/devws.zsh`, `scripts/test_tidy.sh`

**Actions:** `tidy.sh candidates` (managed worktrees: PR merged/closed from
the cache, refreshed first with `pr-query`; clean; no open session's root);
`tidy.sh remove <folder>…` (worktree.sh remove each, report kept ones). Menu
item and `devws tidy`: picker (all ticked) → job.

**Verification:** `scripts/test_tidy.sh` with a stub gh: merged+clean listed;
open, dirty or open-PR not; removal keeps branches.

## Step 5 — job alerts and progress

**Files:** `config/tmuxinator/job.sh`, `scripts/test_job.sh`

**Actions:** after `run`: notify on failure, or on success after > 30 s
(`DEVWS_NOTIFY`, `DEVWS_NOTIFY_COMMAND` for tests; terminal-notifier with
`-execute` opening the log, else osascript). `rows`: the last `[n/m]` in the
log tail → a 10-cell bar (header_bar.sh) on the job's entry.

**Verification:** tests: failure notifies, quick success does not, an aged
long success does; a log with `[dev 4/9]` shows `4/9` and a bar.

## Step 6 — docs and check

**Files:** `README.md`, `scripts/check.sh`

**Verification:** `scripts/check.sh` passes.

## Final Verification

1. `scripts/check.sh` passes.
2. Manual: `C-b Space`, type "data" → App · Database; `C-b e` runs the
   branch; Esc in the repo list goes back to the owners; Tidy lists a merged
   branch folder; a failing `devws job` notifies and the click opens its log.
