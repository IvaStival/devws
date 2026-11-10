# Plan: Move devAgentics into devws

- **Date:** 04-11-2026
- **Branch:** feat/agentic
- **Status:** In progress

## Steps

1. Copy the crate into `agentic/` (no `.git`, no `target/`). Verify: `cd agentic && cargo test`.
2. `install.sh` `ensure_agentic`, `delete.sh` `remove_owned_agentic`, with lifecycle tests. Verify: `scripts/test_install_lifecycle.sh`.
3. Missing-program message in `agentic.sh`. Verify: `scripts/test_agentic.sh`.
4. Rust part of `scripts/check.sh`; cross test moved to `scripts/test_agentic_cross.sh`. Verify: both run.
5. Docs: guide, READMEs, usage and design notes. Verify: `scripts/test_guide.sh`.
6. Commit (no AI credits, exclude `images/`); archive the old repo only on the user's OK.

## Final Verification

1. `scripts/check.sh` passes.
2. `./install.sh` on a clean HOME builds `devagentics`; `./delete.sh` removes it.
3. `devws agentic` in a real tmux (the live look at the board is still pending).
