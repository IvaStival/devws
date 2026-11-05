# Spec: Lanes, group workspaces, hammers that stay

- **Date:** 04-11-2026
- **Branch:** feat/agentic
- **Status:** Executed (uncommitted)

## Context

Seen live: the CTO stopped a hammer after each step; only one hammer could
write at a time; the agents sat in the CTO's workspace. The user wants
hammers reused, the work split by area (back, front, infra, docs) so hammers
build at once, and a workspace per group.

## Requirements

1. Lanes: `add_lane(name, files, repo)` makes a worktree per area on
   `<branch>--<lane>`; one writer per lane; a repo with lanes takes writers
   only in lanes; orders default to the lane's files, catch the lane up with
   the task branch first; `accept_step` merges the lane into the task branch;
   a clash is undone and names the files.
2. Hammers stay between steps; `max_agents` 6; the leader stops agents only
   at the end, near a limit, or for room.
3. Each group (Seniors, Floor) is a workspace of its own beside the team's,
   with its task, four agents each; go, peek and the sidebar work across.
4. Also: header ticker self-heals; jungleforge Info in the App tab; hammers in
   auto mode; team workspaces never frozen.

## Success Criteria

- [x] `lanes.rs` tests: worktree, side by side + merge, clash undone, two
      lanes from spawn to merge.
- [x] `test_new_panel.sh`: group workspaces, Floor 2, peek, go, `--folder`.
- [x] Cross test with real devws and devagentics.
- [ ] Live on publisherProvider: back and front hammers at once.
