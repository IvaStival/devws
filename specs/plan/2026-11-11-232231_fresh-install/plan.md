# Plan: Fresh-install dependencies and menu authentication

- **Date:** 11-11-2026
- **Branch:** fix/fresh-install
- **Status:** In progress
- **Authorization:** User explicitly requested these installer changes; proceed with implementation.

## Steps

1. Add regression tests for default dependency installation without a fzf prompt, no authentication during installation, missing/local/remote devdb, and failure before configuration writes. Update lifecycle fixtures for the new default.
2. Run dependencies before fzf validation and configuration writes; install uv with the Brewfile; fall back to the public devdb source when no default local checkout exists. Keep explicit source errors actionable and preserve ownership tracking.
3. Add and test the Git-tab authentication action; remove the installer login prompt. Update installation and menu guides.
   Include the later server report: regression-test the rendered board command under an old tmux PATH; add its Rust/local bin paths and prepare Rust by default. Verify build failures stop before linking configuration.
4. Run lifecycle/menu tests, `make check`, and `git diff --check`. Record results and remaining platform limitations.

## Verification

Tests use isolated HOME directories and stub package managers; no dependency installations or authentication changes on this development machine. Full checks cover existing install/uninstall, menu, and workspace behavior.

## Progress

- Steps 1–3 implemented, including the later `devagentics` server error.
- Reproduced missing tools under an older tmux PATH using the real board template and database runner, then added explicit Rust/uv bin paths.
- Lifecycle tests cover automatic Rust setup, build failures, dependency failures, devdb fallback and explicit local sources, and no installer authentication.
- `make check` passed on the initial branch base; updating to the newest main before publication and verifying the combined result.
- `git diff --check` and the guide test passed.
- No fresh-server access was provided; package managers and toolchain downloads are stubbed in tests. Verified devdb is public and exposes the expected CLI in its package metadata.
