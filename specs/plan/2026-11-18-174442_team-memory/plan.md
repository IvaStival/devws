# Team memory through ai-memory

- **Date** 18-11-2026
- **Branch** `feat/opencode-workers`
- **Status** Executed, live verification pending

## Problem

A new leader, senior or floor agent starts with nothing but `task_status`, the
log and its order. The leader role asks for ai-memory handoffs "when ai-memory
tools are available", but only a Claude leader has those tools. A Codex leader
and every reviewed worker (managed MCP-only adapter) cannot reach ai-memory.
The store shows session captures for the task's repo and no handoffs. What one
agent learned is lost when it is replaced.

## Approach

devAgentics talks to ai-memory itself, so every program gets the same memory
without its own wiring. Agents save notes and read them through devAgentics
tools. devAgentics writes the summaries that must not depend on an agent's
goodwill or on an account that has run out, and it hands each new agent what
it needs in its first message.

Pages go where `docs/design.md` already says. Task-wide notes go to the
`devagentics-tasks` project under `tasks/<task>/`, and notes about one repo go
to that repo's project. Stored memory is history, not instructions. Every page
handed to an agent is fenced and labelled as untrusted, as the ai-memory
session hook already does. Exact state stays in the task folder.

## Step 1 — A memory port for devAgentics

Add a `memory` module with a small trait: save a page, search, and read the
newest pages under a path. Implement it with the `ai-memory` CLI (`write-page`,
`search`, `recent`, `read-page`) using explicit `--workspace` and `--project`.
When ai-memory is missing or its service does not answer, use an
implementation that does nothing and says so once in the log. Give the fake
devws a recording memory for tests. Cap page size and the number of pages read.

## Step 2 — Agents save and search notes

Add leader MCP tools `memory_save` and `memory_search`, and the same two as
worker tools. A note has a kind (decision, gotcha, handoff, finding), a title
and a body, and devAgentics adds the author, role, step and time.
Workers write only under their own task, and only their repo's or the task's
project. The tools are recorded in the log like other worker tools. The role
instructions say when to save: a leader after each accepted step or decision;
a senior with its plan or review; a floor agent when it hits something the next
agent must know.

## Step 3 — devAgentics records what happened

Save a page without asking any agent when:

- an agent reports on an order: the step, order, outcome and report summary;
- a step is accepted or rejected: the decision and the reasons;
- the leader moves (switch, Change leader, or its account running out): a
  digest built from the task state, covering accepted steps, open orders and
  who holds them, blocked steps and why, open questions and escalations, and
  the next ready items.

The digest is deterministic and costs no tokens. It works even when the
outgoing leader's account has run out, which is the usual reason for a move.

The outgoing leader writes its own handoff too. When a move is due,
devAgentics asks the leader, through its inbox, to save a `handoff` note (what
it was doing, what it decided, what comes next). It moves the leader once that
note is saved, or after a deadline (10 minutes, or at once when the leader's
pane is gone). A late or missing handoff never blocks the move: the digest
covers it, and the log says which one the new leader got.

## Step 4 — New agents start with memory

- **New leader:** after a handover, a restart or `devws agentic`, its first
  message carries the digest and the newest leader handoff notes, so it
  carries on without searching.
- **New senior or floor agent:** its first order carries the newest notes for
  its step and lane, and the last report of the agent it replaces, when it
  replaces one (resume, takeover, `spawn_agent` for a lost agent's step).
- Each block is short, fenced as untrusted history, and names the pages, so the
  agent can `memory_search` for more.

## Step 5 — Verify

Unit tests for the port (CLI arguments, scoping, missing service), the tools
(scope refusals, sizes, log), the automatic pages, and the first messages of a
new leader and a replacement worker. Run the devAgentics suite, fmt, Clippy and
the menu suite. Live: move the leader with Change leader and check that the new
one opens with the digest. Replace a floor agent and check that its order
carries the previous report. Check the pages in the ai-memory web UI.

## Decisions

1. The outgoing leader writes its own handoff before every move, within a
   deadline (the user, 18-11-2026).
2. Every worker report is saved automatically (the user, 18-11-2026).

## Notes from execution

- All notes go to the `devagentics-tasks` project under `tasks/<task>/`. A
  note about a step goes under `steps/<step>/`, where the next order on that
  step reads it. Handoffs and digests are filed by kind. Per-repo projects were
  left out: ai-memory derives a repo's project name itself, and guessing it
  would scatter a task's notes.
- The fake devws keeps no notes unless a test turns memory on, so the tests
  written before this change keep their behavior.
