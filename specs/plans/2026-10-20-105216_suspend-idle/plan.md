# Plan: Freeze idle workspaces

- **Date:** 20-10-2026
- **Branch:** feat/suspend-idle
- **Status:** Executed (live check pending)
- **Spec:** [spec.md](spec.md)

## Approach

### 1. Runners keep a quiet bash above the program
- `config/tmuxinator/agent_runner.sh` and `editor_runner.sh`: drop `exec` before
  claude/codex/nvim/editor. Comment why: the waiting bash shields the shell from a freeze.
- Side effect: tmux's `pane_current_command` reads "bash" for these panes. Two readers:
  - `config/tmux/renew_env.sh` (`C-b $`) types into panes running a shell. It gets
    `#{@devws_role}` and skips the agent, editor, extra-agent and extra-editor panes.
  - The `agent_pane` and `editor_pane` fallback in `workspace_actions.sh` only applies to
    panes without a role. No change.
- Workspaces already running keep their old exec'd programs. Those panes aren't frozen until
  the agent is restarted or the workspace is reopened. The README says so.

### 2. `suspend_workspace.sh`: freeze below the devws script
- In each freezable pane (roles agent/extra-agent/editor/minimized-*, and the sidebar), find
  the topmost process whose command runs a `/.tmuxinator/` script. That's the runner, the
  sidebar's `session_picker.sh`, or a placeholder.
  - Freeze only that process's descendants, never the script itself and never the pane's
    shell: the program, language servers, MCP servers, fzf.
  - A pane with no such script is left alone. That covers terminals, a pane typed by hand,
    and an extra Neovim (`new_panel.sh` runs `nvim .` directly).
- Keep the rest of the draft:
  - The idle rule: unattached, agent unset or `done`, and `DEVWS_SUSPEND_AFTER` minutes
    (default 15; 0 turns it off) since the later of `@devws_seen` (or `session_created`) and
    `@devws_agent_state_since`.
  - The PID and start-time file, keyed by session id. SIGSTOP goes parents first; SIGCONT
    goes children first, only to PIDs whose start time still matches.
  - `@devws_frozen`, and `@devws_seen` stamped by the sweep and by resume.
  - Self-resume if a client attached while freezing.
  - `resume <session|--all|--closed>`.
- `new_panel.sh`: start the extra Neovim through `editor_runner.sh`? No: the editor runner
  claims the main editor role and socket. Extra Neovims simply stay unfrozen.

### 3. Triggers (from the draft)
- `header_ticker.sh`: `in_background suspend "$scripts_dir/suspend_workspace.sh" sweep` in
  the every-minute slot. It runs even while the header is hidden.
- `tmux.conf`, indexed hooks so a reload replaces them:
  - `client-session-changed[1]` and `client-attached[1]` run `resume #{q:session_id}`.
  - `session-closed[1]` runs `resume --closed`.
- `reload_devws.sh` runs `resume --all` before respawning the sidebars.

### 4. Visible state and docs
- `session_picker.sh` `workspace_rows` adds `#{@devws_frozen}` to its list-sessions format
  and shows ` · paused` (dim) on the agent line.
- README, "Paused workspaces":
  - when a workspace freezes, and what stays running (terminals, panes you typed in, extra
    Neovims);
  - `DEVWS_SUSPEND_AFTER`, and that memory stays used;
  - that a workspace started before this update freezes only its sidebar until its agent and
    editor restart.
- `scripts/check.sh`: add `bash -n` and a run of `test_suspend_workspace.sh`.

## Files

| File | Change |
| --- | --- |
| `config/tmuxinator/suspend_workspace.sh` | new (drafted): freeze below the devws script |
| `config/tmuxinator/agent_runner.sh`, `editor_runner.sh` | no `exec` |
| `config/tmux/renew_env.sh` | skip agent and editor panes |
| `config/tmuxinator/header_ticker.sh` | sweep every minute |
| `config/tmux/tmux.conf` | resume hooks (drafted) |
| `config/tmuxinator/reload_devws.sh` | `resume --all` first |
| `config/tmuxinator/session_picker.sh` | `· paused` |
| `README.md`, `scripts/check.sh`, `scripts/test_suspend_workspace.sh` | docs, checks, tests |

## Verification

`scripts/test_suspend_workspace.sh`, isolated tmux server. It uses the real
`agent_runner.sh` and `editor_runner.sh` with stub claude and nvim (`exec sleep 600`), typed
into `zsh -f -i` panes as tmuxinator does, plus the real `session_picker.sh` with a stub fzf.
- The sweep freezes an idle workspace:
  - the stub agent, the stub editor and the sidebar's children are in state `T`;
  - zsh, the runners and the terminal are not;
  - `@devws_frozen` is 1.
- Resume: everything runs again, the runner is still the pane's foreground, and the capture
  has no "suspended".
- Working and waiting agents, a recently seen workspace, a recently finished agent, an
  attached workspace and `DEVWS_SUSPEND_AFTER=0` all stay unfrozen.
- tmux.conf's hooks: a client switching to a frozen workspace resumes it.
  `kill-session` leaves no stopped process.
- The runners no longer exec: the stub's parent is the runner's bash.
- `renew_env.sh` doesn't type into an agent pane.

Then run `scripts/check.sh`. Live, with the user's OK:
1. Link the new script (`./install.sh` or a symlink) and run `devws reload`.
2. Restart one workspace's agent (and reopen its editor), leave it for the timeout (or set
   `DEVWS_SUSPEND_AFTER=1`).
3. Check `ps -o state` and the sidebar's `· paused`.
4. Switch back and type in its agent.
