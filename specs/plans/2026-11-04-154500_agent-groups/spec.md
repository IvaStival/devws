# Spec: Agent groups — CTO, Seniors, Factory floor

- **Date:** 04-11-2026
- **Branch:** feat/agentic
- **Status:** Executed (uncommitted)

## Context

Roles were flat: the leader ordered any agent, and the ui-designer wrote UI
code itself. The user wants three groups: the CTO (the leader) handles
everything; seniors (planner, ui-designer, a future QA) plan an execution and
answer questions about it; the factory floor (the hammers) only runs what a
senior planned.

## Requirements

1. A role has a `group` (`cto`, `senior`, `floor`); old files get one from
   `writes`. A senior never writes; the floor always does.
2. After the user approves the overall plan, `devagentics plan write` from a
   senior stores an execution plan for the step of its order; the CTO approves
   it by accepting that step (`read_plan`, `accept_step`).
3. An order to the floor names an accepted execution plan (`plan`); it is
   refused otherwise. A senior's order forbids every file.
4. A hammer asks (`devagentics ask`); the senior who planned answers
   (`devagentics answer`), or the CTO (`answer_question`) when that senior is
   gone. An agent waiting for an answer is not stalled.
5. The board groups "Who does what" by group and shows open questions.
6. Codex agents can write the task's state (`--add-dir`); Claude agents are
   pre-allowed `plan show`, `ask`, `answer`.

## Constraints

- Orders still go CTO → agent; the user approves only the overall plan.
- The ui-designer hands over a design and a front-end plan, no code.

## Success Criteria

- [x] Unit tests for groups, plans, floor orders, questions, board, sandbox.
- [x] Flow test: plan → approve → execution plan → accept → hammer with plan
      → ask → answer → report → accept → PR.
- [x] `scripts/check.sh` passes.
- [ ] Live: the ui-designer plans a UI change and hammer-codex builds it.
