# Tell the leader from the board's details dialog

Status: built and installed (2026-11-16, approved by the user); live check pending

## Why

The user reads a step's details in the board's dialog: LEADER REVIEWING,
"leader hasn't reviewed …" and failed steps under NEEDS YOU. Today, to act on
one, they have to switch to the leader's pane and retype the context. They
want to write the instruction right in that dialog (2026-11-16).

## How it looks

```
╭─ B1-1-AT ───────────────────────────────────────────╮
│ B1-1-AT Registry tests: blocked                      │
│ Attempts: 6 …                                        │
│                                                      │
│ To the leader › accept it if the checkpoint passed▏  │
│ Enter send · Esc cancel                              │
╰──────────────────────────────────────────────────────╯
```
- In a step's dialog, or the leader row's dialog, the hint line gains
  `m message the leader`.
- **m** opens a one-line input at the bottom of the dialog. Typing
  goes into it (q, j/k, Space and so on are just text), Backspace deletes,
  **Enter** sends, and **Esc** cancels and keeps the dialog open. A long text
  scrolls so its end and the cursor stay visible.
- After sending, the hint says `Sent to the leader ✓` for a few seconds and the
  dialog stays open.

## Changes
1. **`leader::tell(task, step, text)`**, next to the leader's other functions:
   - It queues a leader message. About a step:
     `The user, about step B1-1-AT (blocked · 6 attempts): <text>`.
     From the leader row: `The user says: <text>`.
   - It logs `the user told the leader about B1-1-AT`.
   - It sets `leader_wait` back to `No`: the user has answered, so the
     auto-nudge may wake the leader. In a legacy team the dispatcher types
     the message in, as it does every leader message.
   - It refuses empty text.
2. **Board view (`board_view.rs`):**
   - `draw_dialog` takes an optional input: the prompt, the text, and the
     cursor. It draws the input above the hint, and the hint changes to
     `Enter send · Esc cancel`.
   - The plain hint adds `m message the leader` when the dialog can carry
     a message.
3. **Board keys (`board_tui.rs`):**
   - `Interaction` gets a `compose: Option<String>`. **m** in a step's or
     the leader's dialog opens it.
   - While it's open, keys edit it: chars, Backspace, Enter → `Action::TellLeader
     { step, text }`, Esc closes it.
   - Pasted text arrives as many chars and works the same way.
   - The handler calls `leader::tell` and shows the confirmation, or the error
     in the board's problem line.
4. **Help text** (`HELP`), and a sentence in `agentic/docs/usage.md`.

## Tests first
- `leader::tell` queues the message with the step's status and attempts,
  logs it, clears `leader_wait`, and refuses empty text.
- Keys: m in a step dialog opens the input. In the input, `q` and `j` become
  text, and Backspace deletes. Enter returns `TellLeader` with the step and
  closes the input. Esc closes only the input. m on an agent's dialog
  does nothing.
- View: the input line and the `Enter send · Esc cancel` hint are drawn. A long
  text shows its end.

## Verification
- `cargo test`, `cargo clippy --all-targets`, install, and restart the
  feat-system-observer board.
- Live: open B1-1-AT's details, press m, type, and press Enter. The leader
  gets nudged and its inbox has the message, with the step's context.
