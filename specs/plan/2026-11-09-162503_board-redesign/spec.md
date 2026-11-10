# Spec: A responsive, modern team board

- **Date:** 09-11-2026
- **Branch:** feat/agentic
- **Status:** Executed

## Context

The team board (`devagentics board`, the right panel of an agent team's
workspace) feels slow to scroll, its completed steps cannot be scrolled, and the
user wants a modern look: bars, and only the information that matters.

Measured on 09-11-2026 with the live `feat-system-observer` team:

- One `devws agent usage <pane>` call takes 1.3–2.2 s; `devws agent limits codex`
  takes 3.9 s. The board's `load` makes these calls (usage every 30 s per agent,
  limits for each account) **on the thread that reads keys and the wheel**, so
  input freezes for several seconds at a time. `board --once` took 14–25 s.
- The input loop also handles one event per redraw, so a fast wheel queues up.
- The panel still runs an older build (boxes, a 99-row "Done" box that cannot
  scroll). The branch's build has tabs (Overview, Agents, Steps, Activity,
  Usage) but is text only, and finished steps appear only in Steps after `c`.

## Requirements

### Responsiveness

1. The board's data (`load`: team state, usage, limits) is read on a background
   thread. The input thread only draws the latest snapshot and handles input;
   it never waits on `devws` or `tmux` calls.
2. Before each draw, every pending input event is handled, so a fast wheel turn
   moves at once rather than one step per frame.
3. A refresh still never moves the selection to another row (stable ids).

### Overview, redesigned

4. The Overview shows only what matters, top to bottom, as below at 84 columns:

   ```text
    feat-system-observer · feat/system-observer             ● 2 need you
    Overview  Agents  Steps  Activity  Usage
    Progress  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░  99/101 steps
    claude    5h ▓▓▓▓░░░░░░  42%   wk ▓▓▓▓▓▓░░░░  63%
    codex     5h ▓░░░░░░░░░   9%   wk ▓▓▓▓▓▓▓▓▓░  87%

    NEEDS YOU
    ▸ ⚠ D1   Build the valid feed catch-up hook reproduction        blocked
      ⚠ D1R  Capture the exact live-feed catch-up stop boundary     blocked

    TEAM
      L  leader           claude · high                         ⠋ working
      1  ui-designer-3    Seniors · gpt-6.1-sol                 ✓ done
      2  hammer-claude-3  Floor · front                         ✓ done
      3  hammer-codex-8   Floor · ops                           ✓ done

    DONE  99
      ✓ P3R.12  Keep the status help at the right edge on narrow…  ×2
      ✓ P3R.11  Give the help popover a real width
      …
   ↑↓ select · Enter details · o go · v peek · Tab views · ? help
   ```

   - Header: task · branch, and the attention count on the right (hidden at 0).
   - A progress bar of accepted steps over all steps.
   - One limit bar row per account (5-hour and week), in the header's level
     colors (green, yellow from 60 %, red from 85 %); the same `▓░` cells as the
     devws header.
   - **Needs you**: unanswered questions, steps waiting for review or blocked,
     agents waiting or gone, the dispatcher's error. Hidden when empty.
   - **Team**: the leader and each running agent: number, id, group and lane or
     model, current order (trimmed), and a state mark (`⠋ working`,
     `● needs you`, `✓ done`, `✕ gone`).
   - **Queued**: one line with the count when orders wait (details in Steps).
   - **Done**: accepted steps, newest first, with the count.
5. The whole Overview is one list: the wheel, `j/k`, `PgUp/PgDn` and `g/G` move
   through all sections, so Done scrolls like the rest. Section headings are not
   selectable; the selection skips them.
6. Removed from the Overview: the summary sentence ("Plan approved. 99 of 101
   steps accepted." — the bar says it), the separate `Leader:` line (it is the
   Team's first row), and the long footer (one short line of keys; `?` has all).
7. Narrow panes (under 60 columns) drop the second limit bar and the lane or
   model column before anything else; nothing overflows its row.

### Other views

8. Agents, Steps, Activity and Usage keep their content; they take the new
   header (title, tabs, progress, limits) and colors. Steps shows a state mark
   per row like Done's.

## Constraints

- No change to what the board reads or to the dispatcher; only where the reading
  runs and how it is drawn.
- `board --once` keeps printing text for scripts (same sections, no colors).
- The bars use `▓` and `░`, as the devws header does, so they look the same in
  every terminal.
- Keys stay as they are; only the footer text shortens.

## Files Involved

| File | Action |
| ---- | ------ |
| `agentic/src/board_tui.rs` | Modify — background loader thread; drain input before drawing |
| `agentic/src/board_view.rs` | Modify — new header, Overview sections, bars, headings skipped by selection |
| `agentic/src/board_view_tests.rs` | Modify — rendering and navigation tests |
| `agentic/src/board.rs` | Modify — only if a field the design needs is missing (e.g. an agent's lane) |
| `agentic/docs/usage.md`, `docs/guide/agentic.md` | Modify — describe the new Overview |

## Success Criteria

- [ ] With a stub `devws` whose usage call sleeps 2 s, keys and wheel events are
      handled within one frame (a test drives the loop without the sleep blocking it).
- [ ] The Overview at 84×40 renders as the mockup's sections; a 101-step task
      shows a Done section the wheel reaches; headings are never selected.
- [ ] At 40 columns no row overflows and the attention count stays visible.
- [ ] `make check` passes; the live board restarted on the new build scrolls
      without stalls.
