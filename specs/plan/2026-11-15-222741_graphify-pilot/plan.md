# Plan: Graphify pilot for agent teams

- **Date:** 15-11-2026
- **Branch:** feat/graphify-pilot (off feat/team-delivery-gates)
- **Status:** In progress — steps 1–3 done; step 4 (measurement) next
- **Authorization:** The user approved the pilot as proposed in chat ("yes").

## Context

Agent teams lose work to lookups: planners invent APIs instead of reusing
existing ones, contracts must name real repository patterns, and 20 of the 37
failed checks in `feat-system-observer` were file-policy violations (work
outside the planned files). [Graphify](https://github.com/Graphify-Labs/graphify)
builds a local, deterministic call/import graph of a codebase (tree-sitter, no
LLM for code, no telemetry) that agents query instead of grepping.

Its default setup does not fit the reviewed workflow: its skill and hooks do
not reach managed workers (native tools off, no network, a container), its
graph goes stale across lanes and snapshots, its strict mode blocks reads, its
semantic pass sends documents to an LLM provider, and it writes into the
project. So devAgentics owns the graph and offers it as a read-only tool, and
the pilot is kept only if `devagentics bench` shows it helps.

## Step 1 — Pinned, code-only Graphify

- Download the latest `graphifyy` release (double y: other `graphify*` packages
  are unaffiliated) into an isolated folder without installing it; read its
  metadata, dependencies, entry points and the code paths that touch the
  network, LLM backends and the query log.
- Install that exact version in an isolated tool environment (no
  `graphify install`: no skills, hooks or git hooks).
- devAgentics always runs it with `GRAPHIFY_BACKEND=none` and
  `GRAPHIFY_QUERY_LOG_DISABLE=1`, code only.
- Verify on publisherProvider's tracked source: build time, graph size, and
  answers to `explain`, `path` and `query` against known code.

## Step 2 — A graph per commit, in the task folder

- `graph.rs`: build the graph of a repo's tracked source at a commit (an
  exported tree, as candidate checks do) into
  `<task>/graphs/<repo>/<tree>/`, never into the project.
- Key it by the source tree, reuse it while the tree is unchanged, and record
  build time as a timing interval.
- Fail closed: no graph (Graphify missing, build failed) is an explicit answer,
  never a stale graph.

## Step 3 — A read-only `graph` tool

- Leader MCP tool and worker action `graph` (`explain`, `path`, `query`) on the
  graph of the agent's current source (its lane head or review candidate),
  bounded output.
- Offered to planners and reviewers first; role guidance tells them to use it
  for reuse, interfaces, callers and impact.
- Pilot switch per task (`devagentics task graph <task> on|off`, default off).

## Step 4 — Measure

- `bench` reports graph queries per run.
- Run the `notes` fixture with the graph off and on (the protocol in
  `docs/user/devagentics-bench.md`); keep it only if corrections
  (`stale_plan_interface`, `scope_omission`) or tokens drop.

## Verification

- TDD per step; `cargo fmt --check`, `cargo clippy --all-targets -D warnings`,
  `cargo test`, the Docker end-to-end tests, `scripts/check.sh`.
- Graphify behaviour checked against real repos, not assumed from its README.

## Execution notes

### Step 1 (15-11-2026)

- `graphifyy 0.9.111`, wheel sha256
  `1d377d7f6b0a3bea07b1b3887290132e2499137c1092c749331522764c5035f1`, pure
  Python (>=3.11), Apache-2.0. Base dependencies: networkx, numpy, rapidfuzz,
  pyyaml and tree-sitter grammars; MCP, Neo4j, PDF and others are extras.
- Read before installing: network fetching lives only in `ingest.py` (`add
  <url>`); the build path imports `security.py` only for secret redaction and
  size checks. `GRAPHIFY_BACKEND=none` disables every LLM backend. The query
  log is opt-in in this version (`GRAPHIFY_QUERY_LOG_ENABLE`), and
  `GRAPHIFY_QUERY_LOG_DISABLE=1` still forces it off.
- Installed from that verified wheel in its own venv,
  `~/.local/share/devagentics/graphify-0.9.111` (220 MB), with
  `~/.local/share/devagentics/graphify` linking to it. No `graphify install`:
  no skills, hooks or git hooks.
- `graphify update <dir>` builds the code-only graph and writes `graphify-out/`
  inside `<dir>`, so devAgentics builds in an exported copy, never the project.
- publisherProvider's tracked source, built with no network at all (`unshare
  -rn`) and a throwaway `HOME`: 9.8 s, 9,936 nodes, 27,770 edges; nothing in
  `HOME`.
- Answers: `explain` refuses an ambiguous name and lists both candidates (two
  `QueueDetails` components); on the exact node it lists the right importers,
  where grep also matched an unrelated sibling import. `query` is a fuzzy BFS
  (581 nodes, truncated to 50, noisy): the tool should favour `explain` and
  `path`, with `query` bounded.

### Steps 2–3 (15-11-2026)

- `graph.rs`: `graph()` exports a commit's tracked source (runner-visible files
  only when the task has a runner) into a staging folder, runs `graphify update
  .` there and renames it to `<task>/graphs/<repo>/<tree>/`; a tree's graph is
  built once. Graphify runs with `GRAPHIFY_BACKEND=none`,
  `GRAPHIFY_QUERY_LOG_DISABLE=1`, its own `HOME`, a timeout, and under `unshare
  -rn` when available. A failed build keeps nothing.
- `answer()` gates on `State.graph_enabled` (CLI-only `devagentics task graph
  <task> on|off`, which first checks `graphify --version`), parses explain /
  path / query (no option-like values), bounds answers to 16 KiB and records a
  `graph_queries` interval; `bench` reports the count.
- Leader MCP tool `graph` (task branch head); worker action `graph` (a
  reviewer's pending candidate, else the agent's lane head); the worker prompt
  mentions it only when the graph is on.
- Tests use a stub Graphify; an ignored test runs the pinned install (offline
  build, `explain formatTime` names its caller, nothing in its `HOME`).
