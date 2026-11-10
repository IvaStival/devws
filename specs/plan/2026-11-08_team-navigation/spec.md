# Spec: Team board, sidebar, and menu navigation

- **Date:** 08-11-2026
- **Status:** Executed

## Context

The current board prioritizes usage over blockers and lists all completed work. The sidebar spends three lines per workspace and truncates nested team names. The user approved a clarity and navigation redesign for the board, persistent sidebar, and popup.

## Requirements

1. Provide Overview, Agents, Steps, Activity, and Usage views. Prioritize progress, leader identity, questions, blockers, and dispatcher errors.
2. Select rows and open full details. Preserve selection across refreshes, support more than nine agents, keep numbered navigation and peek shortcuts, and locate the leader by identity.
3. Keep short/narrow panes usable with attention counts, scrolling, and adaptive hints.
4. Keep agent-team sidebar entries compact: name/status on the first line and a dim model line below when known; wrap models and adapt indentation/status icons for narrow panes. Normal workspaces retain their original branch, Git/PR, agent and editor details (user refinements after screenshot review). Expansion reveals team children. Allow persistent manual collapse/expansion. Show working/attention counts, and search hidden descendants.
5. Offer a contextual Team popup tab, including from group workspaces, that targets the existing board without starting another dispatcher.
6. Record observed model metadata from runtime reports and cached transcripts, with configured model as fallback. Do not guess unknown models or scan transcripts per redraw.
7. Preserve existing uncommitted work, usage caching, leader switching, and bounded refresh costs.

## Boundaries

No new approval, worker management, or dispatch control workflows. Dispatcher lifecycle remains unchanged. No commits requested.

## Success criteria

- Board rows remain reachable at small sizes and beyond nine agents.
- Questions and failures are visible before usage or history.
- Selection and expansion survive refreshes.
- Team menu targets the existing board and leader.
- Relevant shell suites, Rust formatting/lint/tests, and make check pass.
