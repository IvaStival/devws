# Spec: User-approved worker usage limit override

- **Date:** 14-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Executed

## Context

Worker startup currently rejects accounts using at least 85% of either their weekly or five-hour allowance. Automatic fallback helps while another provider has room, but when both Claude and Codex reach the threshold the leader cannot continue delegating. The user wants the leader to ask whether it may keep using workers despite those percentage limits.

## Requirements

1. Keep the existing 85% worker threshold and automatic fallback when another compatible provider is below it.
2. When both compatible providers are blocked by percentage thresholds, return an actionable approval-required result and expose the affected accounts, windows and percentages to the leader.
3. The leader must explain the situation and ask the user whether workers may continue above the percentage threshold. Do not interpret general task or plan approval as this consent.
4. Record an explicit approval or denial in task state and the task log. Approval permits worker launches above the percentage threshold for the current task only, including deferred startup retries and replacements. It survives leader/dispatcher restarts, does not affect other tasks, and can be revoked.
5. Pending approval or denial leaves worker startup blocked; avoid repeatedly asking after denial unless the user requests reconsideration.
6. Reevaluate previously percentage-blocked candidates after approval so pending workers can actually resume. Preserve worker identity, role, lane and queued work.
7. Continue enforcing real provider quota errors, provider/model unavailability, plan approval, concurrency limits and worktree ownership. The override affects only the local percentage guard.
8. Expose approval state in task status and document the leader workflow and task-scoped override.

## Constraints

- Do not change the leader's existing 95% switching policy.
- Do not raise thresholds globally or grant approval automatically.
- Reuse existing permission/state/log conventions where practical, keeping percentage consent distinct from external-action permission.
- Preserve the existing uncommitted team-history work.
- Follow the project's canonical `specs/plan/` location.

## Files Involved

| File | Action |
| ---- | ------ |
| `agentic/src/worker.rs` | Separate percentage rejection from provider failure and honor task consent |
| `agentic/src/team.rs` | Apply consent to worker launch and cover startup behavior |
| `agentic/src/state.rs` | Persist backward-compatible task consent |
| `agentic/src/mcp.rs` | Expose approval-required responses and explicit consent decisions |
| `agentic/src/status.rs` | Report usage-limit approval state |
| `agentic/src/watch.rs` | Verify deferred retries respect consent |
| `agentic/roles/leader.md` | Require the user prompt and explain consent scope |
| `agentic/docs/usage.md` | Document behavior and scope |

## Success Criteria

- [x] Below-threshold fallback works unchanged and does not request consent.
- [x] Both providers at or above 85% produce an approval request without starting workers.
- [x] Explicit approval enables percentage-blocked workers in this task, including after restart.
- [x] Pending, denied or revoked consent blocks over-threshold launches; other tasks are unaffected.
- [x] Actual provider rejections still trigger bounded fallback or a blocked result.
- [x] Existing staffing and execution safeguards remain enforced.
- [x] Regression tests, Rust formatting, lint and relevant build checks pass.
