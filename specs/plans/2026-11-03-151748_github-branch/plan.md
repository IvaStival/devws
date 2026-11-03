# Plan: open an existing GitHub branch from the start screen

- **Date:** 03-11-2026
- **Branch:** feat/github-branch
- **Status:** Executed

## Context

Today the start screen's **＋ New branch from GitHub…** row always creates a
new branch (kind + description, off `origin/<default>`). There is no way to
open a branch that already exists on GitHub (a teammate's branch, one you
pushed from another machine) unless a worktree for it already exists.
`worktree.sh ensure` can already check out a remote branch and set it to
track; the start screen just never offers one.

Chosen design (confirmed with the user): keep one row, renamed
**⎇ Branch from GitHub…**. After you pick the repo, a **Branch** step lists
**+ New branch…** first, then the repo's GitHub branches, newest first.

```
Branch >
  + New branch…
  feat/remote     2 hours ago    ● folder
  fix/crash       3 days ago
  main            1 week ago
```

## Changes

### `config/tmuxinator/github_repo.sh`
- **New command** `branches <dir>`: the repo's `origin/*` branches, newest
  commit first, as `<branch>\t<relative date>`. `origin/HEAD` is left out.
  It uses `git for-each-ref --sort=-committerdate --format='%(refname:lstrip=3)\t%(committerdate:relative)' refs/remotes/origin`,
  sitting next to `worktrees` and `default-branch`. Add it to the header
  usage comment and the `case` dispatch.

### `config/tmuxinator/github_workspace.sh`
- Start screen row `new_row` becomes `'⎇ Branch from GitHub…'`. Update the
  picker header to "…or open a GitHub branch" and the file's top comment.
- **New function** `pick_branch <bare>`: prints the chosen branch name.
  - Rows: `+ New branch…` first, then each line from `github_repo.sh branches`.
    Branches that already have a folder (from `github_repo.sh worktrees`) are
    marked `● folder`.
  - Uses the existing `picker.sh` with `--delimiter=$'\t'`, the same way
    `pick_worktree` does.
  - `+ New branch…` calls the existing `ask_branch`. Esc on its Kind question
    goes back to the branch list (a loop), not out.
  - Esc on the branch list returns 1, so the caller's existing loop goes back
    to the start screen (or exits for `github_workspace.sh <repo>`), as Esc
    on Kind does now.
- `main`: `ask_branch` is replaced by `pick_branch "$bare"`. The rest stays
  the same: `worktree.sh ensure "$bare" "$branch" "$base"` already reopens a
  branch's folder, tracks a remote branch, or creates a new one off `$base`.
  `shared_files.sh link` still runs for every folder.
- The `<repo>` path (`pick_worktree`, then **+ New branch**) now goes through
  `pick_branch` too, so GitHub branches can be opened from there as well.

### `docs/guide/workspaces.md`
- Show the start screen with the renamed row. Rewrite step 2 (**Branch**) as:
  pick a GitHub branch (checked out in its own folder, tracking GitHub, or
  its folder reopened when it has one), or **+ New branch…**, then kind +
  words / **Custom name…** as before.

### `scripts/test_github_repo.sh` (the fixture already pushes `feat/remote`)
- **New** `test_branches_lists_remote_branches_newest_first`:
  `github_repo branches` lists `feat/remote` and `main` with dates, and no `HEAD`.
- **New** `test_an_existing_github_branch_is_opened`: start screen →
  `⎇ Branch` → `  acme` → `● app` → `feat/remote` gives
  `$DEVWS_HOME/acme/app/app-feat-remote`, whose branch tracks `origin/feat/remote`
  (`git rev-parse --abbrev-ref @{u}`). Picking it again shows `● folder` and
  reopens the same folder.
- Update the existing answer queues for the extra step: `'＋ New branch'` →
  `'⎇ Branch'`, and add `'+ New branch'` before the Kind answer in
  `test_a_new_branch_is_picked_by_owner` and
  `test_a_new_workspace_gets_the_imported_files`.
- Esc on Kind returns to the branch list: covered in the new-branch test
  (`'+ New branch' '' '+ New branch' 'fix'`).

## Workflow
- Branch `feat/github-branch` off `main`. Save this plan in
  `specs/plans/<date>_github-branch/plan.md`. Run the vault update
  (`~/.claude/scripts/obsidian-update.sh`) at plan approval and after implementing.
- TDD: write the new tests first and watch them fail, then implement.

## Verification
- `bash scripts/test_github_repo.sh`, then `make check`. The known failures
  on `main` (`test_git_actions`, `test_workspace_minimize`) stay as they are.
- By hand: `devws` → **⎇ Branch from GitHub…** → this repo (`IvaStival/devws`)
  → pick `fix/header-clock-memory-dot`. The workspace should open in
  `~/.devws/IvaStival/devws/devws-fix-header-clock-memory-dot` on the GitHub
  branch, tracking it.
- Commit with the commit standard (no AI credit), push, and open a PR when asked.
