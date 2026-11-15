# Spec: MCP effect permissions and role ceilings

- **Date:** 12-11-2026
- **Branch:** `feat/devagentics-reliable-execution`
- **Status:** Executed

## Context

Use UnniDesktop's system/external/destructive effect labels to gate risky leader MCP actions, and make the team's existing role, repository, sandbox and provider approval controls visible in status and the board.

## Requirements

1. Classify all leader MCP tools as system, external or destructive in their catalog descriptions.
2. Require explicit, per-invocation approval before external or destructive calls execute.
3. Bind approval to the exact tool arguments, consume it once and preserve plan approval and step review as separate controls.
4. Show each role's effective repository permission ceiling, sandbox and provider approval mode in task status and team role details.
5. Load old task state without permission records and document the new flow.

## Constraints

- Derive role ceilings from existing role/group/order settings; add no parallel role policy field.
- Do not gate task-local system operations.
- Do not change existing plan approval, order delivery or step review semantics.

## Files Involved

| File | Action |
| --- | --- |
| `agentic/src/state.rs` | Add compatible permission request state and effect/status types |
| `agentic/src/mcp.rs` | Classify tools, gate calls and authorize exact one-use retries |
| `agentic/src/status.rs` | Report pending requests and effective role settings |
| `agentic/src/board.rs`, `board_view.rs` | Show role permission summaries in team details |
| `agentic/roles/leader.md` | Explain consent and retry behavior |
| `agentic/tests/flow.rs` | Model explicit approval in end-to-end MCP scenarios |
| `docs/architecture/devagentics-execution.md`, `docs/user/devagentics-execution.md` | Document architecture and user behavior |

## Success Criteria

- [x] External/destructive calls do not run before an explicit authorization record.
- [x] Changed arguments, denial and replay cannot use a prior approval.
- [x] Status and board expose effective role controls.
- [x] Existing task state loads with no permission records.
- [x] Rust tests, lint, formatting and build pass.
