# Architecture: Agent performance

## Overview

devws adds editor-follow hooks, state indicators, and usage statistics around an
agent process. Their cost matters when several agents run at once: a tool event
can trigger repository scans, tmux queries, and header updates before the next
tool starts. The runtime avoids work that has no consumer and reuses transcript
metadata that has not changed.

```text
Tool event
    ├── agent_follow_hook.sh
    │       ├── editor absent / follow off → discard obsolete snapshot
    │       └── eligible editor → snapshot / diff → editor_open.sh
    └── agent_state.sh → agent_status.sh
                            ├── pane state changes → update pane / tab alert
                            └── aggregate changes → update session / sidebar

header_ticker.sh
    ├── display beat: 0.5 seconds
    ├── usage refresh → header_tokens.sh → token_cache.pl
    │                                      ├── unchanged → compact cached records
    │                                      └── changed → parse source / publish cache
    └── monthly cost refresh → mark account rows for refresh
```

The display beat does not determine agent execution speed. It controls the
existing ghost and spinner updates. Agent state changes still arrive through
hooks, and notifications remain tied to state transitions.

## Editor-follow eligibility

`agent_follow_hook.sh` checks the pane's workspace, linked editor socket, and
follow mode before parsing the tool payload or invoking Git. Without a socket,
outside tmux, or with following off, no repository snapshot is needed. Checking
for the Unix socket is deliberately cheaper than a synchronous Neovim RPC.
An existing but unresponsive socket can still pass this check; `editor_open.sh`
retains its existing bounded background remote request.

Each eligible pane owns a snapshot named `pane-<pane id>`, sanitized by
`agent_changes.sh`. `discard` removes its temporary index and before-tree
without inspecting Git. This prevents a skipped pre-hook from leaving a snapshot
that a later post-hook could mistake for the current tool call. The eligible
path continues to detect shell-generated changes and send changed line ranges
to the editor, while payload fallbacks preserve reads and direct edits.

Enabled following still scans the repository before and after a tool call. The
optimization removes those scans when their result cannot be displayed. It
does not try to infer whether an arbitrary shell or MCP tool can modify files.

## Compact transcript cache

`header_tokens.sh` retains the public modes `session`, `usage`, `limits`, and
`costs`. Their aggregation, prices, limit-reset times, and formatting remain in
the shell/jq layer. `token_cache.pl` receives NUL-separated source paths on stdin
and emits normalized JSONL records to that layer.

The helper uses Perl and core modules already available on the supported macOS
runtime. It retains token counts, model/turn associations, message identifiers,
timestamps, and primary/secondary limit metadata. It omits message content,
tool arguments, and tool results. Records are cached instead of final totals
because Claude messages can repeat across files, prices can change, and a limit
can reset without any source file changing.

Cache files live in `${TMPDIR:-/tmp}/devws-tokens-<uid>/`. The directory must be
owned by the current user, private (`0700`), and not a symlink; otherwise parsing
continues without caching. A SHA-256 key combines the canonical source path and
parser schema version. Canonical account paths isolate accounts and fixtures.
Entries use `0600` permissions and contain a fingerprint plus the compact records.

The fingerprint includes device, inode, size, and high-resolution modification
and change times from `Time::HiRes::stat`. Appends, truncation, replacement, and
same-size rewrites invalidate the entry. Source metadata is checked again after
parsing; a source that changed during the read is not published as a stable
cache entry. The next refresh retries it. An invalid/incomplete JSONL line is
skipped, and completing that line changes metadata and triggers another parse.

Writes use a unique temporary file in the cache directory followed by an atomic
rename. Simultaneous cold readers may both parse a source, but they publish
complete generations. Cache corruption, directory failures, or publication
failures fall back to source parsing. Source transcripts are never modified.
Entries older than 32 days are pruned at most once an hour when the helper runs.

Every query still enumerates the eligible log set. Added/deleted files and files
aging out of the existing eight-day limit or 32-day cost windows are reflected
without caching a stale directory listing. A changed file is reparsed in full;
this implementation does not track JSONL byte offsets. All callers can reuse
the same compact records, and reset rules and prices are applied at query time.

For diagnostics, `DEVWS_TOKEN_CACHE_DISABLE=1` bypasses the cache and
`DEVWS_TOKEN_CACHE_METRICS=<file>` appends `parse <bytes>` for each source parse.
Metrics contain no paths or transcript text. These are diagnostic environment
overrides, not settings required for normal use.

## State updates and display scheduling

`agent_state.sh` extracts the event, notification type, and transcript path in
one JSON parse, using NUL-separated fields to preserve quoted paths. It obtains
session and pane role together, and writes pane/session transcript options only
when their paths change. Missing transcript fields preserve the previous path;
extra agents continue to update only their own transcript option.

`agent_status.sh` compares a pane's state before writing it. A repeated working
event does not rewrite pane or session options, or re-mark the tab. Session
aggregation uses one `list-panes -s` query containing both role and state,
including hidden windows, rather than one query per agent. Priority remains
waiting, working, done, then idle. Existing ticker health checks still run on
repeated events so an unchanged state can recover a dead ticker.

The header uses 20 logical ticks per second and advances ten ticks every
half-second. All scheduled tasks fall on these boundaries. Spinner indices
advance by one and ghost frame indices by five per wake, preserving their
previous visible sequence without waking for intermediate ticks that did no
work. Focus, popup, and hidden-header rules still govern display updates.

Costs and account rows refresh independently. Completing a cost calculation
marks `costs.changed`; the regular loop requests an account refresh, using its
existing worker slot, instead of waiting up to a minute for the next usage
period. This avoids startup rows being built before the cost file exists.
The account worker consumes pending notifications before rendering, so a cost
result arriving during an older render retains its request for another pass.

Sidebar reload policy remains event-driven. It was not broadened or replaced
with polling: repeated events now avoid option writes, while real aggregate
changes retain the existing sidebar notification path.

## Measuring changes

The original report remains available, grouped by workspace and pane kind:

```bash
scripts/measure_load.sh 20
```

Named modes require Python 3 for development probes; production hooks and the
header do not depend on it:

```bash
scripts/measure_load.sh live --seconds 5
scripts/measure_load.sh hooks --samples 5 --files 1500
scripts/measure_load.sh tokens --samples 5 --log-mb 32
scripts/measure_load.sh state --samples 5
scripts/measure_load.sh rendering --seconds 4 --samples 3
```

`hooks`, `tokens`, and `state` use disposable fixtures and start no paid agents.
The hook fixture includes eligible and absent editors and reports Git calls and
repository scans for one, two, and four simultaneous simulated agents. The token
fixture reports source bytes and parses for uncached, unchanged, and appended
queries. The state fixture counts option writes and tmux queries on an isolated
server. None of these count an attached terminal's actual paint operations.

`live` reads process counters and tmux metadata. Its CPU numbers exclude
processes created or exited between snapshots. Query latency is not
keystroke-to-screen latency. `rendering` alternates live samples with the header
ticker briefly stopped, then resumes it in a cleanup handler; agent processes
keep running. This mode requires process/tmux access and should finish before
starting another rendering comparison. It isolates header maintenance from the
rest of the live workload, whose output may still vary.

Deterministic tests assert operation counts and behavior. Wall-clock probe
results are recorded separately, with warm-up and repeated samples, because
filesystem caches and other applications affect elapsed times. The initial
comparison and remaining iTerm2 renderer finding are in the
[measurement record](../../specs/plan/2026-11-08-000752_multi-agent-performance/measurements.md).
That renderer cost went away when the terminal changed to Ghostty; see the
[resolution](../../specs/plan/2026-11-08-073107_iterm-renderer-follow-up/measurements.md#resolution).

## Extending the cache

When usage parsing needs a new source field, add it to the compact record
whitelist, bump the schema key, and add a test through `header_tokens.sh`. Test
both cold and unchanged queries and a file mutation; an output-only test would
miss accidental whole-log rereads. Preserve cross-file deduplication and apply
time-based rules after cache lookup.

## Referenced files

| File | Responsibility |
| --- | --- |
| [agent_follow_hook.sh](../../config/tmuxinator/agent_follow_hook.sh) | Check eligibility and follow tool changes |
| [agent_changes.sh](../../config/tmuxinator/agent_changes.sh) | Snapshot, diff, and discard pane snapshots |
| [editor_open.sh](../../config/tmuxinator/editor_open.sh) | Apply enabled follow modes to Neovim |
| [agent_status.sh](../../config/tmuxinator/agent_status.sh) | Update pane and aggregate state |
| [header_tokens.sh](../../config/tmuxinator/header_tokens.sh) | Aggregate and format usage records |
| [token_cache.pl](../../config/tmuxinator/token_cache.pl) | Normalize and cache per-file metadata |
| [header_ticker.sh](../../config/tmuxinator/header_ticker.sh) | Schedule data and display updates |
| [measure_load.sh](../../scripts/measure_load.sh) | Existing CPU report and probe entry point |
| [measure_load.py](../../scripts/measure_load.py) | Isolated workloads and sampled measurements |
