# Spec: The leader switches between its programs on request and near the 5-hour limit

- **Date:** 08-11-2026
- **Branch:** feat/agentic
- **Status:** Executed

## Context

The leader-fallback feature (05-11-2026) moves the leader from its role's
program (Claude) to its fallback (Codex Sol) only when Claude has used 95% of
its **week**, and never moves back on its own.

In use, the leader ended up on Codex, and Codex's **5-hour window** was almost
gone. The user told the leader to change to Claude. It could not: it has no
tool for it, and answered that it "can't transfer this conversation's
leadership to a Claude model from here", so the task stayed paused.

The user wants:

1. The leader switches program when the user tells it to.
2. A rule: when the leading account reaches 95% of its 5-hour window, the
   leader moves to the other program if that one has more room left.

## Requirements

1. **Two programs, both ways.** The leader's role program (`agent`) and its
   `[fallback]` are its two programs. Whichever leads now, "the other" is the
   one that does not. Moves go both ways (Claude → Codex and Codex → Claude);
   the "never back on its own" rule goes.
2. **Run out** now means 95% or more of the **5-hour window or of the week**
   (either). The same rule is used at start and while running.
3. **Automatic move.** While the task runs, when the leading account has run
   out and the other has not, the leader is handed over to the other once its
   turn is over (not while it works). When both have run out, it stays and
   nothing is moved.
4. **At start** (`devws agentic`): the leader starts on the role's program,
   or on the other when the role's has run out and the other has not (as
   today, with the 5-hour window now counted too).
5. **On request.** A new leader tool, `switch_leader` (optional `reason`),
   asks for the move to the other program. It is refused, with a message,
   when there is no other program (no `[fallback]`) or when the other has run
   out (with its percentages). Accepted: it is recorded in the task's state,
   and the leader is told to tell the user in one line and end its turn. The
   board performs it as soon as the leader's turn is over (checked every
   dispatch tick, not only every minute), whatever the limits.
6. **The handover** keeps today's mechanics: the leader's pane restarts on the
   other program with its tools and is told to catch up from `task_status` and
   `read_log`. The message says why: run out (which window, how much) or
   asked by the user. The move is logged, `leader_agent` updated, the board
   shows which program leads, and a pending request is cleared.
7. **From the devws menu.** In an agent team's workspace (`@devws_task`
   set), the agent tab has "⇄ Switch leader model". It runs
   `devagentics leader switch <task>`, which makes the same request as
   `switch_leader` (same refusals) and says where the leader will move; the
   board performs it once the leader's turn is over. Hidden in other
   workspaces.
8. **The leader knows.** `roles/leader.md` tells it: when the user asks to
   move to another model/program, call `switch_leader`; never claim it cannot.
   `task_status` shows which program leads and whether the other is available
   (its limits).

## Constraints

- No flapping: an automatic move happens only when the current account has
  run out and the other has not, so the new leader never immediately qualifies
  to move back.
- The other roles' limit rule (no new agent past 85% of the 5-hour window) is
  unchanged.
- Still only the leader has a fallback; role files without one keep working
  (no switching, the tool says so).
- No switching to arbitrary agents/models: only between the two programs the
  leader's role defines.
- The conversation is not carried over (as today).

## Files Involved

| File | Action |
| ---- | ------ |
| `agentic/src/leader.rs` | Modify — run out by 5h or week; "the other program"; both ways; requested switch; `request_switch` |
| `agentic/src/state.rs` | Modify — `leader_switch` (pending request with reason) |
| `agentic/src/mcp.rs` | Modify — `switch_leader` tool |
| `agentic/src/main.rs` | Modify — `devagentics leader switch <task>` |
| `config/tmuxinator/devws_menu.sh` | Modify — "Switch leader model" (when `agentic`) |
| `config/tmuxinator/workspace_actions.sh` | Modify — `switch_leader` action |
| `scripts/test_devws_menu.sh` | Modify — the entry shows only in a task's workspace and runs the command |
| `agentic/src/board_tui.rs` | Modify — check a pending request every tick |
| `agentic/src/status.rs` | Modify — leader program and the other's availability in `task_status` |
| `agentic/src/board.rs` | Modify — fallback label when the leader is on the other program (if needed) |
| `agentic/roles/leader.md` | Modify — comment and instructions for `switch_leader` |
| `agentic/docs/usage.md`, `docs/guide/agentic.md` | Modify — docs |

## Success Criteria

- [x] Unit tests: run out at 95% of the 5-hour window (not 94%) and of the
      week; start picks the other on a 5-hour run-out; a running leader moves
      Codex → Claude when Codex's 5-hour window hits 95% and Claude's has room;
      stays when both are out; does not move while working; no immediate move
      back.
- [x] Unit tests: `switch_leader` records a request, refused without a
      fallback or when the other is out; the board performs it once the turn
      is over even with room left, clears it, logs it with the reason.
- [x] Menu test: the entry is listed only in a workspace with `@devws_task`
      and calls `devagentics leader switch <task>`.
- [x] MCP test: `switch_leader` listed and callable.
- [x] `scripts/check.sh` passes (cargo test, clippy, shell tests).
- [ ] Live: telling the running leader "switch to Claude" restarts its pane on
      Claude, which catches up and says it took over.
