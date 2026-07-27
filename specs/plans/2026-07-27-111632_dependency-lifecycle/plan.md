# Plan: Safe dependency lifecycle during installation

- **Date:** 27-07-2026
- **Branch:** main
- **Status:** Executed

## Context

Implement the dependency ownership and recovery behavior defined in
`spec.md`, with Homebrew as the only automatic package manager.

## Step 1 — Add isolated lifecycle tests

**Files:**
- `scripts/test_install_lifecycle.sh`
- `scripts/check.sh`

**Actions:**
- Build a temporary-home test harness with mocked Homebrew commands.
- Cover pre-existing and missing `fzf`, accepted and declined installation,
  tmuxinator upgrade detection, uninstall ownership, and reload output.
- Run the lifecycle suite from the standard project check.

**Verification:**
- Confirm the new tests fail against the current installer behavior for the
  expected missing features.

## Step 2 — Validate and manage dependencies during install

**Files:**
- `install.sh`

**Actions:**
- Check `fzf` before creating state, backups, links, or shell integration.
- Prompt interactively before installing missing `fzf` with Homebrew and fail
  safely when consent or Homebrew is unavailable.
- Record only a devws-installed `fzf` in dependency state.
- Upgrade a Homebrew-managed tmuxinator automatically during dependency
  installation; preserve and report non-Homebrew installations.
- Print a separated colored shell reload instruction.

**Verification:**
- Run the isolated tests for all installer branches and Bash syntax checks.

## Step 3 — Remove only dependencies owned by devws

**Files:**
- `delete.sh`

**Actions:**
- Read dependency ownership state without executing it as shell code.
- Uninstall `fzf` only when state records that devws installed it.
- Preserve the ownership marker and warn if Homebrew removal fails; otherwise
  clean the dependency state alongside existing uninstall state.

**Verification:**
- Test both pre-existing and devws-installed `fzf` uninstall scenarios.

## Step 4 — Prevent picker restart loops

**Files:**
- `config/tmuxinator/session_picker.sh`

**Actions:**
- Add an early `fzf` availability guard before entering the persistent picker
  loop.
- Emit one actionable installation/restart message and exit non-zero.

**Verification:**
- Run the picker with a mocked missing `fzf` path and assert one error only.

## Step 5 — Document and finalize

**Files:**
- `README.md`
- `spec.md`
- `plan.md`

**Actions:**
- Document the prompt, tmuxinator upgrade, dependency ownership, reload
  command, and uninstall behavior.
- Mark the spec and plan as executed after all verification succeeds.

**Verification:**
- Run `make check`, `git diff --check`, and review the final diff for unrelated
  or sensitive files.

## Final Verification

1. All Bash and Zsh syntax checks pass.
2. Isolated install/delete lifecycle tests pass without modifying the real
   home directory or Homebrew installation.
3. Missing `fzf` cannot create a picker error loop.
4. Existing dependency and configuration state is preserved.
