# Account switches on the board

Status: built and installed (2026-11-16, approved by the user); live check pending

## Why

The user wants to choose, from the team board, which AI accounts a team may
use, for example only `codex`, with `claude` and `claude_work` off
(2026-11-16). Today the only way is to edit each role's program and
alternatives, or run `task codex-team`.

The user's choices:
- an **ACCOUNTS** card right under the CTO/leader card;
- **toggle pills** (`[━━● on ]` / `[○━━ off]`);
- move with the board's normal keys, and **Space** toggles.

## Design

```
╭─ ACCOUNTS ───────────────────────── space on/off ─╮
│ ▸ [━━● on ]  codex        5h 19%  wk 41%           │
│   [○━━ off]  claude       5h 40%  wk 22%           │
│   [○━━ off]  claude_work  5h  3%  wk 10%           │
╰────────────────────────────────────────────────────╯
```
- On is a green pill and the account name in normal text. Off is a dim pill and a dim name.
- Usage is the account's own 5-hour and weekly use, the same numbers the header bar shows today.
- The setting is **per task** and kept in `State::disabled_accounts`. Each
  team chooses its own. A new task starts with everything on.

## Changes

1. **The account list comes from devws.** A new `devws agent accounts`
   (`agent_cli.sh`) prints each registered name and its kind
   (`codex codex`, `claude claude`, `claude_work claude`) from
   `DEVWS_AGENTS`. On the devAgentics side, `Devws::accounts()`, plus the fake.
2. **The state and its rule:** a small `accounts` module (one job: which accounts a task may use).
   - `set_enabled(task, account, on)` logs the change. It refuses to turn off
     the last enabled account and to change an account devws doesn't have.
   - `is_enabled(state, account)`.
3. **The rule applies everywhere an agent is chosen:**
   - `LaunchState::next_available` (`worker.rs`) skips a disabled account and
     records "<account> is turned off for this task". When every candidate is
     off, the spawn fails with that reason, so the leader can tell the user.
   - The leader: `switch_leader` refuses to move to a disabled account, and
     the leader's handover (`leader::hand_over`) never picks one. A leader
     already running on an account you turn off keeps running. The card shows
     it as `off · leader still on it`, and `task_status` reports it, so the
     leader can move.
   - Agents already running aren't stopped. The setting applies to new agents.
4. **The board:**
   - `Board::accounts` lists every registered account with an `enabled`
     flag, not only those in use.
   - `board_view` draws the ACCOUNTS card under CTO, with each row's id
     `account:<name>`.
   - The leader's Enter details list the accounts and their switches too.
   - Keys (`board_tui.rs`): Space on an `account:` row toggles it
     (`Action::ToggleAccount`). Everywhere else Space still pages down.
   - The help text gets "Space on an account: turn it on/off".
5. **The leader is told:** `task_status` gets `accounts: [{name, kind, enabled}]`.
   Each change sends a line to the leader inbox, "The user turned claude off
   for this task", which the auto-nudge delivers. `leader.md` says to staff
   only enabled accounts.
6. **Docs:** a short paragraph in `agentic/docs/usage.md`, in the board
   section.

## Tests first
- `accounts`: toggling off and on, refusing the last one, refusing an unknown
  name, and the change is logged.
- `next_available` skips a disabled candidate and picks the next. With all
  of them off it fails, giving the reason.
- `switch_leader` to a disabled account is refused.
- Board view: the ACCOUNTS card sits under CTO, shows the pills, dims off
  rows, and shows usage. A leader on a disabled account says so.
- Keys: Space on an account row returns `ToggleAccount`; on another row
  it still pages.
- `devws agent accounts` prints the registry (shell test next to the existing
  `agent_cli` tests).

## Verification
- `cargo test`, `cargo clippy --all-targets`, the shell tests, install, and
  restart the feat-system-observer board.
- Live: the card lists codex, claude and claude_work. Turning claude off
  makes it dim, and the leader's inbox hears about it. A new worker gets
  only enabled accounts.
