# Spec: Team role settings in the menu, and Codex agents that start unattended

- **Date:** 09-11-2026
- **Branch:** feat/agentic
- **Status:** Executed

## Context

An agent team's roles are TOML front matter in `~/.devagentics/roles/*.md`,
overlaid by a repo's `.devws/roles/`. Today the only way to change a role's
model, effort or approvals is to edit those files by hand. The user wants a
place in the devws menu for it (for example, to make the Floor run Sonnet;
`hammer-claude` was switched to `claude-sonnet-5-5` by hand on 09-11-2026).

Codex team agents also stop and wait for the user. Two causes were verified:

1. **Folder trust.** Codex asks "Trust this folder?" when it starts in a folder
   with no `[projects."<folder>"] trust_level = "trusted"` entry in its
   `config.toml`. Each task gets new lane worktrees under
   `~/.devagentics/tasks/<task>/lanes/`, so every Floor Codex agent in a new lane
   waits there. Verified with an isolated `CODEX_HOME`: neither `-c
   projects."…".trust_level=…` nor trusting the parent `tasks` folder skips the
   prompt; an entry for the exact folder does.
2. **Approvals.** The user's `ui-designer` role (Codex) has
   `approvals = "on-request"`, so it asks before running commands.

## Requirements

1. `devagentics role set <role> [--model M] [--effort E] [--approvals A]
   [--repo <folder>]` changes those settings in the file the role comes from
   (`~/.devagentics/roles` or the repo's `.devws/roles`), and an empty value
   (`--model ""`) removes the setting so the program's default applies.
2. It rewrites only the named keys' lines in the front matter: other settings,
   comments, key order and the instructions are kept byte for byte.
3. It refuses, with a message naming the allowed values, an effort or approvals
   value that is not valid for the role's program:
   - Claude effort `low|medium|high|xhigh|max`; approvals
     `default|acceptEdits|plan|auto|bypassPermissions`.
   - Codex effort `minimal|low|medium|high|xhigh`; approvals
     `untrusted|on-request|never|auto`.
   Models are free text (new models appear faster than a list can follow).
4. It refuses the built-in leader (it has **Switch leader model**) and unknown
   roles, and the result must still load (`Roles::load`) before it is written.
5. The devws menu gets **⚙ Team roles** in the Agent tab, shown in every
   workspace so roles can be set before a team starts. It lists each role (name,
   group, program, model, effort, approvals), then the setting to change, then
   the value: the program's choices plus **Default** and, for the model, the
   known models of that program plus **Other…** for typed text.
6. After a change it says that agents started from now on use it, and running
   agents keep their settings.
7. When `agent_runner.sh` starts a Codex agent whose working folder is inside the
   devAgentics tasks folder (`$DEVAGENTICS_HOME/tasks`, default
   `~/.devagentics/tasks`), it adds the trust entry for that exact folder to
   that account's `config.toml` (its `CODEX_HOME`, or `~/.codex`) unless one
   exists. Nothing else in that file changes.
8. `role list` also shows each role's effort and approvals, which the menu reads.

## Constraints

- No change to how roles are loaded, overlaid or turned into arguments.
- Folders outside the tasks folder are never trusted automatically: the user's
  own projects keep Codex's prompt.
- No change to running agents; no restart of the team.
- The user's role files are not edited by this change; which approvals
  `ui-designer` uses is the user's choice, made through the new menu item.

## Files Involved

| File | Action |
| ---- | ------ |
| `agentic/src/roles.rs` | Modify — `set` that edits a role file's front matter, with validation |
| `agentic/src/main.rs` | Modify — `role set` subcommand; `role list` shows effort and approvals |
| `config/tmuxinator/workspace_actions.sh` | Modify — `team_roles` menu action |
| `config/tmuxinator/devws_menu.sh` | Modify — **⚙ Team roles** in the Agent tab |
| `config/tmuxinator/agent_runner.sh` | Modify — trust a task folder for Codex |
| `agentic/tests/flow.rs` or `roles.rs` tests | Modify — `role set` cases |
| `scripts/test_devws_menu.sh` | Modify — menu item and its flow |
| `scripts/test_agent_cli.sh` (or the runner's suite) | Modify — trust entry cases |
| `agentic/docs/usage.md`, `docs/guide/agentic.md` | Modify — document both |

## Success Criteria

- [ ] `role set hammer-claude --model claude-sonnet-5-5` changes only that line;
      `--effort ""` removes the key; invalid values and the leader are refused.
- [ ] The menu item lists roles with their settings and writes the chosen value.
- [ ] A Codex agent started in a new lane folder shows no trust prompt; one
      started in a folder outside the tasks folder is not trusted by devws; a
      folder already trusted is not added twice.
- [ ] `make check` passes.
