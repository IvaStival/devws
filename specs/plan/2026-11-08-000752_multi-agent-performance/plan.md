# Plan: Keep devws responsive with multiple active agents

- **Date:** 08-11-2026
- **Branch:** feat/agentic
- **Status:** Executed — verified and activated on 08-11-2026
- **Specification:** [spec.md](spec.md), approved by the user
- **Baseline:** [measurements.md](measurements.md)

## Approach

First establish comparable measurements, then remove confirmed unnecessary hook
work and repeated transcript parsing. Use a controlled rendering comparison to
decide the final redraw changes. Keep agent execution, follow behavior, usage
values, and state notifications correct throughout.

The recorded live sample identifies iTerm2 as the largest sustained CPU consumer,
but does not establish which redraw source caused it. The hook fixture establishes
two unnecessary full-repository scans per tool call without a linked editor;
its cold/warm single-run timings do not establish a concurrency slowdown ratio.

## Step 1 — Establish repeatable measurements

**Status:** Complete

**Files:**
- `scripts/measure_load.sh` — extend existing entry point, preserving numeric CPU reports
- `scripts/test_measure_load.sh` — new deterministic probe checks
- A small helper under `scripts/` only if needed for reliable sampling; keep
  production runtime dependencies unchanged
- `measurements.md` in this folder

**Actions:**
- Turn the temporary baseline and hook probes into a documented repository tool.
  Separate read-only live sampling from isolated fixture benchmarks.
- Benchmark one, two, and four simulated agents with the same repository fixture,
  one warm-up, at least five measured samples, and identical pre/post event counts.
  Cover both an eligible editor and no eligible editor. Report medians, p95 where
  sample count supports it, total Git calls, and full-repository scan counts.
- Add isolated state-transition measurements with one, two, and four agents and
  multiple sidebar workspaces. Record tmux command counts and sidebar rebuilds
  for repeated identical states and real transitions.
- Measure header token refreshes with large synthetic transcripts, recording
  unchanged and appended-file parse counts separately from durations.
- For live CPU samples, report sampling duration, CPU normalization, excluded
  short-lived processes, and tmux query latency. Do not label query latency as
  keystroke-to-screen latency. Never print transcript contents or process arguments.
- Record commands and machine/workload context so results can be repeated.

**Verification:**
- Run `bash scripts/test_measure_load.sh`; use fixture outputs to verify metric
  calculation, missing-tool handling, and cleanup, without timing assertions.
- Run the isolated benchmark and record the existing unnecessary scans before
  changing hooks. Confirm all fixture writes stay in temporary directories.
- Capture the live process identifiers needed to verify that validation does not
  restart or terminate running agents.

## Step 2 — Skip follow work without an eligible editor

**Status:** Complete

**Files:**
- `config/tmuxinator/agent_follow_hook.sh`
- `config/tmuxinator/agent_changes.sh`
- `config/tmuxinator/editor_open.sh` only if sharing eligibility requires it
- `scripts/test_agent_follow.sh`

**Actions:**
- Write regression cases before implementation for no tmux context, missing editor
  socket, a socket that has disappeared, and follow mode `off`. Assert zero Git
  snapshot scans, rather than checking that hooks simply return successfully.
- Resolve workspace/editor eligibility before snapshot or change detection.
  Match the existing follow-mode semantics; keep notify, focus, explore, default,
  and legacy `on` behavior intact. Do not add a synchronous editor RPC to each hook.
- Preserve per-pane snapshot isolation. Discard obsolete snapshot/index files when
  following is skipped. If eligibility changes between pre/post, do not compare
  against a snapshot from an earlier tool call or replay stale marks.
- Preserve normal post-hook payload fallbacks, shell-generated edits, line ranges,
  read-follow behavior, and prompt-time mark clearing whenever applicable.
- Keep hooks silent and non-fatal, with no change to agent security configuration.

**Verification:**
- Run `bash scripts/test_agent_follow.sh`, including existing enabled-follow tests
  and new transition cases: on→off, off→on, editor disappears, and concurrent panes.
- Run the Step 1 benchmark: disabled/unlinked follow must make zero repository
  scans for one, two, and four agents. Report warmed hook durations before/after.
- Verify the fixture's real Git index and staged state remain unchanged.

## Step 3 — Cache unchanged transcript work

**Status:** Complete

**Files:**
- `config/tmuxinator/header_tokens.sh`
- `config/tmuxinator/token_cache.sh` only if needed to isolate cache mechanics
- `scripts/test_header.sh`
- `scripts/test_token_cache.sh` if a separate cache helper is introduced

**Actions:**
- Add regression coverage for repeated unchanged queries before implementing a
  cache. Instrument source-file reads/parses; do not make tests depend on duration.
- Cache compact usage/limit records per source file under a private temporary
  cache directory. Store only data needed for header calculations, without prompts,
  tool output, or credentials. Use canonical account/path identity and parser schema
  version in the cache key; prevent cross-account and test/live contamination.
- Detect append, truncation, replacement, and same-size rewrites using file
  identity, size, and sufficiently precise modification/change metadata. Reparse
  when reliable invalidation cannot be established. Detect a file changing during
  parsing and avoid publishing the result as a valid unchanged-file cache entry.
- Reparse only changed files; byte-offset incremental JSONL parsing is outside
  this iteration. Enumerate eligible files each refresh so additions, deletions,
  and files aging out of the existing time window are reflected correctly.
- For Codex limits, cache source records and apply reset-time rules at render
  time, even when files have not changed. For costs, retain the records necessary
  for existing cross-file/message-ID deduplication and model association; do not
  sum per-file totals if doing so changes semantics.
- Preserve month boundaries, price changes, unknown-model flags, session totals,
  and account isolation. Price final records using current pricing, or invalidate
  derived results when the pricing file changes.
- Ignore incomplete/malformed trailing records consistently and detect later
  completion. Publish cache files atomically. Concurrent readers/writers must see
  complete valid records; a cache error falls back to source parsing, never to
  fabricated zero usage. Prune expired cache entries without touching source logs.

**Verification:**
- Run header and cache tests for all modes: `session`, `usage`, `limits`, `costs`.
- Assert unchanged refreshes perform zero source transcript reparses; changing
  one file reparses only that file. Verify append, truncate, replace, same-size
  rewrite, added/deleted file, malformed-then-completed tail, and concurrent reads.
- Verify time-only limit resets, month rollover, pricing changes, duplicate Claude
  message IDs, and Codex model/turn associations against established expected values.
- Repeat the large-log benchmark with unchanged and appended inputs; record cost
  reduction separately from terminal CPU.

## Step 4 — Reduce rendering and state work supported by measurements

**Status:** Complete — controlled comparison retained animation behavior

**Files:**
- `config/tmuxinator/header_ticker.sh`
- `config/tmuxinator/agent_spinner.sh`
- `config/tmuxinator/agent_state.sh`, `agent_status.sh` if state work is material
- `config/tmuxinator/refresh_devws_pickers.sh` if reload fanout is material
- `scripts/test_header.sh`, `scripts/test_agent_status.sh`

**Actions:**
- Use fixed synthetic output and agent-state schedules in an isolated tmux server
  to compare normal updates against held decorative frames. Keep geometry,
  visible content volume, terminal dimensions, and sample duration comparable.
- Measure real terminal CPU with attached-client comparisons where possible.
  Synthetic detached tmux tests can count updates but cannot establish iTerm2
  rendering cost. If a brief live diagnostic is needed, snapshot and restore
  exact prior display settings with cleanup traps and a bounded duration; keep
  active agents running and state notifications available.
- Make only changes supported by these measurements. Preferred small changes:
  suppress unchanged writes, batch related visible updates, and avoid global
  animation writes for workspaces with no relevant visible activity.
- If repeated identical state events or sidebar fanout are significant, suppress
  redundant processing or coalesce reloads. Preserve final state and alert
  delivery, per-pane aggregation, transcript updates, and hidden-sidebar freshness
  when switching workspaces. Do not replace events with aggressive polling.
- Add behavior/count regression tests before each selected optimization. Retain
  existing focus, popup, narrow-terminal, hidden-header, and multi-client behavior.
- If decorative redraw suppression does not materially reduce terminal CPU,
  record that result and inspect agent-output repaint cost next. Do not remove
  features or claim the visual slowdown is resolved from hook benchmarks alone.

**Verification:**
- Run `bash scripts/test_header.sh` and `bash scripts/test_agent_status.sh`.
- Verify working/waiting/done transitions, concurrent agent changes, workspace
  switching, popup behavior, and unchanged-row suppression.
- Repeat matched terminal CPU and query-latency samples at least three times per
  condition; report ranges, not a guaranteed improvement from a single sample.
- Show the measured redraw/subprocess reduction and any remaining bottleneck.

## Step 5 — Document runtime behavior and adoption

**Status:** Complete

**Files:**
- `docs/architecture/agent-performance.md` if the implementation adds new cache or
  event-flow architecture; use the applicable doc-generator skill
- Existing guide configuration/agent pages only if user-visible behavior changes
- `spec.md`, `plan.md`, `measurements.md` in this folder

**Actions:**
- Document cache ownership, invalidation, fallback, and cleanup, plus the benchmark
  invocation and measured tradeoffs. Record the actual implementation, omitting
  candidates that were not needed.
- Explain how changes reach running sessions: hook scripts are linked to this
  checkout; long-lived ticker processes may need a targeted reload. Do not restart
  agents or broadly reload unrelated configuration as a performance workaround.
- Keep before/after measurements with commands, fixture/workload conditions,
  limits of the measurements, and unresolved findings.

**Verification:**
- Verify documented commands against the actual probe interface.
- Verify any targeted display reload preserves existing pane identities and agent
  processes. Ensure cache documentation matches tests and implementation.

## Step 6 — Complete final validation

**Status:** Complete — final `make check` passed; ticker activated

**Files:** All changed files and phase artifacts.

**Actions and verification:**
1. Run affected suites as each step completes. After the final change, run
   `make check` once with the required tmux access. It covers shell syntax,
   executable bits, templates, available Lua syntax checks, Rust formatting,
   Clippy/tests, and all shell integration suites. Do not launch a competing
   sandboxed full run that is known to lack tmux access.
2. Run `git diff --check`; review scope and preserve the earlier panel fix.
3. Rerun one/two/four-agent and unchanged/appended-transcript benchmarks. Publish
   operation counts and repeated timing summaries next to the baseline.
4. Verify running agents remain alive and that the editor, terminal, menu, agent
   state indicators, and follow behavior still work.
5. Mark only verified success criteria complete. Update phase status to Executed
   only when required work is finished; record any remaining rendering limitation
   explicitly. Do not mark the user's visible slowdown fixed without evidence.

## Execution checkpoint

The user approved execution on 08-11-2026. Implementation and validation are complete; remaining terminal-renderer cost is documented.
No commit is included in this plan; commit only if requested, using the commit skill.

## Execution notes

- The existing numeric CPU-report interface was retained; named modes extend it.
- Disabled/unlinked follow produced zero scans for all agent counts. Enabled
  follow continues to detect shell edits; the follow integration suite passed.
- Compact-record cache tests cover unchanged queries, append, incomplete tails,
  truncate, replacement, same-size rewrite, limit-only events, time-based resets,
  deduplication, pricing, concurrency, corruption, and cache-directory failure.
- Rendering comparisons did not show material terminal CPU savings when the
  ticker was held. Animation rate and sidebar reload policy were retained. State
  work and empty ticker wakeups were reduced with deterministic regressions.
- The state probe measures one/two/four agents and tmux operation counts; it does
  not benchmark live fzf fanout. Existing integration tests verify sidebar updates.
- Validation exposed the account-row/cost startup race, now refreshed on cost
  completion. Header tests now wait for asynchronous costs and avoid mistaking a
  full spinner cycle for a stopped spinner.
- Remaining iTerm2 annotation-renderer CPU is recorded in measurements.md; no
  claim is made that these changes alone eliminate the visible slowdown.
- No commit was requested or created.

- Full validation found a CLI fixture that omitted the new Perl helper and a
  guide-check exemption missing for diagnostic-only variables. Both were fixed.
- A header retry caught a lost cost-refresh notification. The deterministic
  regression failed before moving marker consumption to the beginning of the
  account refresh, and passes after that fix.

- Final `make check`: exit 0, all checks passed. Perl syntax, doc links, missing
  probe tool handling, and whitespace checks passed as well.
- Activated the installed ticker after validation. Eleven pane identities and
  ten recorded Codex process IDs survived unchanged; no agent restart occurred.
- Final benchmark: four unlinked-editor agents, eight scans → zero; median
  batch 572.20 ms → 266.77 ms in the synthetic fixture. No claim of an enabled
  follow speedup or resolved iTerm2 renderer CPU is made.
