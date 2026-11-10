# Spec: A tech-style board with the team's consumption

- **Date:** 10-11-2026
- **Branch:** feat/agentic
- **Status:** Executed
- **Follows:** [board redesign](../2026-11-09-162503_board-redesign/spec.md)

## Context

The redesigned board (10-11-2026) is fast and scrolls, but the user finds it
too plain: rows sit against each other, and it says little about what each
agent and group does and costs. They want a more modern, "tech" look, and per
agent and per group: tokens, cost, and other useful figures.

The board already reads each running agent's usage (input, output, cache
tokens, cost at API prices), but sums it by role and shows it only in the Usage
view. Orders link agents to steps, so accepted steps and retries per agent can
be counted. No per-agent time is stored, so none is shown.

The header's limit bars use `▓░`. Ghostty draws those as flat tints; a font
setting that brings back iTerm2's dotted texture applies only to windows opened
after it, and the user's window dates from 09-11 09:20, before it.

## Requirements

1. The Overview, at 84 columns:

   ```text
   ╭─ feat-system-observer · feat/system-observer ──────────── ● 5 need you ─╮
   │  overview  agents  steps  activity  usage                               │
   │  PROGRESS  ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣀  126/130 · 97%                   │
   │  COST  $31.40      TOKENS  4.6M in · 1.1M out · 474M cache              │
   │  claude  5h ▓▓░░░░░░░░  18%   wk ▓▓▓░░░░░░░  34%                        │
   ╰─────────────────────────────────────────────────────────────────────────╯

    ▌NEEDS YOU · 5
      ? q863  hammer-claude-5 → ui-designer-3: L-F12 blocker: read ack…
      ⚠ D1    Build the valid feed catch-up hook reproduction        blocked

    ▌CTO                                                  $12.10 · 2.1M tok
      L  leader            claude · high       ● needs you    $12.10   2.1M

    ▌SENIORS · 1                                           $8.30 · 1.2M tok
      1  ui-designer-3     gpt-6.1-sol         ✓ done          $8.30   1.2M
         └ 14 steps planned

    ▌FLOOR · 2                                            $11.00 · 1.3M tok
      2  hammer-claude-5   sonnet · front      ⠋ working       $6.20   0.9M
         └ now L-F12 Integrate the queue workspace…   31 accepted · 4 retries
      3  hammer-codex-8    gpt-5.6-luna · ops  ✓ done          $4.80   0.4M
         └ 22 accepted

    ▌DONE · 126                                       newest first · ×n retries
      ✓ L-F11  Captured-message panel: timeline, body, headers…
   ↑↓ select · ⏎ details · o go · v peek · ⇥ views · ? help
   ```

   - A rounded frame around the header; its title line carries task, branch
     and the attention count.
   - A team line: total cost (`+` when a model has no price) and total input,
     output and cache tokens.
   - Sections have an accent bar (`▌`), a count, a blank line before each, and
     totals (cost, tokens) on group headings.
   - Each agent row: number, id, program (model, lane), state, cost and tokens
     at fixed columns; a dim second line (`└`) with its current step, accepted
     steps and retries. Groups follow the team: CTO, Seniors, Floor.
2. Usage is kept per agent as well as per role; a stopped agent keeps its last
   reading, as now.
3. Bars drawn by the board (progress) use braille cells, `⣿` filled and `⣀`
   free, which every terminal draws as dots: a texture without font settings.
4. Limit bars stay `▓░`, the same cells as the devws header, so the two match.
   Their texture comes from the Ghostty font setting, checked in a newly opened
   window; if that window still shows flat tints, the devws header and the
   board switch their limit bars to the braille cells too (a separate change).
5. Narrow panes (under 60 columns) drop the tokens column, then the program,
   before the state; nothing overflows its row. Agents and Usage views keep
   their content under the new frame.

## Constraints

- No new readings: only the usage the board already reads, per agent.
- `board --once` prints the same layout as text.
- Keys and scrolling stay as they are.

## Files Involved

| File | Action |
| ---- | ------ |
| `agentic/src/board_usage.rs` | Modify — keep each agent's usage beside the role sums |
| `agentic/src/board.rs` | Modify — per-agent usage, accepted steps and retries; group totals |
| `agentic/src/board_view.rs` | Modify — frame, sections, agent rows with second lines, braille progress |
| `agentic/src/board_view_tests.rs` | Modify — layout, totals, narrow widths |
| `agentic/tests/flow.rs`, `scripts/test_agentic_cross.sh` | Modify — expectations of the board text |
| `agentic/docs/usage.md` | Modify — describe it |

## Success Criteria

- [ ] At 84×40 the Overview renders the mockup's parts; group totals equal the
      sum of their agents; the team total equals the sum of all.
- [ ] An agent's second line shows its current step, accepted count and retries.
- [ ] At 40 columns nothing overflows and state stays visible.
- [ ] `make check` passes; the restarted live board shows it.
