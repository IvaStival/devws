# Spec: Safe devAgentics communication

- **Date:** 14-11-2026
- **Branch:** feat/devagentics-reliable-execution
- **Status:** Specified — awaiting approval

## Context

The dispatcher delivers queued team messages through `Devws::send`, which invokes `devws agent send`. `agent_cli.sh` pastes or types the message into the pane and presses Enter. Its idle/done state check does not establish that the user's editable prompt is empty. A user's unsent draft can therefore merge with an injected message and be submitted. Pane capture is also a weak substitute for the structured task status, logs, orders and questions already available through devAgentics.

## Requirements

1. Use devAgentics as the authoritative communication channel: durable, addressed messages with IDs, sender/recipient identities, task/order context and delivery acknowledgements.
2. Let leaders and workers inspect status, logs, current assignments and their own inbox through supported tools/CLI. Scope worker access to its task and role; do not expose leader mutation tools to workers.
3. Deliver orders, answers, questions, reports and recovery notices through this channel. Reading is non-destructive; acknowledgement is explicit and idempotent. Preserve messages across restarts and do not mark orders delivered merely because text was read.
4. Team delivery must never paste/type text or press Enter in an interactive agent prompt, including leader notifications. Preserve drafts exactly; do not clear, stash or submit them automatically.
5. Provide a supported activation mechanism for agents that are idle: inspect Claude/Codex integration capabilities and select a hook, controlled worker turn/session mechanism, or equivalent that does not write into an editable prompt. Where unavailable, retain pending messages and show a board notification/manual fetch action. Do not silently revert to terminal injection.
6. On startup, continuation and after a turn, agents check authoritative task context/inbox using the integration chosen during planning. Busy agents receive messages at safe boundaries and retain current order semantics.
7. Role guidance must use devAgentics status/log/inbox tools for team coordination. Pane inspection remains a diagnostic action when structured information is insufficient; prohibit routine capture-based coordination.
8. Preserve pause/resume, lane and writer ownership, checks, report identity, permission boundaries, startup fallback and queue recovery. Old task queues load without message loss or automatic injection.
9. Give the user visibility into pending, read, acknowledged and blocked delivery, including an actionable explanation when an idle agent cannot be safely activated.

## Constraints and decisions

- No dependency on parsing screenshots or rendered prompt text to decide that injection is safe.
- No blind terminal clearing, simulated keystrokes or prompt rewriting as a workaround.
- Do not claim exactly-once execution from message acknowledgement; use durable identity and existing order invariants to prevent duplicate effects.
- Inspect actual installed provider capabilities before choosing the wake-up transport. Planning must resolve this design question before implementation.
- The separately approved usage-limit override remains a separate change.

## Files involved

| Area | Expected change |
| --- | --- |
| `agentic/src/state.rs`, `task.rs` | Durable inbox envelope and acknowledgement state, compatibility |
| `agentic/src/watch.rs`, `devws.rs` | Replace terminal delivery with safe message/activation transport |
| `agentic/src/mcp.rs`, `main.rs` | Scoped inbox/status/log and acknowledgement interfaces |
| `agentic/src/start.rs`, `agent_args.rs`, `roles/` | Worker integration and communication guidance |
| `agentic/src/orders.rs`, `questions.rs`, `team.rs` | Integrate assignment/question/recovery delivery semantics |
| `agentic/src/status.rs`, `board*.rs` | Pending/acknowledged/activation-blocked visibility |
| `config/tmuxinator/agent_cli.sh` | Keep manual send explicit; stop using it for automatic team delivery |
| `scripts/test_agent_cli.sh`, Rust tests | Draft preservation, delivery, restart and recovery regression coverage |
| `docs/user/devagentics-execution.md`, `docs/architecture/devagentics-execution.md` | Workflow and transport documentation |

## Success criteria

- [ ] A queued team message arriving while the user has an unsent draft leaves the draft untouched and never submits it.
- [ ] Both Claude and Codex can receive and acknowledge messages using supported devAgentics interfaces.
- [ ] Idle agents activate safely where supported; otherwise pending work is clearly visible and manually retrievable.
- [ ] Concurrent messages, repeated reads/acknowledgements and restarts do not lose messages or duplicate orders.
- [ ] Worker identity cannot read other tasks or invoke leader-only actions.
- [ ] Pause/resume, questions/answers, startup readiness and order recovery still work.
- [ ] Tests verify no automatic team path invokes terminal send/paste/Enter.
- [ ] Rust tests, formatting, Clippy, build and relevant shell regression tests pass.
