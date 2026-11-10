# Spec: Keep devws responsive with multiple active agents

- **Date:** 08-11-2026
- **Branch:** feat/agentic
- **Status:** Executed — remaining iTerm2 bottleneck documented

## Context

The user reports that devws becomes slow with more than one running agent.
Initial investigation found high terminal rendering CPU and avoidable work in
tool hooks. Measurements are recorded in [measurements.md](measurements.md).
The relative contributions to the user's perceived slowdown are not yet proven.
An optional question about the affected interaction remains unanswered; for now,
measure both terminal responsiveness and tool hook overhead.

## Requirements

1. Provide a repeatable performance probe for one, two, and four agents. Report
   tmux query latency, terminal/tmux CPU, hook duration, repository scans, and
   redraw counts separately. Use synthetic agents for repeatable comparisons;
   do not start paid agent sessions or expose prompts, logs, commands, or secrets.
2. Skip repository snapshots and follow processing when there is no eligible
   linked editor, or following is off. Preserve edits, shell-generated changes,
   read-follow behavior, change marks, and all enabled follow modes. Handle mode
   changes between pre/post hooks without applying a stale snapshot.
3. Measure terminal redraw cost with controlled comparisons before changing
   rendering behavior. Reduce redundant/global updates where the comparison
   demonstrates a benefit; retain timely working/waiting/done indicators and
   visible workspace information. Do not promise that hook optimization alone
   fixes terminal rendering CPU.
4. Bound repeated usage-statistics work. Cache unchanged transcript results and
   avoid reparsing unchanged historical data every refresh. Preserve usage,
   account limits, reset behavior, and cost calculations. Correctly handle log
   append, truncation, replacement, and malformed/incomplete trailing entries.
5. Use tests that assert unnecessary work is avoided, alongside behavior tests.
   Keep wall-clock performance measurements separate from deterministic pass/fail
   tests to avoid flaky timing thresholds.

## Constraints

- Active agents must keep running. Do not stop, freeze, restart, or discard their
  sessions as a performance workaround.
- No changes to agent security settings, prompts, application workload, or user
  repositories for benchmarking. Isolate fixtures, temporary indexes, and tmux
  servers.
- Preserve existing uncommitted panel-recovery changes and untracked images.
- No transcript contents or credentials in probe output or saved measurements.
- Scope implementation to costs supported by measurements. Treat memory/swap as
  context; current swap allocation alone does not establish active memory pressure.
- Keep benchmarks comparable: same fixture, warm-up, repeated samples, same
  number of tool events, and separately report total work and elapsed time.
- Save phase artifacts under `specs/plan/` as the project's AGENTS.md directs.

## Files Involved

Candidate implementation files; narrow the final list after controlled profiling.

| File | Action |
| ---- | ------ |
| `scripts/measure_load.sh` | Add repeatable CPU, latency, and isolated hook measurements |
| `config/tmuxinator/agent_follow_hook.sh` | Gate unnecessary follow work before snapshots |
| `config/tmuxinator/agent_changes.sh` | Adjust snapshot cleanup only if required by gating |
| `config/tmuxinator/editor_open.sh` | Reuse eligibility semantics if needed |
| `config/tmuxinator/agent_state.sh`, `agent_status.sh` | Avoid repeated status work if measured |
| `config/tmuxinator/header_ticker.sh`, `agent_spinner.sh` | Reduce verified redundant redraws |
| `config/tmuxinator/refresh_devws_pickers.sh` | Coalesce sidebar reloads if measured |
| `config/tmuxinator/header_tokens.sh` | Cache unchanged usage and account-log work |
| `scripts/test_agent_follow.sh` | Cover disabled/unlinked follow and preserved enabled behavior |
| `scripts/test_agent_status.sh`, `scripts/test_header.sh` | Verify bounded updates and usage correctness |
| `docs/architecture/` | Document any new caching or event flow introduced |

## Success Criteria

- [x] Disabled/unlinked follow performs zero repository snapshot scans for one,
      two, and four simultaneous agents.
- [x] Enabled follow still identifies actual changes, including shell edits.
- [x] Repeated statistics refreshes avoid rereading unchanged historical logs;
      appends, truncation, and replacement produce correct results.
- [x] Before/after measurements identify which changes reduce hook cost and
      which reduce terminal CPU; record any remaining rendering bottleneck.
- [x] A controlled rendering comparison informs the final rendering change,
      with no loss of agent state information or interactive functionality.
- [x] Relevant regression tests and `make check` pass, running the full check
      once with the required tmux access.
- [x] Existing agent processes remain running throughout validation.

## Phase Checkpoint

The user approved the spec, plan, and execution on 08-11-2026. See [plan.md](plan.md).
