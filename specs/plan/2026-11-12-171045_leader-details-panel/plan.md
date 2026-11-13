# Plan: Enrich the Leader Details Panel

- **Date:** 12-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed
- **Origin:** Proposed by Codex in the "Improve devagentics commands" session (2026-11-12); saved at the user's request.

## Summary

Expand the existing leader row in the team board’s Overview. Pressing Enter on the row will show the leader’s live status, effective role permissions, task state, and pending permission requests. Pending requests will also appear in “Needs You” so they are visible without opening the details.

## Changes

- Build leader details from the active leader role, including fallback settings, pane state, repository access ceiling, sandbox, and provider approval mode.
- Include whether the plan is approved and the task is paused, plus a concise summary of pending permission requests.
- Add each pending request to “Needs You,” showing its request ID, tool, effect level, and exact arguments so the user can review the call.
- Keep the current compact leader row and its navigation behavior; this adds information to its details and the existing attention list.

## Tests

- Verify leader details show effective permissions and active fallback settings.
- Verify task approval and pause state appear correctly.
- Verify pending requests appear in both leader details and “Needs You,” with enough information to identify the exact action.
- Verify no pending requests leave the current Overview behavior unchanged.

## Assumptions

- “Leader panel” means the leader row in the existing team board’s Overview; Enter already opens its details.
- Pending approval details should show the exact invocation because the user needs to review the specific call before approving it.
- This is a board presentation change; permission enforcement and MCP approval flow remain as implemented.

## Likely files

`agentic/src/board.rs`, `agentic/src/board_view.rs`, `agentic/src/board_view_tests.rs`, `agentic/src/roles.rs`.
