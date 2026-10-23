# Spec: Freeze idle workspaces

- **Date:** 20-10-2026
- **Branch:** feat/suspend-idle
- **Status:** Executed (live check pending)

## Context

The user wants devws lighter by suspending the workspaces where nothing is happening. Measured
on 5 workspaces:
- An idle agent uses about 0.3% CPU and 250 MB.
- Each sidebar uses about 0.3%, because it polls 7 times a second even when unseen.
- The header uses about 2% in total.

Freezing saves CPU, not memory.

## Requirements

1. A workspace freezes only when nothing is running an action. That means:
   - no client shows it;
   - its agent is idle or done, never working or needing you;
   - no shell runs below its agent or editor (a background job the agent started, a
     `:terminal` in Neovim);
   - it has been unseen, and its agent unchanged, for `DEVWS_SUSPEND_AFTER` minutes (default
     15; 0 turns the feature off).
2. Frozen: the agents, editor, sidebar and placeholders, with what they started. Never the
   terminals.
3. Switching to it, attaching to it or reloading devws brings it back at once, unchanged.
   Closing it leaves nothing stopped.
4. The sidebar shows `· paused` for a frozen workspace.

## Constraints

- tmux continues a pane's own process when it stops. An interactive zsh that sees its program
  stop takes the pane back ("zsh: suspended"). Both were verified with probes.
  - So the program must be frozen below a non-interactive bash (the runner), which waits
    without job control.
  - So `agent_runner.sh` and `editor_runner.sh` no longer `exec`.
- `C-b $` (renew_env.sh) types into panes running bash or zsh, so it must skip the agent and
  editor panes.

## Files Involved

See [plan.md](plan.md).

## Success Criteria

- [x] An idle workspace freezes. Its terminal, shells and runners keep running.
- [x] Resume returns the program to the foreground with no "suspended".
- [x] Working, waiting, recent and attached workspaces stay unfrozen, and so does
      `DEVWS_SUSPEND_AFTER=0`.
- [x] tmux.conf's hooks resume on switch, and closing a workspace leaves nothing stopped.
- [x] The header's ticker sweeps, and `devws reload` resumes everything first.
- [x] `scripts/check.sh` passes.
- [ ] Checked in a real workspace.
