# Spec: Team panel shows interrupted agents and leader reviews correctly

- **Date:** 12-11-2026
- **Branch:** fix/team-panel-states (to be created from `main`)
- **Status:** Executed

## Context

The agent team's info panel (`devagentics board`) and the devws sidebars show
each agent's state from the pane option `@devws_pane_state`, which only the
agents' hooks change (`config/tmuxinator/agent_state.sh`). Two wrong displays
came up on task `feat-system-observer`:

1. **Interrupted turns stay "working".** On 11-11 the task was paused
   (log: `pause` at 1794452146) and the turns were interrupted by hand. Codex
   wrote `{"type":"turn_aborted","reason":"interrupted"}` to hammer-7's rollout
   at 1794452187, but neither Codex nor Claude Code calls the `Stop` hook on an
   interrupt, so the leader and hammer kept showing `⠋ working` until their
   next turn. Claude Code records an interrupt in its transcript as a user
   message `[Request interrupted by user]`. Every agent pane already knows its
   transcript (`@devws_transcript`).
2. **A leader review shows as "needs you".** The board's NEEDS YOU card lists
   every step that is reported, failed, blocked or rejected
   (`agentic/src/board.rs:237`). Those steps are the leader's to review
   (`accept_step`/`reject_step`), not the user's. Step `ph` was reported on
   11-11 and later plans superseded it, but nobody ever accepted or rejected
   it, so the board says `● 1 need you` while `devagentics task status`
   reports `waiting_for_you: 0`.

## Requirements

1. An agent whose turn was interrupted leaves `working` within about 5 s of
   the interrupt (~10 s: two sweeps must agree), for both Claude Code and Codex, and shows `idle` (an
   interrupted turn is not finished work, so no "done" notification).
2. Detection reads only the end of the agent's own transcript. It never scans
   whole transcripts or other agents' panes, and a new prompt after the
   interrupt keeps the agent `working`.
3. A step waiting for the leader's review shows in a **LEADER REVIEWING** card
   on the overview, not under NEEDS YOU.
4. Such a step moves to NEEDS YOU only when the leader has left it: reported
   for more than 10 minutes while the leader is not working. It reads as
   `leader hasn't reviewed <step> · <age>`, so the user knows to tell the
   leader, not to review it themselves.
5. Questions, a dispatcher problem, and agents waiting or gone stay under
   NEEDS YOU as today.

## Constraints

- No change to how hooks set states, and no change to pause semantics: pause
  still stops delivery only, it does not interrupt running turns.
- Reuse the header ticker's existing loop; no new background process.
- Old `state.json` files load unchanged: a step without a report time counts
  as reported long ago.
- Scope is the board and the state of working agents. The sidebar's freeze
  behavior (frozen workspaces redraw when shown) is unchanged.

## Files involved

| File | Expected change |
| --- | --- |
| `config/tmuxinator/agent_interrupt.sh` | **New**: sweep working panes, detect an interrupt at the end of their transcript, mark them idle |
| `config/tmuxinator/header_ticker.sh` | Run the sweep every 5 s with the system source |
| `scripts/test_agent_interrupt.sh` | **New**: Claude and Codex fixtures, new-prompt-after-interrupt, non-working panes untouched |
| `scripts/check.sh`, `install.sh` | Register the new test and link the new script, if they list scripts explicitly |
| `agentic/src/state.rs` | `Step.review_since: Option<u64>` (serde default), `Step::set_status` |
| `agentic/src/orders.rs`, `team.rs` | Change status through `set_status`, which stamps `review_since` |
| `agentic/src/board.rs` | Split review steps into leader-reviewing and stale (needs you), using the leader's state |
| `agentic/src/board_view.rs`, `board_view_tests.rs` | LEADER REVIEWING card; stale-review entry under NEEDS YOU |
| `agentic/docs/usage.md`, `docs/guide/agentic.md` | Describe the card, the stale rule, and interrupt detection |

## Success criteria

- [ ] Interrupting a Claude Code agent and a Codex agent mid-turn turns each to idle within ~10 s on the board and the sidebar. *(Covered by tests and real transcript cut points; not yet tried on a live agent.)*
- [x] A prompt sent after an interrupt shows `working` again and is not reset by the sweep.
- [x] A freshly reported step appears under LEADER REVIEWING; NEEDS YOU stays empty.
- [x] With the leader idle, a step reported more than 10 minutes ago appears under NEEDS YOU as `leader hasn't reviewed …`.
- [x] Old state files without `review_since` load, and their reported steps count as stale.
- [x] `make check` and `git diff --check` pass (except `test_devws_menu`, which fails the same way on `main`); the live board for `feat-system-observer` is inspected after reinstalling.
