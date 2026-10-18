# Spec: Reload devws after an update

- **Date:** 15-10-2026
- **Branch:** feat/devws-reload
- **Status:** Executed

## Context

After a `git pull` of devws, the running workspaces keep using the old code. tmux.conf is only
read when the server starts, and devws's own long-running scripts (the sidebar, the agent
spinner ticker, the minimized placeholders) keep running the version they started with. Bash
reads a script as it runs, so a running sidebar can even misbehave once its file changes. Today
the only fix is to restart tmux (`devws save`, `tmux kill-server`, `devws`), which also restarts
the agents and editors and loses the agents' conversations. Decisions: a **soft** reload that
keeps every agent, editor, terminal and database pane running; `./install.sh` runs it by itself
when a tmux server is up, so updating is `git pull && ./install.sh`.

## Requirements

1. `devws reload` applies an update to every open devws workspace without stopping the
   programs the user works in:
   - reloads `~/.tmux.conf` (formats, key bindings, colors);
   - restarts every sidebar pane (title `devws-picker`) with the current `session_picker.sh`;
   - restarts every minimized placeholder pane (role `minimized-agent` / `minimized-terminal`
     / `minimized-database`) with the current `minimized_placeholder.sh`, keeping its role and
     label;
   - restarts the spinner ticker of every workspace whose agent is working, so exactly one
     ticker per working workspace runs the current `agent_status.sh`.
2. The agent, editor, terminal and database panes keep running: same process, same output.
3. It prints what it reloaded, e.g. `devws: reloaded tmux.conf, 3 sidebar(s), 1 placeholder(s),
   1 spinner(s)`.
4. Without a tmux server it says there is nothing to reload and succeeds.
5. It works from inside a devws pane and from a terminal outside tmux.
6. `./install.sh` runs the same reload at the end when a tmux server is running, and says so;
   without a server it skips it silently.

## Constraints

- No `tmux kill-server` and no restore: a hard restart stays the manual three commands.
- Agents and editors are not restarted, so changes they read only at start (the agents' hook
  settings in `agent_settings.json`, the editor config, `dev.yml`'s per-session options such as
  `status-left`) apply to new or restarted agents/workspaces only. The README says so.
- Only devws workspaces are touched (sessions `dev-*`); other tmux sessions are left alone,
  apart from tmux.conf, which is global.
- Reuse what exists: `refresh_devws_pickers.sh`'s pane discovery, `agent_status.sh`'s ticker.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/reload_devws.sh` | Create — the reload (install.sh links it into `~/.tmuxinator`) |
| `shell/devws.zsh` | Modify — `devws reload` subcommand |
| `install.sh` | Modify — run the reload at the end when tmux is up |
| `scripts/test_devws_reload.sh` | Create — isolated tmux server test |
| `scripts/test_install_lifecycle.sh` | Modify — install runs the reload only when a server is up |
| `scripts/check.sh` | Modify — syntax-check and run the new test |
| `README.md` | Modify — document `devws reload` and the update flow |

## Success Criteria

- [x] After `devws reload`, sidebar and placeholder panes run new processes; agent, editor,
      terminal and database panes keep their process ids.
- [x] An option changed in tmux.conf is live after the reload.
- [x] A working workspace has exactly one ticker after the reload, and it is a new process.
- [x] Minimized placeholders keep their role and label, and restoring them (`C-b _`) still works.
- [x] `devws reload` with no server prints a message and exits 0.
- [x] `./install.sh` reloads when a server is up and does not call it otherwise (lifecycle test).
- [x] `./scripts/check.sh` passes.
