# Initial performance measurements

Date: 08-11-2026. Read-only live sampling plus an isolated synthetic hook fixture.
No application code changed during this investigation.

## Live sample

Command: `python3 /private/tmp/devws-perf-investigation/baseline.py`

Thirty tmux metadata queries spaced 150 ms apart; elapsed 4.97 s.
CPU is the change in cumulative process CPU time over the sample, summed by
executable for processes present at both endpoints. Short-lived process CPU is
not included, so this undercounts shell/grep/hook work.

| Metric | Value |
| ------ | ----- |
| iTerm2 CPU, one core = 100% | 109.3% |
| Codex CPU, ten processes combined | 18.7% |
| WindowServer CPU | 12.5% |
| tmux CPU, two processes combined | 4.8% |
| tmux metadata latency median | 8.25 ms |
| tmux metadata latency p95 | 15.50 ms |
| tmux metadata latency max | 68.32 ms |
| Allocated swap in earlier snapshot | 3643.88 MB |

An earlier instantaneous sample showed two grep processes at 89.2% and 86.1%.
Their calling ancestry was not established, so attributing them to the header
would be speculation. Swap allocation alone does not prove ongoing pressure.
The sampling command is not a measurement of keystroke-to-screen latency.

## Follow-hook fixture

Command: `python3 /private/tmp/devws-perf-investigation/hooks.py`

Fixture: 1500 tracked 1.5 KB text files in a disposable Git repository. Real
`agent_follow_hook.sh` and `agent_changes.sh`; wrapped Git records command count.
Each simulated agent receives one pre/post pair for a read-only tool call.
Each agent has its own pane identifier. tmux is stubbed to reject editor lookup;
no linked editor exists. Neither tool nor fixture files change during the pair.

| Concurrent agents | Git invocations | Whole-repository `git add -A` scans | Wall time |
| ----------------- | --------------- | ---------------------------------- | --------- |
| 1 | 8 | 2 | 1237 ms |
| 2 | 16 | 4 | 532 ms |
| 4 | 32 | 8 | 645 ms |

These are single runs, with the one-agent run first and cold. The timings do not
establish a concurrency slowdown ratio. The deterministic operation counts
establish unnecessary linear growth: two full scans per tool call even when
there is no editor to consume the result. The scans also create temporary Git
objects in the fixture, as the production implementation does in its repository.

## Transcript metadata

Only file metadata was inspected; no transcript content was printed.

| Default account folder | JSONL files | Modified within 8 days | Modified within 32 days |
| ---------------------- | ----------- | ---------------------- | ----------------------- |
| `.codex/sessions` | 34 | 119.9 MB | 141.5 MB |
| `.claude/projects` | 71 | 107.4 MB | 171.6 MB |

`header_tokens.sh limits` greps Codex JSONL files modified within eight days and
parses matching records every refresh. The ticker schedules account/session
updates every minute. Cost calculation reads the 32-day file set every ten
minutes. The metadata totals describe eligible input sizes; they are not direct
CPU or I/O duration measurements. Custom agent account folders were not counted.

## Ranked hypotheses and next discriminating measurements

1. Terminal redraws dominate the visible slowdown. Compare the same workload
   with normal updates and temporarily held decorative/status updates, restoring
   prior settings automatically. Prediction: terminal CPU drops while agent
   workload remains comparable.
2. Follow hooks perform unnecessary Git work, amplified by concurrent agents.
   The fixture confirms the unnecessary work. Compare a gated implementation
   using warmed repeated samples and assert zero scans without an eligible editor.
3. Repeated historical log scans cause periodic CPU/I/O bursts. Compare unchanged
   refreshes against appended-log refreshes, with operation counts and durations.
4. Agent-state/sidebar fanout causes redundant subprocesses as agent and workspace
   counts rise. Count tmux invocations and sidebar rebuilds under repeated identical
   states and concurrent state transitions before deciding whether to change it.

## Repeated hook comparison

Command: `bash scripts/measure_load.sh hooks --samples 5 --files 1500`.
One warm-up per condition, then five measured batches. The same read-only
fixture is used for all cases. Measurements were made during normal machine
activity; timings are indicative, while the scan counts are deterministic.

| Agents, no editor | Baseline median | Optimized median | Baseline scans per batch | Optimized scans |
| --- | --- | --- | --- | --- |
| 1 | 506.64 ms | 112.47 ms | 2 | 0 |
| 2 | 442.33 ms | 148.61 ms | 4 | 0 |
| 4 | 572.20 ms | 266.77 ms | 8 | 0 |

Enabled-follow cases still perform two scans per pane, preserving arbitrary
shell/MCP edit detection. Their timings varied between runs and do not establish
an enabled-follow speedup. The absent-editor case removes all Git calls, not
just its `add -A` scans.

## Transcript cache comparison

Command: `bash scripts/measure_load.sh tokens --samples 5 --log-mb 32`.
The fixture is approximately 32 MB, mostly irrelevant tool-output records, with
a valid token-count record. The uncached condition uses the new compact parser
with caching disabled, rather than the unmodified original jq pipeline.

| Five measured refreshes | Median refresh | Source parses | Source bytes parsed |
| --- | --- | --- | --- |
| Cache disabled | 100.52 ms | 5 | 167,770,075 |
| Unchanged, warm cache | 86.29 ms | 0 | 0 |
| Appended before every refresh | 106.67 ms | 5 | 167,774,695 |

The strongest result is avoided source I/O; process startup and formatting
still contribute to elapsed time. A changed file is reparsed in full. These
measurements do not establish a reduction in terminal rendering CPU.

## State and scheduler measurements

The pre-fix regression on an isolated server found two unnecessary option writes
for a repeated working event with one agent. The fixed test asserts zero writes.
`bash scripts/measure_load.sh state --samples 5` then measured these unchanged
batches; all five samples had the same counts:

| Agents / events per batch | tmux calls | Option writes |
| --- | --- | --- |
| 1 | 5 | 0 |
| 2 | 10 | 0 |
| 4 | 20 | 0 |

The state fixture does not benchmark live fzf sidebar rebuild cost. Real state
transitions retain the existing sidebar reload path; no new fanout policy was
introduced. Existing status integration tests cover that path and alerts.

The deterministic scheduler regression records four half-second waits and
ghost indices 0, 5, 10, 15; spinner indices 0, 1, 2, 3; two layout runs; and one
initial token refresh. The previous scheduler failed this test because it woke
every 0.05 s and advanced through ticks with no scheduled output. The new loop
performs two waits per second instead of 20, preserving displayed frame indices.
This reduces wakeups by 90%; it is not a claim of a 90% CPU reduction.

## Live header comparison and remaining renderer bottleneck

Command: `bash scripts/measure_load.sh rendering --seconds 4 --samples 3`.
The order alternated normal/held, held/normal, normal/held. Only the header ticker
was held; it was resumed in cleanup. Agent output continued, so workload was not
perfectly fixed. Six sample durations were 4.07–4.22 seconds.

| Condition | iTerm2 CPU samples, one core = 100% | Median query latencies |
| --- | --- | --- |
| Ticker running | 109.4%, 108.6%, 108.6% | 11.05, 8.38, 7.89 ms |
| Ticker held | 109.0%, 109.0%, 108.0% | 11.93, 11.11, 7.04 ms |

Holding the ticker did not materially reduce iTerm2 CPU. Therefore no reduction
in visible animation rate or removal of header features was justified.

A two-second `sample` profile of iTerm2 3.4.20 found 140 of 156 main-thread
samples under `iTermMetalPerFrameState loadAnnotationRangesFromTextView:`, with
most of that subtree traversing `IntervalTree` through
`VT100Screen charactersWithNotesOnLine:`. This local stack evidence identifies
annotation-range work inside iTerm2's Metal renderer as a remaining bottleneck;
it does not establish which terminal content or historical annotations caused it.
No terminal contents were read or annotations deleted. No iTerm2 upgrade or
restart was performed as part of these code changes.

The code changes reduce avoidable devws work. A claim that the user's entire
visible slowdown is resolved still requires a follow-up responsiveness check
and investigation of the terminal-renderer issue.

## Final validation and activation

The final optimized hook numbers above were recorded after the complete check
finished, with editor eligibility batched into one tmux query per event. Earlier
intermediate runs varied, reinforcing that operation counts are the reliable
regression signal. Enabled following retains its original scan count and is not
claimed to have become faster.

`make check` completed with exit status 0 and `All checks passed.` This includes
Rust formatting/Clippy/tests, shell/template checks, and every integration suite,
including the ten cache cases and the deterministic lost-notification regression.
`perl -c config/tmuxinator/token_cache.pl`, documentation link checks, the probe's
missing-tool check, and `git diff --check` also passed.

The installed header ticker was restarted after validation to adopt its new
schedule. All eleven pane IDs/process identities were unchanged, and all ten
recorded Codex process IDs remained present. Agent processes were not restarted.
The existing panel-recovery changes and untracked images were preserved.
