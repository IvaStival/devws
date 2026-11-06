# Spec: The leader moves to another model when its account runs out

- **Date:** 05-11-2026
- **Branch:** feat/agentic
- **Status:** Executed

## Context

The leader (the CTO) runs on Claude. When the Claude account is near its
weekly limit, the team stops: the leader cannot answer or give orders. Codex
had room (Claude 87% of its week, Codex 14%). The user wants the leader to
move to Codex Sol, medium effort, in that case.

## Requirements

1. The leader's role can name a **fallback**: another agent, model and
   effort. The built-in leader's is Codex `gpt-6.1-sol`, effort medium.
2. An account counts as running out when it has used **95% or more of its
   week** (the 5-hour window does not count for this).
3. **At start** (`devws agentic`): when the leader's account is out and the
   fallback's is not, the leader starts on the fallback.
4. **While running**: when the leader's account runs out, the leader is
   handed over to the fallback. This happens once the leader has finished its
   turn, not while it works. The leader's pane restarts on the fallback and
   is told it takes over: it calls `task_status` and reads the log to catch
   up, then tells the user in one line. Its old conversation is not carried
   over.
5. The handover is logged and shown on the board. It does not switch back
   on its own; the next `devws agentic` start chooses again.
6. Only the leader has a fallback: a `fallback` in another role's file is
   refused, with a message saying so.

## Constraints

- The other roles' limit rule (no new agent past 85% of the 5-hour window)
  is unchanged.
- No handover when the fallback's account is out too; the board says so.
- The user's own role files keep working (the fallback is optional).

## Files Involved

| File | Action |
| ---- | ------ |
| `agentic/src/roles.rs` | Modify — `fallback` front matter (leader only) |
| `agentic/roles/leader.md` | Modify — Codex Sol medium fallback |
| `agentic/src/start.rs` | Modify — the leader's program chosen by limits |
| `agentic/src/leader.rs` | Create — "is it out", handover |
| `agentic/src/watch.rs` | Modify — check now and then, hand over when idle |
| `agentic/src/devws.rs` | Modify — `run` (devws agent run) in the trait |
| `agentic/src/state.rs`, `log.rs`, `board*.rs` | Modify — which program leads, the event |
| `agentic/docs/usage.md`, `docs/guide/agentic.md` | Modify — docs |

## Success Criteria

- [ ] Unit tests: the fallback parsed (leader only); start picks the
      fallback at 95% of the week, not at 94% or when the fallback is out
      too; a running leader is handed over only when not working, once.
- [ ] `scripts/check.sh` passes.
- [ ] Live: with Claude's week at 95% or more, `devws agentic` starts the
      leader on Codex Sol, and it has the leader's tools.
