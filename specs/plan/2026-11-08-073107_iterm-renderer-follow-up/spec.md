# Spec: Reduce terminal rendering cost while preserving running agents

- **Date:** 08-11-2026
- **Branch:** feat/agentic
- **Status:** Solved on 09-11-2026 — the terminal was replaced by Ghostty
- **Preceding work:** [devws performance measurements](../2026-11-08-000752_multi-agent-performance/measurements.md)

## Context

The devws overhead changes are implemented, validated, and activated. The earlier
live comparison still found iTerm2 consuming roughly one CPU core with the header
ticker both running and held. A stack sample placed 140 of 156 main-thread
samples under annotation-range processing in its Metal renderer.

The installed app is iTerm2 3.4.20. On 08-11-2026, the
[official downloads page](https://iterm2.com/downloads.html) lists 3.7.7, built on
03-11-2026, as the recommended stable release for macOS 13+. That makes an update
a reasonable experiment; it does not establish that this particular renderer
problem is fixed by that version.

A fresh client is a separate experiment. The repository's tmux configuration
notes that terminal-capability overrides apply to newly attached clients. A
fresh terminal also has different accumulated terminal state. Updating and
relaunching in one operation would confound these effects with a version change.

## Requirements

1. Recheck the current live CPU and profile before attributing an earlier sample
   to the present workload. Record app version, client dimensions, visible
   workspace, focus, active agents, and measurement conditions without recording
   terminal contents or process command arguments.
2. Compare the existing client with a fresh client on iTerm2 3.4.20, attached
   directly to the same existing tmux session. Match geometry, visible panes,
   focus, and workload as closely as possible. Avoid comparing two simultaneously
   visible clients against one. Report a combined fresh-client/state effect;
   do not claim to have isolated scrollback from capability negotiation.
3. Preserve the tmux server, sessions, pane identities, and agent processes.
   Direct client detach/attach must not become workspace restore, pane respawn,
   conversation restart, or terminal history clearing. Verify live session
   survival settings before any client is detached.
4. If the renderer cost remains significant, stage the current stable iTerm2
   release from the official source. Recheck version and macOS compatibility
   at execution time, and verify the downloaded application's signature and
   release identity before using it. Keep the current app available for rollback.
5. Preserve user preferences, profiles, fonts, colors, global/profile key maps,
   custom preference-folder settings, and devws's Shift+Enter mapping. Backups
   containing personal settings stay private and outside Git. Avoid unrelated
   shell-integration installation or default-setting changes during comparison.
6. Separate running-agent preservation from terminal-local state preservation.
   Inventory non-tmux terminal jobs and session-local unsaved state before a
   terminal cutover. Do not assume keeping tmux alive preserves iTerm2's own
   scrollback, notes, or saved windows. Keep the original client available until
   the comparison succeeds; make any necessary app restart explicit in the plan.
7. Compare fresh clients on old and new versions under matched conditions, with
   at least three samples per condition and the same synthetic output/state
   schedule where feasible. Include real multi-agent observation separately.
   Record terminal CPU, tmux query latency, and typing/scrolling/menu behavior.
   Query latency alone must not be presented as screen-response latency.
8. If an update/fresh client does not materially help, capture another stack
   profile and run one reversible renderer-setting comparison at a time, restoring
   the prior setting afterward. Do not delete annotations or histories to force
   a positive result. Narrow subsequent work to evidence from the new profile.
9. Keep or roll back the terminal change based on measurements and workflow
   checks. Document the outcome, remaining limitation, and exact adoption/revert
   procedure. Do not claim success from a single low-CPU sample.

## Constraints and decisions

- This phase is specification only: no app download, installation, preferences
  mutation, terminal restart, or live-client switching is authorized by writing it.
- Continue using the optimized devws runtime; leave its existing uncommitted
  changes, earlier panel fix, and untracked images intact.
- Use direct tmux attachment for controlled comparisons; invoking the devws
  startup picker or restore path adds unrelated work.
- Preserve shell history, tmux history, agent conversations, saved workspace
  records, terminal notes, and scrollback. Clearing history is outside scope.
- Do not run `install.sh --iterm-keys` for a renderer experiment. The existing
  mapping should survive without rewriting user preferences.
- Repository code changes are limited to a demonstrated gap in measurement or
  compatibility. Add a regression first if such a code change is needed. No
  generic terminal-management framework or alternative-terminal migration.
- Application/profile operations belong to the later approved execution plan.
  Treat any rollback of migrated preferences as a separate verified operation;
  restoring an older executable alone may not restore the prior configuration.

## Files and resources involved

| Resource | Role |
| --- | --- |
| `scripts/measure_load.sh`, `scripts/measure_load.py` | Reuse CPU/latency probes; extend only for a measured gap |
| `config/tmux/tmux.conf` | Inspect terminal-capability overrides and session survival settings |
| `scripts/setup_iterm_keys.sh` | Read the mapping contract; no mapping rewrite planned |
| `docs/architecture/agent-performance.md` | Update measured renderer guidance after execution |
| `/Applications/iTerm.app` | Inspect current version; stage/update only during approved execution |
| iTerm2 live and custom preferences | Preserve privately; inspect only relevant non-secret metadata |
| Existing tmux sessions and pane processes | Preserve and verify before/after client changes |
| This specification folder | Store the approved plan and redacted measurement record |

## Success criteria

- [ ] Fresh-client comparison is measured separately from the version upgrade.
      Not done: the fresh attach froze iTerm2; no upgrade was attempted.
- [x] The selected client/version reduces renderer CPU consistently, or the
      remaining bottleneck is identified with a new profile and next action.
- [ ] Typing, scrolling, menus, pane switching, editor following, and Shift+Enter
      work after cutover; terminal display corruption does not recur.
- [ ] Existing agent processes and pane/session identities survive the change.
      Not met: the force-quit lost the team's group workspaces; the user restarted the team.
- [ ] Preferences and terminal-local state are preserved, with any unavoidable
      cutover limitation identified before proceeding.
- [ ] The old application/configuration can be restored using the recorded rollback.
- [ ] Repository checks are run only if repository code changes; documentation
      links and whitespace are checked for documentation-only changes.

## Phase checkpoint

The user approved this scope on 08-11-2026. The detailed [plan](plan.md) is ready
for execution approval. No terminal changes have been made in this phase.
