# Spec: Agent alerts that name their workspace and jump there on click

- **Date:** 24-10-2026
- **Branch:** feat/suspend-idle (or a new `feat/agent-alerts` off it)
- **Status:** Executed (awaiting a real click check)

## Context

When an agent in a workspace you are not looking at needs you or is done,
`agent_status.sh` (`notify`) posts a macOS notification through `osascript`:

```
devws · work
claude is done
```

It is hard to tell at a glance which workspace sent it, and:

- it always names the workspace's main agent (`@devws_agent` on the session),
  even when an extra agent opened with New panel is the one that finished;
- it does not say which project or branch the workspace is on, which matters
  now that worktree workspaces put several branches of one repo side by side;
- clicking it does nothing: you still have to find the workspace yourself
  (or press `C-b a`).

## Requirements

1. **The title names the workspace and the state:** `work ✓ done`,
   `work ● needs you` (the workspace name without `dev-`, as today).
2. **The subtitle names the project folder and branch:** `orbit-back · feat/login`,
   from the workspace's `@devws_root` and its current git branch. Without a
   branch (not a repo, detached HEAD) it is the folder alone.
3. **The message names the agent that changed state**, from the pane that
   reported it (`agent_status.sh`'s `[pane]`):
   - the main agent: `claude is done` / `claude needs you`;
   - an extra agent (New panel): `codex in a new panel is done` /
     `codex in a new panel needs you`, with that pane's own `@devws_agent`.
4. **Clicking the alert takes you there:** it switches the most recently
   active tmux client to that workspace, restores the agent group when it is
   minimized, focuses that agent's pane, and brings iTerm2 to the front. A
   workspace or pane that has gone since does no harm: it focuses what is
   left, or does nothing.
5. **A newer alert from the same workspace replaces the older one** in
   Notification Center, so a workspace does not pile up stale alerts.
6. **terminal-notifier does the click.** It is added to the Brewfile. Without
   it, the alert falls back to `osascript`, with the same title, subtitle and
   message, just not clickable.
7. **Unchanged:** `DEVWS_NOTIFY` (1 / `waiting` / 0), no alert for a workspace
   a client already shows, alerts only when the workspace's state changes, no
   sounds, and the hook that reported the state never waits on the alert.

## Constraints

- **When an alert fires does not change.** It still follows the workspace's
  summed state (waiting, else working, else done): an extra agent that
  finishes while the main one still works sends no alert, as today. Alerting
  per agent is a separate decision.
- The click reuses the logic `jump_agent.sh` already has for focusing an agent
  pane (restore the group, switch client, select window and pane), moved into
  one place both use, instead of a second copy.
- The click runs outside tmux (from terminal-notifier, with a minimal `PATH`),
  so it addresses the tmux server by its socket and tmux by its full path.
- iTerm2 is the terminal brought forward (devws already sets up iTerm2's
  keys); no new setting for other terminals.
- `DEVWS_NOTIFY_COMMAND` stays the test hook; it receives the title, the
  subtitle, the message and the click's target (session and pane), so tests
  check the content without posting real notifications.
- No new options in `.env`.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/agent_status.sh` | Modify — build title, subtitle and message from the workspace and the reporting pane; post with terminal-notifier (click, group) or fall back to osascript |
| `config/tmuxinator/focus_agent.sh` | Create — take a client to a workspace's agent pane (restore, switch, focus); usable from outside tmux |
| `config/tmuxinator/jump_agent.sh` | Modify — use `focus_agent.sh` for the switch and focus |
| `Brewfile` | Modify — add `terminal-notifier` |
| `scripts/test_agent_status.sh` | Modify — the alert's title, subtitle, message and target, for the main and an extra agent |
| `scripts/test_jump_agent.sh` | Modify — `focus_agent.sh` from outside tmux: switches the client, restores a minimized group, tolerates a gone pane |
| `scripts/check.sh` | Modify — syntax-check and require the new script to be executable |
| `README.md` | Modify — describe the new alert and the click |

## Success Criteria

- [x] An alert for the main agent reads `work ✓ done` / `<folder> · <branch>` / `claude is done`.
- [x] An alert caused by an extra agent names that agent and says it is in a new panel.
- [x] Outside a git repo the subtitle is the folder alone.
- [x] Running the alert's click command from a shell with no `TMUX` and a bare `PATH` switches the most recent client to the workspace and focuses the agent pane, restoring a minimized group.
- [x] The click command for a pane that no longer exists still switches to the workspace and exits 0.
- [x] `C-b a` (`jump_agent.sh`) behaves as before.
- [x] Without terminal-notifier, the osascript alert carries the same title, subtitle and message.
- [x] `DEVWS_NOTIFY`, the on-screen rule and the one-alert-per-change rule still hold (existing tests).
- [x] `make check` passes.
