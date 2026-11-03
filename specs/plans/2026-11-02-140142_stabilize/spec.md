# Spec: Stabilize round

- **Date:** 02-11-2026
- **Branch:** feat/stabilize (off feat/job-log-panel)
- **Status:** Specified

## Context

devws grew fast (GitHub workspaces, jungleforge runs, background jobs, the UX
round, the log panel — five stacked, unmerged branches). Real use kept
tripping on the same things rather than on missing features: a new script
missing from `~/.tmuxinator` until `./install.sh` (the log panel silently
never opened), open shells running an old `devws` until `source ~/.zshrc`, a
build killed with `Error 137` and no explanation, and a 10-minute `check.sh`.
This round removes those failure classes, then merges the stack.

## Requirements

1. Every devws start (and `devws reload`, `./install.sh`) links devws scripts
   missing from `~/.tmuxinator` and removes links to scripts that are gone,
   never touching other files there.
2. `devws` is an executable on PATH (`~/.local/bin/devws` → `bin/devws`), so
   every open pane runs the current version; `shell/devws.zsh` keeps only the
   prompt and editor settings.
3. A background step that fails (the log panel, the sidebar's refresh) says so
   in the header as an error note — the sidebar's at most once an hour.
4. A failed job's log, panel and notification say why when the exit code
   tells: 137 out of memory (Docker Desktop's memory), 127 command not found,
   130/143 interrupted — also through `make`'s `Error N`.
5. `check.sh` runs the suites in parallel, reporting each; `check.sh --quick`
   runs the syntax checks and fast suites in under 30 s.
6. One PR for the whole stack into `main`, and one for jungleforge's
   `feat/devws-targets` — pushed only after confirmation.

## Constraints

- `link_scripts.sh` never replaces a real file or a link pointing elsewhere.
- Existing behaviour of every `devws` subcommand is unchanged.

## Success Criteria

- [ ] Deleting a script link, then running `devws`, links it again.
- [ ] An old shell runs the current `devws` without re-sourcing.
- [ ] A job failing with `Error 137` shows the memory hint.
- [ ] `check.sh` passes with per-suite results; `--quick` under 30 s.
- [ ] PRs open after confirmation.
