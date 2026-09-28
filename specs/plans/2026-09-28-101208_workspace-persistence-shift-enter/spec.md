# Spec: Workspace state persistence and a working Shift+Enter

- **Date:** 28-09-2026
- **Branch:** main
- **Status:** Executed

## Context

Two independent gaps, addressed together because both are about a devws
workspace surviving contact with the real machine.

**Workspace state is lost on restart or crash.** Everything devws knows about a
workspace lives only in the tmux server: the session options `@devws_root`,
`@devws_agent` and `@devws_editor` set by `config/tmuxinator/dev.yml`, and the
`@devws_role` pane option set by the runners. When macOS restarts or
iTerm2/tmux dies, every open workspace — with its agent and editor choices,
its extra terminals and its sidebar state — is gone and has to be rebuilt by
hand.

**Shift+Enter does not insert a newline, anywhere in iTerm2.** iTerm2 sends a
bare `CR` (0x0d) for both Enter and Shift+Enter, so no program downstream can
tell them apart and every agent composer submits instead of adding a line.
Commit `b8bdbf7` added `set -s extended-keys always` to
`config/tmux/tmux.conf`, which could never have worked: `man tmux` states that
`always` forces modifyOtherKeys **mode 1**, which changes the sequence only for
"keys which lack an existing well-known representation" — `Enter` has one, so
the Shift bit is dropped on re-encoding. The bit was also never transmitted in
the first place, which is why the bug reproduces outside tmux entirely.

`tmux-resurrect`/`tmux-continuum` were considered for the first half and
rejected: every devws session is generated from one tmuxinator template, so a
restore is a replay of `tmuxinator start dev …` per workspace. A devws-owned
state file needs no new dependency and gives back a genuine devws workspace —
roles, sidebar, status bar — rather than a raw layout snapshot.

## Requirements

1. Persist every open devws workspace to disk, capturing the workspace folder,
   agent, editor, window layout, each pane's role and working directory, the
   extra terminals created from the sidebar, and whether the sidebar was open.
2. Save automatically whenever a workspace changes: created, closed, client
   detached, agent or editor swapped, terminal added, sidebar toggled.
   Saves must be crash-safe, idempotent, and cheap enough to run on a tmux hook.
3. Restore the saved workspaces automatically the first time `devws` runs with
   no tmux server, before starting the workspace that was asked for. Never
   restore while a server is already running, so a deliberately closed
   workspace is not resurrected.
4. Provide explicit `devws save`, `devws restore [--no-attach]` and
   `devws fresh` commands, and an opt-out for the automatic path.
5. Restore must be idempotent and degrade gracefully: skip a workspace that is
   already open, skip one whose folder no longer exists (with a message), and
   fall back to the configured default when a saved agent or editor is no
   longer listed in `.env`.
6. Add nothing to the top level of `shell/devws.zsh`: it is sourced by every
   interactive shell, so no state probe may run at shell startup.
7. Make Shift+Enter distinguishable at the source — in iTerm2 — and ship that
   configuration in the repository, idempotently, with a backup and a reversal
   in `delete.sh`.
8. Correct the tmux side so it neither blocks nor misreports modified keys.
9. Cover both features with tests registered in `scripts/check.sh`, and
   document both in `README.md`.

## Constraints

- No new runtime dependency. `jq` is not in the `Brewfile`, so the state file
  must be readable with the shell tools already in use.
- `#!/usr/bin/env bash` may resolve to macOS bash 3.2: no associative arrays,
  no `mapfile`.
- The iTerm2 change must not clobber the user's existing key bindings. While
  `GlobalKeyMap` is unset, iTerm2 falls back to a bundled default map, so a
  naive single-entry write silently drops every built-in global binding.
- iTerm2 holds its preferences in memory and rewrites them on quit; a write
  made while it is running is discarded.
- The mapping must survive a restart even when iTerm2 loads its preferences
  from a custom folder.
- Restoring must never attach on its own: the caller decides.

## Files Involved

| File | Action |
| ---- | ------ |
| `config/tmuxinator/save_state.sh` | Create — snapshot every open workspace to `~/.devws-state/sessions.tsv` |
| `config/tmuxinator/restore_state.sh` | Create — rebuild the saved workspaces, without attaching |
| `scripts/setup_iterm_keys.sh` | Create — add the Shift+Enter mapping to iTerm2's global key map |
| `scripts/test_workspace_state.sh` | Create — save/restore coverage against fake `tmux`/`tmuxinator` |
| `scripts/test_iterm_keys.sh` | Create — key-map coverage against a fake `defaults`/`pgrep` |
| `shell/devws.zsh` | Modify — `save`/`restore`/`fresh` subcommands, auto-restore, save after mutations |
| `config/tmuxinator/session_picker.sh` | Modify — save after every workspace mutation; role-first terminal-pane lookup |
| `config/tmuxinator/dev.yml` | Modify — the terminal pane marks itself with `@devws_role` |
| `config/tmuxinator/.env` | Modify — document `DEVWS_AUTO_RESTORE` |
| `config/tmux/tmux.conf` | Modify — save hooks; `extended-keys on`, `tmux-256color`, `extkeys`, `csi-u` |
| `install.sh` | Modify — `--iterm-keys` flag and an opt-in prompt |
| `delete.sh` | Modify — remove the state file and the owned iTerm2 mapping |
| `scripts/check.sh` | Modify — register the two new tests and the new script |
| `Makefile` | Modify — `install-iterm-keys` target |
| `README.md` | Modify — document session persistence and the iTerm2 mapping |

## Success Criteria

Verified:

- [x] A workspace with an extra terminal in its own subdirectory and the
      sidebar closed survives being killed and comes back with the right
      folder, agent, editor, panes and geometry.
- [x] A second restore is a silent no-op and leaves no lock behind.
- [x] A workspace whose folder was deleted is skipped with a message; an
      unknown agent or editor falls back to the configured default.
- [x] `devws save`, `devws restore --no-attach` and `devws fresh` behave as
      documented.
- [x] The written global key map contains iTerm2's bundled bindings plus the
      new one, never the new one alone.
- [x] `make check` passes, including both new test scripts.

Pending — both need iTerm2 quit, which cannot be done from inside it:

- [ ] Shift+Enter produces `^[^M` in a bare iTerm2 tab and inside a tmux pane,
      and inserts a newline in the agent composer while Enter still submits.
- [ ] `./delete.sh` returns the machine to its pre-devws state, including the
      key map and `~/.devws-state`.
