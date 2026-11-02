# Plan: GitHub workspaces — GitHub as the source of truth

- **Date:** 01-11-2026
- **Branch:** feat/github-workspaces
- **Status:** Executed (not committed yet)

## Context

Implements `spec.md` in this folder.

## Step 1 — github_repo.sh

**Files:** `config/tmuxinator/github_repo.sh`, `scripts/test_github_repo.sh`

**Actions:**
- `home`, `parse <owner/repo|url>`, `list [--refresh]` (gh, cached 1h),
  `cloned`, `ensure <repo>` (init bare + origin + refspec, fetch, `set-head --auto`),
  `default-branch <dir>`, `worktrees <repo dir>`, `pr <dir>` (cached state),
  `pr-refresh <dir>…` (background, throttled to 5 min).
- Clone URL from `gh config get git_protocol`; `DEVWS_CLONE_BASE` overrides it (tests, GitHub Enterprise).

**Verification:** `scripts/test_github_repo.sh` with a local bare origin and a stub `gh`.

## Step 2 — worktree.sh for bare repos

**Files:** `config/tmuxinator/worktree.sh`, `scripts/test_worktree.sh`

**Actions:** folder rule for a `.bare` common dir; `ensure <dir> <branch> [base]`
branches off base with `--no-track`; `remove` runs from the common dir.

**Verification:** `scripts/test_worktree.sh` (old cases plus bare-repo cases).

## Step 3 — GitHub workspace flow

**Files:** `config/tmuxinator/github_workspace.sh`, `workspace_actions.sh`, `devws_menu.sh`, `shell/devws.zsh`

**Actions:** repo picker → reopen-or-new → branch name → `worktree.sh ensure`;
prints the folder. `devws` with no folder and the menu's "New GitHub workspace" start it.

**Verification:** branch-name slug unit tests in `test_github_repo.sh`; manual run.

## Step 4 — Commit and Push

**Files:** `config/tmuxinator/git_actions.sh`, `scripts/test_git_actions.sh`, `devws_menu.sh`, `workspace_actions.sh`, `shell/devws.zsh`, `scripts/test_devws_menu.sh`

**Actions:** `commit [--manual]`, `push`; Git tab; `devws commit|push`.

**Verification:** `scripts/test_git_actions.sh`, `scripts/test_devws_menu.sh`.

## Step 5 — Sidebar PR state

**Files:** `config/tmuxinator/session_picker.sh`

**Actions:** show the cached PR state on the branch line; start a background refresh.

**Verification:** manual — the sidebar shows "PR open" after Push.

## Step 6 — Resume agents on restore

**Files:** `config/tmuxinator/dev.yml`, `restore_state.sh`, `scripts/test_workspace_state.sh`

**Actions:** `resume=1` from restore → `agent_runner.sh <agent> --resume`.
The existing `--resume` continues the folder's last conversation, and each
worktree is its own folder, so no session ids are stored (simpler than the
first plan, which saved them in the state file).

**Verification:** `scripts/test_workspace_state.sh` checks `resume=1`; check.sh renders dev.yml.

## Step 7 — Config and docs

**Files:** `.env`, `Brewfile`, `README.md`, `scripts/check.sh`

**Verification:** `scripts/check.sh` passes.

## Final Verification

1. `scripts/check.sh` passes.
2. Manual: `devws` → repo → new branch → workspace on `~/.devws/<owner>/<repo>/<repo>-<branch>`; Commit; Push → PR; sidebar shows it; kill tmux → `devws restore` resumes the agent.
