# Spec for DevAgentics token optimization

- **Date** 21-11-2026
- **Branch** `feat/opencode-workers` at `1b9e06f` plus local changes
- **Status** Compact MCP measured on Notes. Read-only Leader CLI pilot implemented for opted-in reviewed Codex tasks.

Reduce the total provider cost per successfully integrated development task while preserving delivery quality, autonomy, authorization, isolation, and recovery. The implementation plan covers the current MCP setup, a Leader CLI hybrid, and a CLI-first candidate with both roles using CLI.

1. Measure provider usage and local payload sizes by role, operation, and turn before choosing a transport.
2. Compare equivalent workloads and retain complete, authorized access to diagnostics.
3. Route new interfaces through existing service and permission checks. Keep agents behind their existing provider restrictions, Docker execution, Git checkpoints, and review and test gates.
4. Preserve tmux notifications, transactional file-based coordination, provider session resumption, and ai-memory. Evaluate narrower context and memory reads before changing session lifecycles.
5. Make trials independently reversible per task and provider. Rollout requires an improvement in cost per integrated task and passing reliability and security gates.

The implementation plan gives source-level audit findings, proposed contracts, acceptance gates, and a benchmark protocol. Implementation proceeds in measurement order, with transport changes gated by baseline results.
