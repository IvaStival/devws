# Plan: MCP effect permissions and role ceilings

- **Date:** 12-11-2026
- **Branch:** `feat/devagentics-reliable-execution`
- **Status:** Executed — 12-11-2026
- **Specification:** [spec.md](spec.md), approved 12-11-2026

## Context

Add effect labels and one-use consent for external/destructive MCP actions while exposing existing role controls in the task status and board. Preserve plan approval and step review as separate gates.

## Step 1 — Persist and enforce per-call permissions

**Status:** Completed

**Files:** `agentic/src/state.rs`, `agentic/src/mcp.rs`

**Actions:**

- Add defaulted permission state for older tasks.
- Label each MCP tool with its effect tier.
- Gate external and destructive tools before dispatch; bind approval to exact arguments, require explicit authorization and consume permission before execution.
- Record denial and reject changed calls and replays.

**Verification:** MCP tests cover classification, no-action request, explicit approval, changed arguments, denied destructive actions and one-use replay.

## Step 2 — Surface role permissions and document the workflow

**Status:** Completed

**Files:** `agentic/src/status.rs`, `agentic/src/board.rs`, `agentic/src/board_view.rs`, `agentic/roles/leader.md`, `docs/architecture/devagentics-execution.md`, `docs/user/devagentics-execution.md`

**Actions:**

- Report role group, repository ceiling, sandbox and provider approval mode, accounting for the leader's active fallback.
- Show the permission summary with role details in the team board.
- Tell the leader to obtain consent for the exact call and keep this separate from plan/step approval.
- Document the permission workflow and state transition.

**Verification:** Status and board tests assert effective role controls; role instruction tests include the authorization tool.

## Step 3 — Verify behavior end to end

**Status:** Completed

**Files:** `agentic/src/mcp.rs`, `agentic/tests/flow.rs`

**Actions:**

- Test exact one-use authorization and denial.
- Update flow fixtures to explicitly approve gated calls.
- Run the full test suite, formatter, Clippy and locked build.

**Verification:** `cargo test --locked`: 160 unit and 6 integration tests passed; `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo build --locked` and `git diff --check` passed. The process-group timeout test required execution outside the sandbox.
