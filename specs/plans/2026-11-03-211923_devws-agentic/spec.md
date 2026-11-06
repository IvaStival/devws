# Spec: `devws agentic`, a workspace for an agent team

- **Date:** 03-11-2026
- **Branch:** feat/agentic (suggested, off feat/agent-commands until that merges)
- **Status:** Specified

## Context

devAgentics (`~/Projects/devAgentics`) runs agent teams: a leader the user
talks to, and custom agents (backend, frontend, tests…) the leader gives
orders to. The user wants one devws command that opens the right workspace for
it, as a default template: the leader, a log, and room for the agents the
leader opens when it needs them. The user talks to the leader; the leader
talks to the others.

`feat/agent-commands` (af3dd3d) already gives the leader its hands: `devws
open --task`, `devws agent spawn|run|state|send|mark|stop|limits`, `devws
worktree ensure`, sidebar grouping. This spec adds the command and the layout.

Decisions (grilling, 03-11-2026):
- Only the leader starts with the workspace; it opens agent panels when
  needed.
- Agent definitions are role files in `~/.devagentics/roles/*.md`. Only
  devAgentics reads them; devws never does.
- Layout: the leader takes the left half, full height, with a short log
  under it; the agents are a grid on the right; no editor unless asked.
- The repo idea is the one devws already has: a clone in `~/.devws`
  (`<owner>/<repo>/.bare`) and a worktree per task, off the fresh default
  branch.

## Requirements

1. **`devws agentic [--editor] [<repo or folder> [<branch>]]`** opens the
   agentic workspace, through `devws open --no-attach --template agentic`
   (new: `--template <name>` starts another tmuxinator project, `--set
   <key=value>` adds a setting for it), then switches or attaches to it; from
   a script, with no terminal, it only prints the session's name.
   - No argument: the start screen (`github_workspace.sh`: owner, repo, the
     branch's kind and a few words), then the workspace on the new worktree.
   - `owner/repo` or a URL plus a branch: `devws worktree ensure`, then the
     workspace on that worktree.
   - A folder: the workspace on it as it is.
   - The session is `dev-<folder name>`, as every workspace, so the sidebar
     lists it; it carries `@devws_template agentic`.
2. **The leader is devAgentics's.** devws calls
   `devagentics task start <folder>` (`DEVWS_AGENTIC_COMMAND` replaces the
   program, for tests), which prints three lines: the task id, the leader's
   agent (a `DEVWS_AGENTS` name) and the path of its arguments file. devws then
   opens the workspace with `--task <id>` and starts the leader with that agent
   and file. Without devagentics on PATH, `devws agentic` says how to install
   it and exits 1; nothing is opened.
3. **The template** (`config/tmuxinator/agentic.yml`, a tmuxinator project
   beside `dev.yml`): the sidebar, the leader (role `agent`, focused, the
   full width beside the sidebar) and under it a log pane (role `team-log`,
   about 8 lines) running `devagentics watch <task id>`, with a `Log` header.
4. **Team panels.** `devws agent spawn <session> <agent> --place team
   [--label <text>]` adds a panel to the team area right of the leader:
   - the first panel takes the right half, so the leader and the log keep the
     left half;
   - the next ones split the team area into a grid (the largest panel splits
     in two, alternating right and below);
   - the focus stays where it is, as spawn does today.
5. **Role labels.** A panel with the pane option `@devws_label` shows it in its
   header in place of "Agent" (`Backend · claude`), with the state icon and
   the `needs you` / `done` badge as now. The leader's header says `Leader`.
6. **No editor.** `minimize_panes.sh`, `toggle_layout.sh` and `open_database.sh`
   fail without an editor pane, and New panel expands the agent group first.
   In a workspace with no editor pane they do nothing and say so, and the
   menu hides the Layout and Database items there (`shown()` condition
   `editor`); New panel opens its panel without expanding anything.
7. **`--editor`** adds a Neovim panel on the folder (as New panel's Neovim,
   role `extra-editor`, not linked to an agent) under the log.
8. **Restore.** The workspace is saved with its template (and a `-` for a task
   it does not have yet, as an empty field would not survive being read). A
   restart does not rebuild an agentic workspace by itself: `restore_state.sh`
   skips it and says so, naming its task. devAgentics brings the task back
   (`task resume`), as the leader and the agents are its to restart.
9. **The menu and the guide.** Workspace tab: `◈ Open an agent team` runs
   `devws agentic`. A guide page `docs/guide/agentic.md` (what it is, the
   command, the layout, that the leader opens the panels) with its Help item;
   `commands.md` lists the command.

## Constraints

- devws knows nothing of roles, orders or plans: a panel's label and the
  task id are plain strings it displays and groups by.
- The leader and the panels are started with the commands that exist
  (`devws open`, `devws agent spawn`); no second way to start agents.
- Bash 3.2 for scripts, zsh for `bin/devws`; the existing `dev` workspace and
  every current flow behave as before.
- Not in scope: the devAgentics side (`devagentics task start`, `watch`, the
  leader's role and tools) and automatic restore of a team.

## Files Involved

| File | Action |
| ---- | ------ |
| `bin/devws` | Modify — `agentic` subcommand |
| `config/tmuxinator/agentic.sh` | Create — choose the folder, call devagentics, open the workspace |
| `config/tmuxinator/agentic.yml` | Create — the template |
| `config/tmuxinator/agent_cli.sh`, `new_panel.sh` | Modify — `--place team`, `--label`, no group expand without an editor |
| `config/tmuxinator/minimize_panes.sh`, `toggle_layout.sh`, `open_database.sh` | Modify — no editor pane is not an error |
| `config/tmuxinator/devws_menu.sh` | Modify — `Open an agent team`; hide editor-only items |
| `config/tmux/tmux.conf` | Modify — the header shows `@devws_label` |
| `config/tmuxinator/save_state.sh`, `restore_state.sh` | Modify — the template; skip agentic on restore |
| `docs/guide/agentic.md`, `commands.md`, `README.md` | Create / modify |
| `scripts/test_agentic.sh` | Create — the command, the layout and the grid with a stub devagentics |

## Success Criteria

- [ ] With a stub `devagentics`, `devws agentic <folder>` opens `dev-<name>`
      detached-testable, with the leader pane running the stub's agent and
      file, the log pane running `watch <id>`, `@devws_task` set, and no
      editor.
- [ ] `devws agentic owner/repo feat/x` creates the worktree in `~/.devws` and
      opens it; with no `devagentics`, it exits 1 and opens nothing.
- [ ] Three `agent spawn --place team` calls leave the leader and log on the
      left half and the three panels in a grid on the right, focus unchanged.
- [ ] A panel's header shows its label and state.
- [ ] Layout, Database and New panel behave in a workspace with no editor.
- [ ] A restart leaves the agentic workspace for devAgentics and says how.
- [ ] `scripts/check.sh` passes, including the guide test.
