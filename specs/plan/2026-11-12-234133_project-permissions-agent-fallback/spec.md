# Spec: Project-scoped autonomy and agent model fallback

- **Date:** 12-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed — one existing verification blocked by sandbox

## Context

Starting agents is now a system action, but stopping agents still requires destructive-action approval even within the task. Worker startup refuses a Claude account at its usage ceiling; only the leader currently supports fallback, and role parsing explicitly prohibits it for workers.

## Requirements

1. The leader performs actions confined to the current project without separate per-action approval, including starting, stopping and replacing task-owned agents. Explicit plan approval remains part of the existing planning workflow.
2. Ask before actions that cross the current project's boundary, including adding an unrelated repository or publishing changes to a remote service. Classify effects by the actual target, rather than treating every stop or replacement as external or destructive.
3. Use available alternative providers/models automatically when a requested worker provider/model is unavailable. Claude exhaustion must cause startup to try a Codex equivalent without asking the user.
4. Preserve the worker's role instructions, group, repository, lane, write ceiling and assigned work when selecting an alternative. Use provider-compatible model, effort, sandbox and approval settings.
5. Support ordered, bounded candidate selection: preferred configuration first, then compatible alternatives. Skip candidates at the existing 85% week or five-hour ceiling; recognize explicit provider/model unavailability separately from unrelated launch errors. Never retry indefinitely or duplicate a live worker.
6. Record the selected provider/model and fallback reason in task status and logs so the leader can explain what happened. If all candidates are unavailable, return their reasons without creating a phantom live worker.
7. Update the leader instructions, tool descriptions and user/architecture documentation to match the implemented behavior.

## Constraints

- Do not bypass project boundaries, plan approval, agent capacity, lane isolation or write ceilings.
- Do not interpret arbitrary startup errors as token exhaustion or silently discard them.
- Keep existing leader fallback behavior compatible.
- Preserve unrelated in-progress changes in this working tree.

## Files involved

| File | Action |
| --- | --- |
| `agentic/src/mcp.rs` | Refine permission classification and regression tests |
| `agentic/src/roles.rs` | Extend compatible fallback configuration to workers |
| `agentic/src/team.rs` | Select available candidates and handle launch fallback |
| `agentic/src/devws.rs` | Inspect/extend typed availability evidence as needed |
| `agentic/src/status.rs` | Expose actual selected model and fallback reason |
| `agentic/src/state.rs` | Persist selection evidence if needed |
| `agentic/roles/leader.md` | Explain autonomy, boundaries and automatic alternatives |
| `agentic/roles/samples/` | Supply compatible worker defaults |
| `agentic/tests/flow.rs` | Cover end-to-end worker fallback |
| `agentic/docs/usage.md`, `docs/architecture/devagentics-execution.md` | Document final behavior |

## Success criteria

- [x] Starting, stopping and replacing task-owned workers requires no extra permission.
- [x] Calls affecting another project or a remote service remain permission-gated.
- [x] Claude exhaustion starts the same worker role on available Codex automatically.
- [x] Unavailable model/provider candidates are tried once in order; unrelated errors remain actionable.
- [x] Exhausted alternatives leave no live reservation or duplicate worker.
- [x] Existing plan, capacity, write-scope and leader-fallback tests pass.
- [ ] Relevant Rust tests, formatting, Clippy and build checks pass.

## Verification result

Implementation completed. All 185 runnable unit tests and 7 integration tests
passed, as did 10 hook/runner tests, the agent CLI shell suite, formatting,
Clippy, build, documentation links and diff whitespace checks.

The existing `check_profiles::tests::timeout_terminates_the_shell_and_its_background_child`
test is still blocked: the sandbox denies its `ps` subprocess with `Operation
not permitted`. Full-suite escalation attempts were interrupted by daemon
restarts. The remaining suite passed with only this test explicitly filtered;
no test was disabled or altered to hide the restriction.
