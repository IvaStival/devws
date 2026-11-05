# Spec: Move devAgentics into devws as `agentic/`

- **Date:** 04-11-2026
- **Branch:** feat/agentic
- **Status:** Specified

## Context

Agent teams needed two repos: devws (panes, agent states) and devAgentics
(Rust: roles, tasks, orders, MCP, board). One repo means one install, one
check, one set of docs, and no "devagentics is not installed" for a user who
has devws.

## Requirements

1. The devAgentics crate lives in `agentic/` and still builds `devagentics`.
2. `./install.sh` builds it when `cargo` exists; without cargo it warns and
   devws works as before.
3. `./delete.sh` removes `devagentics` only if install put it there.
4. `scripts/check.sh` runs fmt, clippy and the tests of `agentic/` when cargo
   exists; the real devws + real devagentics cross test runs as a suite.
5. The guide and READMEs point into the repo, not to the old GitHub repo.

## Constraints

- No code changes inside the crate; `cd agentic && cargo test` still works.
- Binary name, `DEVWS_AGENTIC_COMMAND` and `~/.devagentics` are unchanged.
- Archiving the old GitHub repo is outward-facing: only on the user's OK.

## Files Involved

| File | Action |
| ---- | ------ |
| `agentic/` | Create — the crate, its docs and its specs |
| `install.sh`, `delete.sh` | Modify — build / remove `devagentics` |
| `config/tmuxinator/agentic.sh` | Modify — missing-program message |
| `scripts/check.sh`, `scripts/test_agentic_cross.sh` | Modify/Create — Rust checks, cross test |
| `scripts/test_install_lifecycle.sh` | Modify — owned, preexisting, no cargo |
| `docs/guide/*.md`, `README.md` | Modify — links |

## Success Criteria

- [x] Lifecycle tests cover owned, preexisting and no-cargo installs.
- [x] Cross test passes against `agentic/target/debug`.
- [x] `scripts/check.sh` passes in full.
