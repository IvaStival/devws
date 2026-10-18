# Plan: Reload devws after an update

- **Date:** 16-10-2026
- **Branch:** feat/header-dashboard (follows the header work, which it reloads)
- **Status:** Executed
- **Spec:** [spec.md](spec.md)

## Context

After `git pull`, running workspaces keep the old tmux.conf and the old copies of devws's
long-running scripts. The spec asks for `devws reload`: apply an update to every open workspace
without restarting the agents, editors, terminals or databases, and run it from `./install.sh`
when a tmux server is up.

Since the spec, the header ticker restarts itself whenever tmux.conf is sourced
(`header_ticker.sh --start` in tmux.conf), so reloading tmux.conf already covers it.

## Approach

`config/tmuxinator/reload_devws.sh` (new; install.sh links every file there):

1. No tmux server → prints `devws: no tmux server, nothing to reload`, exits 0.
2. `tmux source-file ~/.tmux.conf` — formats, bindings, colors; restarts the header ticker.
3. Sidebars: every pane titled `devws-picker` in a `dev-*` session is respawned
   (`respawn-pane -k`) with `~/.tmuxinator/session_picker.sh`, the command it runs.
4. Placeholders: every pane with role `minimized-*` is respawned with `respawn-pane -k` and no
   command, which reruns its own start command (`minimized_placeholder.sh <label>`) — so it
   runs the current script with its label, and keeps its pane options (role, hidden pane).
5. Spinners: in every `dev-*` session whose `@devws_agent_state` is `working`, the ticker in
   `@devws_agent_ticker` is stopped and the option cleared, then `agent_status.sh <session>
   working` starts a fresh one (its own start path, reused).
6. Prints `devws: reloaded tmux.conf, N sidebar(s), N placeholder(s), N spinner(s)`.

Agent, editor, terminal and database panes are never touched.

`shell/devws.zsh`: `devws reload` runs it. `install.sh`: at the end, when `tmux` exists and a
server answers, runs it and reports; otherwise nothing.

## Files

| File | Action |
| --- | --- |
| `config/tmuxinator/reload_devws.sh` | Create |
| `shell/devws.zsh` | Modify — `devws reload` |
| `install.sh` | Modify — reload when a server is up |
| `scripts/test_devws_reload.sh` | Create — isolated tmux server |
| `scripts/test_install_lifecycle.sh` | Modify — reload only with a server |
| `scripts/check.sh` | Modify — syntax-check and run the new test |
| `README.md` | Modify — `devws reload`, the update flow, what needs a restart |

## Verification

`scripts/test_devws_reload.sh` on an isolated server with a workspace holding a sidebar, an
agent, an editor, a minimized terminal placeholder and a working agent state:
- sidebar and placeholder panes have new pids; agent and editor keep theirs;
- the placeholder keeps its role, label and hidden pane, and restoring it works;
- an option written to the sourced tmux.conf is live after the reload;
- exactly one spinner ticker runs afterwards, and it is a new process;
- a non-`dev-*` session's panes are untouched;
- with no server: the message, exit 0.

`test_install_lifecycle.sh`: install calls the reload when a (stub) server answers, not
otherwise. Then `scripts/check.sh`, and `devws reload` on the real server.
