# Spec: The devws commands devAgentics drives

- **Date:** 03-11-2026
- **Branch:** feat/agent-commands (suggested)
- **Status:** Specified

## Context

devAgentics (`~/Projects/devAgentics`, design in its `docs/design.md`) runs
agent teams on top of devws: a planner, a leader, and executors in each
repo's worktree. It must open workspaces, start agents with role-specific
arguments, read their states, hand them text when they're idle, and read
account limits. It does all of this through a small, stable `devws` interface,
never through devws's internal scripts or tmux options directly.

Today that interface doesn't exist:
- **Opening a folder** (`bin/devws`, positional `<folder> <agent> <editor>`)
  always attaches or switches to it.
- **New panel** (`new_panel.sh`) splits whichever pane has focus, and
  `agent_runner.sh` starts a CLI with fixed arguments.
- **Delivering text** is the same two `tmux send-keys` lines repeated in three
  places (git_actions.sh, memory.sh, suspend_workspace.sh), and only
  git_actions.sh checks whether the agent is busy. It checks the workspace's
  combined state, not the pane's.
- **Pane states** live in tmux options (`@devws_pane_state`), and so do Claude
  limits (`@devws_limits_<name>`). Codex limits come from
  `header_tokens.sh limits`.
- **The sidebar** has no grouping.

## Requirements

1. **`devws open [--no-attach] [--task <id>] [--agent-args <file>] <folder> [agent] [editor]`**
   opens a workspace exactly as `devws <folder>` does today. With
   `--no-attach` it never attaches or switches, and it never prompts: missing
   values take the defaults. It prints the session name.
   - `--task <id>` sets the session option `@devws_task`.
   - `--agent-args <file>` passes extra command-line arguments to the main
     agent (see 3).
   - `devws <folder> …` keeps working unchanged.
2. **`devws agent spawn <session> <agent> [--beside <pane>] [--position right|left|below|above] [--agent-args <file>]`**
   adds an extra agent pane, like New panel, and prints its pane id.
   - `--beside` splits that pane instead of the focused one. Without it,
     today's choice applies.
   - The position defaults to right.
   - It never moves the user's focus.
3. **Agent arguments.** `agent_runner.sh` accepts `--args-file <file>`:
   NUL-separated arguments (one can be a prompt with line breaks), appended to the CLI's own arguments after devws's
   (claude: after `--settings … --append-system-prompt …`; codex: after its
   `-c` hooks). devws stays CLI-agnostic: it doesn't interpret them. The
   caller writes kind-specific flags (model, MCP config, initial prompt).
4. **`devws agent run <pane> <agent> [--agent-args <file>]`** restarts that
   agent pane with a new agent and arguments (`respawn-pane -k`), keeping the
   pane's place, role and size. It's used to hand a worktree's pane to the
   next agent.
5. **`devws agent state <pane>`** prints `working`, `waiting`, `done`, `idle`,
   or `gone` when the pane no longer exists.
6. **`devws agent send <pane> <text>`** types the text and Enter only when that
   pane's own state is `done` or `idle`. Otherwise it exits 3 with "busy
   (<state>)", or 4 when the pane is gone.
   - It becomes the shared helper the commit action, memory.sh and
     suspend_workspace.sh use, each keeping its current busy policy.
   - Multi-line text arrives as one message: it goes in through a bracketed
     paste (`load-buffer` + `paste-buffer -p`), not typed line by line.
7. **`devws agent mark <pane> waiting|done`** sets the pane's state through
   `agent_status.sh`, so the sidebar and notifications react ("needs you").
8. **`devws agent stop <pane>`** ends the agent (its CLI is asked to exit,
   then the pane is killed) and refreshes the workspace's state.
9. **`devws agent limits <agent>`** prints `<5h> <week>` used percentages for
   that account (`-` when unknown), from the sources the header already uses
   (`header_agents.sh`'s `limits_of`), loading `.env` itself.
10. **Sidebar grouping.** Workspaces with the same `@devws_task` are listed
    together under a `▸ <task id>` heading line; workspaces without one stay
    as they are.
11. **Restore.** Panes devAgentics marks with the pane option `@devws_team 1`
    are saved but not restarted by `restore_state.sh` (devAgentics brings them
    back itself); `@devws_task` is saved and restored.
12. **`devws worktree ensure <repo> <branch>`** prints the folder of
    `<branch>`'s worktree and opens nothing:
    - `<repo>` as `owner/repo` or a URL goes through `github_repo.sh ensure`
      plus `worktree.sh ensure` off the freshly fetched default branch, and
      the `.shared` files are linked;
    - `<repo>` as a local folder goes through `worktree.sh ensure` beside it.
    `devws worktree <branch> [agent] [editor]` keeps working unchanged.
13. **Docs:** `docs/guide/commands.md` gets an "Agent commands" section
    (`test_guide.sh` checks it). These commands are a stable interface:
    their output and exit codes only change with a note in the guide.

## Constraints

- No devAgentics concepts in devws beyond the opaque `@devws_task` id and
  `@devws_team` marker: no roles, orders or plans.
- bash 3.2 compatible scripts, matching the surrounding code. `bin/devws`
  stays zsh.
- Behaviour unchanged for existing users: `devws <folder>`, New panel, Commit
  (agent), the sidebar without tasks.
- Not in scope: OpenCode as an agent kind (devAgentics v2).

## Files Involved

| File | Action |
| ---- | ------ |
| `bin/devws` | Modify — `open` and `agent` subcommands |
| `config/tmuxinator/agent_cli.sh` | Create — spawn, run, state, send, mark, stop, limits |
| `config/tmuxinator/agent_runner.sh` | Modify — `--args-file` |
| `config/tmuxinator/dev.yml` | Modify — `agent_args` and `task` params |
| `config/tmuxinator/new_panel.sh` | Modify — `--beside`, no focus change, args file |
| `config/tmuxinator/git_actions.sh`, `memory.sh`, `suspend_workspace.sh` | Modify — use `agent_cli.sh send` |
| `config/tmuxinator/session_picker.sh` | Modify — group by task |
| `config/tmuxinator/save_state.sh`, `restore_state.sh` | Modify — `@devws_task`, skip `@devws_team` panes |
| `docs/guide/commands.md` | Modify — Agent commands |
| `scripts/test_agent_cli.sh` | Create — the commands in a test tmux server |
| `scripts/test_*` (picker, state, git actions) | Modify — grouping, restore, shared send |

## Success Criteria

- [ ] `devws open --no-attach --task t1 <folder> claude nvim` prints the
      session name, starts it detached, never prompts, and the session has
      `@devws_task t1`.
- [ ] `devws agent spawn` beside a given pane prints a pane id, keeps focus,
      and the stub agent receives the args file's arguments.
- [ ] `devws agent state` follows the hooks (working → done) and says `gone`
      for a killed pane.
- [ ] `devws agent send` delivers to a done pane (one paste, then Enter),
      exits 3 on a working one and 4 on a missing one.
- [ ] `devws agent run` swaps the agent in place; the layout is unchanged.
- [ ] `devws agent limits` prints the header's numbers for a claude and a
      codex account.
- [ ] The sidebar groups two task workspaces under one heading.
- [ ] A restore skips `@devws_team` panes and keeps `@devws_task`.
- [ ] Commit (agent) still works and still refuses a busy agent.
- [ ] `scripts/check.sh` passes.
