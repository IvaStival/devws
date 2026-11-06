# Plan: Agent groups

- **Date:** 04-11-2026
- **Branch:** feat/agentic
- **Status:** Executed (uncommitted)

## Steps

1. Role `group`, defaults and validation — `agentic/src/roles.rs`.
2. Execution plans — `agentic/src/plans.rs`, `State.plans`, `plan show`, `read_plan`.
3. Floor orders need an accepted plan; seniors forbid `**` — `agentic/src/orders.rs`.
4. Questions — `agentic/src/questions.rs`, `ask`/`answer`, `answer_question`, no stall in `watch.rs`.
5. Board groups and Questions box — `board.rs`, `board_view.rs`; status JSON.
6. Prompts: `roles/leader.md`, samples, the user's `ui-designer`/`hammer-*` files; Codex `--add-dir`; docs.
7. Flow tests through the binary.

## Verification

1. `cd agentic && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
2. `scripts/check.sh`
3. `devagentics board <task> --once --width 73` shows the three groups.
