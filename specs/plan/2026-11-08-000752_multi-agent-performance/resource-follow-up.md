# Additional CPU and memory investigation

- **Date:** 08-11-2026
- **Status:** Hook-entry optimization verified; approved Docker 2 GiB trial applied and healthy
- **Scope:** User requested further CPU/memory optimization. The narrow state-hook
  change continues the previously approved state-entry optimization scope.

## Pre-trial measurements

A five-second live probe measured iTerm2 at 91.1% of one core, tmux at 5.8%,
and thirteen Codex processes combined at 10.1%. Query latency was 6.95 ms median
and 8.72 ms p95. Process counters omit children that exit between samples.

The host has 8 GiB physical RAM and 3365.75 MiB allocated swap. Allocated swap
alone does not establish present paging activity. A separate 5.03-second VM
counter sample observed 8.03 MiB swap-in, no swap-out, 1346.94 MiB compressed,
and 778.02 MiB decompressed. At its endpoint the system reported 4.62 GiB wired,
1.41 GiB physical compressor storage, and 11.18 GiB logical compressed pages.
These counters show current memory turnover, not just an old swap allocation.

Per-process `footprint --noCategories --swapped` reported:

| Group | Reported physical footprint |
| --- | --- |
| Apple virtualization process, confirmed to be Docker's VM | 1877 MB |
| Thirteen Codex processes, summed | 907 MB |
| iTerm2 | 468 MB |

The process figures are reported accounting footprints; they are not a promise
of unique physical memory freed by closing a group. A complete system-wide
footprint scan required root and was not pursued. RSS-only rankings are
misleading under compression/swapping and are not used to promise RAM savings.

A second named-process snapshot covered Node runtimes, REPLs, Neovim, fzf,
and ai-memory. The whole selected set reported 714 MB, including unrelated
desktop Node helpers; it is not all attributable to devws. Neovim accounted for
about 83 MB, the four fzf processes about 33 MB, twelve node_repl processes about
107 MB, and ai-memory about 38 MB. These measurements do not justify removing
editor/sidebar features or terminating agent tools to claim large RAM savings.

Docker's VM ownership was verified through open-file metadata pointing to
Docker's kernel, desktop image, and VM disk. At this baseline, both resource settings files reported
a 4096 MiB memory limit and eight CPUs. The limit is a ceiling, not proof that
the full 4 GiB is currently consumed.

`docker stats --no-stream` showed:

| Running service | Container memory | Container CPU |
| --- | --- | --- |
| observer-db | 42.47 MiB | 0.04% |
| rabbitmq | 226.4 MiB | 0.44% |

Both services were healthy. Five other containers were exited. No Docker
settings or container states were changed during that initial investigation. A future daemon restart must account
for stopped containers' restart policies as well as the two running services.

## Implemented narrow CPU optimization

The earlier state-status optimization avoided repeated working-state writes,
but `agent_state.sh` still parsed every regular payload twice and rewrote both
transcript options on every event.

The new actual-hook regression failed with:

```text
Agent status test failed: an unchanged transcript event rewrote 2 tmux options
```

The entry hook now parses its required fields once, batches session/role lookup,
and writes transcript options only when changed. The regression verifies zero
option writes for an unchanged event, one JSON parse, and correct quoted/
backslash-containing transcript paths. Existing state, notification, missing/
changed transcript, extra-pane, and malformed-input behavior remains covered by
`scripts/test_agent_status.sh`, which passed after the change.

This removes redundant CPU/process/terminal update work. It does not establish
material RAM savings or resolve the separate iTerm2 rendering bottleneck.

## Controlled Docker memory trial

A trial reducing the VM ceiling from 4096 to 2048 MiB is a candidate for the
current small container workload, subject to workload-headroom checks. The
[official Docker resource settings](https://docs.docker.com/desktop/settings-and-maintenance/settings/#advanced)
define the memory limit as VM RAM and document the resource controls.

The user approved this trial. Execution used the following preservation checks:

1. Confirm whether a short Docker restart interrupting observer-db and RabbitMQ
   is acceptable. Keep active tmux agents running and identify work dependent on
   those services.
2. Capture a private resource-settings backup plus running/stopped container
   identities, health checks, and restart policies. Prepare rollback to 4096 MiB.
3. Apply only the memory limit through a supported interface, restart Docker in
   the agreed window, and restore the exact intended service set. Do not restart
   exited containers unintentionally or delete images, volumes, queues, or data.
4. Verify service health and absence of OOM/restart loops. Re-measure host
   pressure, VM footprint, and representative workload throughput, allowing for
   VM cache warm-up and restart effects. Keep the lower limit only if sufficient
   headroom remains; otherwise restore the previous limit.

No exact memory saving is promised: the VM currently consumes less than its
4 GiB limit, and restarting it also changes its cache state. CPU limits remain
unchanged because current container CPU use does not justify reducing them.

## Existing constraints and pending terminal work

`suspend_workspace.sh` uses SIGSTOP: freezing can save CPU but does not release
allocated RAM. Agent-team workspaces are intentionally excluded so orders can
arrive between idle-looking periods. No suspension guard was removed, no active
agent was stopped, and no MCP tools were disabled.

The [fresh-terminal comparison](../2026-11-08-073107_iterm-renderer-follow-up/plan.md)
remains pending the manual window action because the computer-control tool
denied iTerm2 access. This investigation does not bypass that restriction.

## Final validation

`make check` completed with exit status 0 and `All checks passed.` It includes
Rust format/Clippy/tests, shell syntax/template checks, and all integration
suites. The workspace-minimize timing suite failed beside the other suites and
passed on the runner's isolated retry; its reported symptom concerned header
resizing, not the state-hook change. The focused state-hook regression and its
full-run suite both passed. `git diff --check` and relevant Bash syntax checks
passed as well.

The installed agent_state.sh is linked to the checkout, so configured hooks use
this optimization on their next invocation without agent restarts. No claim of
RAM savings is made for the hook change alone. The subsequently approved Docker
trial results are recorded below.

## Completed Docker trial

The user explicitly approved lowering the memory ceiling and the brief service
interruption. Docker Desktop UI control timed out, so the operation used its
supported stop/start CLI and the settings files while Docker was stopped. No
alternative UI automation was used.

Private backups and the container/process inventory were saved under
`~/.devws-state/docker-memory-trial/2026-11-08-191726/` (directory `0700`, private
files `0600`). `settings-store.json`'s `MemoryMiB` and legacy `settings.json`'s
`memoryMiB` changed from 4096 to 2048. The atomic patch verified that no unrelated
JSON fields changed; Docker's CPU allocation remains eight.

The engine reports 1970 MiB usable memory after the 2048 MiB ceiling, compared
with 3916 MiB usable before. Both persisted setting values remained 2048 after
startup and observation.

| Observation | Reported Docker VM footprint |
| --- | --- |
| Before restart / 4096 MiB ceiling | 1877 MB |
| After restart / sample 1 | 1204 MB |
| After restart / sample 2 | 1205 MB |
| After restart / sample 3 | 1206 MB |

The three post-restart samples span 44.6 seconds.
The observed VM footprint was about 671 MB lower at the final sample. This is
an accounting-footprint comparison shortly after a restart with a lower limit;
it does not isolate the limit from cache reset and warm-up effects or guarantee
that all workloads retain the same saving.

Host-wide memory-free percentage was 15% immediately before the trial and 16%
at the final monitored sample. Wired memory varied from 4.70 GiB before to
4.71–4.82 GiB during observation; this trial does not establish that overall
host memory pressure or the separate iTerm2 CPU problem has been resolved.

Operational verification:

- observer-db and RabbitMQ are running and healthy. observer-db needed an
  explicit start because its existing restart policy is `no`.
- All five previously exited containers remain stopped; no restart policies
  were changed. The full seven-container identity set is unchanged.
- The existing container image IDs and data mounts are unchanged. No images,
  volumes, queues, or application data were deleted.
- Both running services had no OOM kill and restart count zero throughout the
  monitored samples and final verification.
- All thirteen recorded Codex process IDs survived. The tmux server and all
  eleven pane identities are unchanged.

The 2048 MiB setting is retained for the current workload. The backup directory
contains `ROLLBACK.md`, original settings files, and a durable
`restore-memory.py` helper. Rollback stops Docker normally, restores only the
original memory keys to 4096 while offline, starts Docker, and verifies the
service set and health. If workloads have changed, snapshot their current
running/stopped states before rollback rather than replaying the old trial
inventory.

No repository runtime code changed in this Docker trial. Service/engine checks,
settings persistence, process preservation, three memory samples, documentation
links, and whitespace verification are the relevant checks; the already-passing
full repository suite was not repeated for an external settings change.
