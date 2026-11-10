# Plan: Team navigation redesign

- **Date:** 08-11-2026
- **Status:** Executed
- **Approval:** User approved specification and implementation plan in conversation.

## 1. Board data and views — complete

Add structured detail identity, unfinished steps, leader navigation, attention-first overview, selectable views, detail scrolling, adaptive rendering, CLI --view, and existing-pane view requests. Verify model, rendering, interaction and CLI tests.

## 2. Sidebar — complete

Keep team rows to name/model/status, retain rich normal-workspace rows, expand current team tree, persist manual tree choices, preserve cursor identity, show aggregate counts, and search hidden descendants. Verify isolated tmux sidebar suites.

## 3. Menu — complete

Contextual Team tab with board-view navigation, leader navigation, and existing switch. Resolve group parent, focus existing board only. Verify menu suites.

## 4. Documentation and final checks — complete

Update user guide and architecture documentation. Run focused tests, Rust fmt/clippy/tests and make check. Inspect text renders at 85 columns and short/narrow sizes. Record results and limitations here.

## Verification

- Full `make check` passed: Rust formatting/lint/tests and all 38 shell suites. Two outdated presentation assertions were corrected and passed on the check's isolated retries.
- Latest Rust suite: 116 unit tests and 5 integration tests pass; fmt and clippy pass.
- The cross-system test verifies Team menu requests reach the existing board, full details retain order text, and navigation preserves the dispatcher lock.
- Live rendering against the running team at 85 × 20 shows blockers immediately below progress and leader identity. Tests cover 85 × 59, 38 × 8, and tiny dimensions down to zero.
- Follow-up screenshot review: compact rows apply to agent teams only; normal workspaces retain their original rich presentation. Hidden indexes are stripped before rendering. Mixed-layout and model-display sidebar row/live suites pass.
- Model detection is complete: launch flags, hook/status-line reports, timestamp-ordered cached-transcript observations, and stale-metadata clearing. Seven model tests and ten token-cache tests pass. Full agent CLI/status, header, and header scheduler suites pass after model support.
- Final focused sidebar suites also pass for mixed layouts, model priority/fallback/unknown cases, narrow rows preserving status, and stable identity/search. Guide coverage, shell syntax, and diff whitespace checks pass.
- Model metadata was backfilled for running agents. Sidebar panes were restarted to activate normal workspace details and compact team name/model/status rows.
- The currently running team board and agent processes have not been restarted. Only sidebar panes were restarted for UI verification.

## Follow-up: Model below agent name

The user's narrow-sidebar screenshot exposed competition between name and model
in the same width budget. A failing 50-column fixture reproduced the inline model.
Move models onto dim secondary lines, wrap long identifiers, and use shorter
indentation/status icons below 36 columns. Cover the real 25-column sidebar as
well as 50 columns, preserve full agent search identities, and retain normal
workspace details. Terminal font size is pane-wide; dim styling supplies the
requested secondary emphasis.

Verified this follow-up live in 25- and 46-column sidebars: names/statuses remain
on the first line and models render below. The regression first failed with an
inline model, then passed at both 25 and 50 columns. Long model wrapping retains
all version characters and exact search matching. Focused sidebar rows, live
keyboard/state tests, shell syntax, diff checks, guide coverage, and the quick
project check all pass. Only sidebar panes were restarted.
