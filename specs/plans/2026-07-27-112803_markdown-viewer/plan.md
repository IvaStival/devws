# Plan: Markdown viewing with Glow

- **Date:** 27-07-2026
- **Branch:** main
- **Status:** Executed

## Context

Implement the Glow-based Markdown workflows defined in `spec.md`, reusing the
existing Homebrew dependency ownership state and project checks.

## Step 1 — Add regression coverage

**Files:**
- `scripts/test_markdown_viewer.sh`
- `scripts/test_install_lifecycle.sh`
- `scripts/check.sh`

**Actions:**
- Add isolated tests for direct file opening, interactive selection, invalid
  input, missing Glow, spaces in paths, and dependency ownership.
- Extend the mocked Homebrew lifecycle to distinguish pre-existing and
  devws-installed Glow.

**Verification:**
- Confirm the tests expose the currently missing command and ownership behavior.

## Step 2 — Manage Glow installation ownership

**Files:**
- `Brewfile`
- `install.sh`
- `delete.sh`

**Actions:**
- Add the Glow formula.
- Snapshot whether Glow existed before `--deps`, then record ownership only
  when the dependency installation added it.
- Generalize safe dependency removal so only recorded Homebrew dependencies
  are removed.

**Verification:**
- Run lifecycle cases for new and pre-existing Glow installations.

## Step 3 — Add the shell Markdown command

**Files:**
- `shell/devws.zsh`

**Actions:**
- Dispatch `devws md` before normal workspace argument parsing.
- Validate arguments and Markdown file paths.
- Use `find` plus `fzf` when no path is supplied and pass the chosen path to
  `glow -p` without unsafe string evaluation.

**Verification:**
- Test direct, selected, spaced, invalid, cancelled, and missing-command cases.

## Step 4 — Add LunarVim preview

**Files:**
- `config/lvim/config.lua`

**Actions:**
- Add `<leader>mp` for Markdown buffers.
- Save modified named buffers, open `glow -p` through `termopen` in a bottom
  split, and close only that preview buffer when the process exits.
- Notify clearly for unsupported buffers or missing Glow.

**Verification:**
- Validate Lua syntax and inspect the argument-vector invocation and cleanup
  callback.

## Step 5 — Document and finalize

**Files:**
- `README.md`
- `spec.md`
- `plan.md`

**Actions:**
- Document dependency installation, both opening workflows, shortcut, errors,
  and uninstall ownership.
- Mark the specification and plan executed after verification.

**Verification:**
- Run `make check`, `git diff --check`, and review the scoped diff.

## Final Verification

1. Shell, Zsh, and Lua syntax checks pass.
2. Markdown command tests pass without opening an interactive real pager.
3. Install/delete lifecycle tests prove Glow ownership behavior.
4. Existing applications and configuration remain untouched in temporary-home
   scenarios.
