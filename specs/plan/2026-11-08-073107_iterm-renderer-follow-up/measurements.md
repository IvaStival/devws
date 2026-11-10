# Terminal renderer follow-up measurements

Date: 08-11-2026. Execution approved by the user. No application update,
restart, preferences mutation, or automatic client switching has occurred.

## Original-client baseline

The installed version is iTerm2 3.4.20. One focused client displays `dev-devws`
at 238 columns by 65 rows. Four tmux sessions and eleven panes were present.
`exit-unattached` and all four sessions' effective `destroy-unattached` values
were `off`.

Three consecutive ten-second samples used the existing `measure_load.py` live
probe. CPU is normalized to one core; query latency is not screen latency.

| Sample | iTerm2 CPU | tmux CPU | Query median | Query p95 |
| --- | --- | --- | --- | --- |
| 1 | 92.3% | 7.4% | 9.60 ms | 16.96 ms |
| 2 | 94.2% | 7.2% | 8.54 ms | 31.82 ms |
| 3 | 92.1% | 5.9% | 7.53 ms | 11.26 ms |

The current symptom still includes sustained high terminal CPU. Agent activity
varied between samples. No paid work was started to produce a workload.

## Preservation preparation

A private backup directory was created under
`~/.devws-state/terminal-upgrade/2026-11-08-161210/`, with `0700` permissions.
It contains an exported effective preferences domain, a copy of the preferences
file, and private process/client/pane inventories. Backup files containing
settings use `0600` permissions and are outside Git.

The existing Shift+Enter mapping matches action 11 with text `0x1b 0x0d`.
No key mappings or other preferences were changed. Terminal-local notes,
scrollback, and unsaved GUI state have not been verified or modified.

## Computer-control limitation

The computer-control tool rejected access to `com.googlecode.iterm2` with:
“Computer Use is not allowed to use the app 'com.googlecode.iterm2' for safety
reasons.” No alternative UI automation was attempted.

Read-only process and tmux metadata collection and private backups remain
available. Opening and switching the fresh iTerm2 window requires a manual user
action. The user was asked to open a fresh matching window and run:

```bash
tmux attach-session -d -t '=dev-devws'
```

The command attaches a fresh client and detaches the old client without closing
the old window or restarting tmux agents. Fresh-client measurements remain
pending until that action is observed or confirmed. Any later GUI quit/relaunch
or settings comparison also requires manual interaction while the restriction
remains in effect.

## Resolution

Date: 09-11-2026.

Attaching a fresh client (`tmux attach-session -d -t '=dev-devws'`) froze iTerm2
3.4.20, and the user force-quit it. The tmux server did not keep the agent
team's group workspaces; the user restarted the team afterwards.

After an iTerm2 restart, with a single agent running, iTerm2 used 29–30% of one
core. That sample had a smaller workload than the baseline, so it does not
separate a fresh window from less output.

The user then switched to Ghostty 1.4.0, configured with the iTerm2 profile's
colors and key mapping. With the full team running again (four workspaces, two
Codex and three Claude agents), the `measure_load.sh live --seconds 8` probe
measured:

| Terminal | Terminal CPU, one core = 100% |
| --- | --- |
| iTerm2 3.4.20, baseline above | 87–110% |
| Ghostty 1.4.0, two samples | 10.9%, 8.2% |

The user reports the workspace is fast now. The terminal renderer was the
dominant cost; no iTerm2 upgrade was needed. Memory pressure (8 GiB, about
3.6 GB swap) remains a separate limit on how many agents run at once.
