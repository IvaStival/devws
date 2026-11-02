# Plan: Stabilize round

- **Date:** 02-11-2026
- **Branch:** feat/stabilize (off feat/job-log-panel)
- **Status:** Executed
- **Commits:** 1e654b9, 1537d2c
- **PRs:** IvaStival/devws#6 (the whole stack), IvaStival/jungleforge#7

Notes from execution:
- `bin/devws` is the old functions moved as they are, ending in `devws "$@"`;
  `shell/devws.zsh` also `unfunction`s them, so a re-sourced shell stops
  shadowing the command.
- `link_scripts.sh` resolves its own links to find the checkout, so it works
  run through `~/.tmuxinator/link_scripts.sh` or the checkout.
- check.sh finds every `scripts/test_*.sh` itself (a new suite can't be
  forgotten) and runs them in batches of `DEVWS_CHECK_JOBS` (4): `wait -n`
  needs bash 4.3, and macOS's `/usr/bin/env bash` may be 3.2.
- `jungleforge.sh running-folders` is silent without jungleforge, so the
  sidebar's hourly error note only reports real failures.
- The full run takes about 4–5 min (was ~10). Under parallel load one suite
  once failed with "fork failed: Device not configured" (the machine briefly
  out of pseudo-terminals); check.sh now re-runs such a suite alone and says
  so when that is what made it pass.
- jungleforge's base work (fix/assets-stage-vendor) was already merged into
  GitHub's main, so its PR goes into main with the one targets commit.

## 1. Scripts link themselves — new `config/tmuxinator/link_scripts.sh`
- `link_scripts.sh` (no args): for each file in `config/tmuxinator/` (and
  `.env`), make `~/.tmuxinator/<name>` a link to it when missing; remove links
  in `~/.tmuxinator` that point into the repo's `config/tmuxinator/` but whose
  target is gone. Never touches a real file or a link pointing elsewhere (the
  user's other tmuxinator projects). Silent unless it changed something.
- Called by `install.sh` (replacing its tmuxinator link loop's job for new
  files; `link_file`'s backup logic stays for first install), by `devws` on
  every start (cheap), and by `reload_devws.sh`.
- Test `scripts/test_link_scripts.sh`: a missing script gets linked; a stale
  link is removed; a foreign file / foreign link is untouched; second run is
  a no-op.

## 2. `devws` as an executable — new `bin/devws`
- Move every function from `shell/devws.zsh` (`devws`, `devws-restore`,
  `-root`, `-run`, `-push`, `-jobs`, `-markdown`, `-menu`) into `bin/devws`, a
  zsh script (`#!/usr/bin/env zsh`) whose top level dispatches the subcommand
  exactly as `devws()` does now; recursion (`devws "$folder" …`) becomes a
  call of its own main function. `self_dir` for `md` comes from `${0:A:h}`.
- `shell/devws.zsh` keeps only `EDITOR`/`VISUAL` and the compact prompt, plus
  `unfunction devws 2>/dev/null` so a shell sourced from the old version
  stops shadowing the executable.
- `install.sh` links `bin/devws` into `~/.local/bin/devws` (via `link_file`,
  backed up like the rest; warns if `~/.local/bin` is not on PATH);
  `delete.sh` removes it.
- `devws` runs `link_scripts.sh` first on every invocation.
- Tests: update tests that source `shell/devws.zsh` to call `bin/devws`
  (grep `devws.zsh` in `scripts/`); check.sh's `zsh -n` covers `bin/devws`.

## 3. No silent background failures — `job.sh`
- `open_panel_later` / `open`: when `job_panel.sh` fails (missing,
  error), post `job.sh note "Log panel could not open: <reason> — run
  ./install.sh" --error` instead of discarding it.
- Same for the sidebar's background refresh (`session_picker.sh`
  `refresh_states`): a failing `jungleforge.sh running-folders` /
  `github_repo.sh pr-refresh` writes its error to a note at most once per
  hour (stamp file), not every 30 s.

## 4. Failures say why — `job.sh`
- On failure, append a hint line to the job's log, shown in the log panel and
  used as the notification's message: 137 → "killed — usually out of memory:
  give Docker Desktop more (Settings → Resources)"; 127 → "command not
  found"; 130/143 → "interrupted"; `make … Error N` lines map the same way
  (the jungleforge case: make exits 2 wrapping a 137).
- Test in `scripts/test_job.sh`: a job whose log ends with `make: *** [deps]
  Error 137` gets the out-of-memory hint; `exit 127` gets "command not found".

## 5. Faster checks — `scripts/check.sh`
- Syntax and executable-bit checks first (fast, sequential).
- Then every `test_*.sh` in parallel (`&`, each to its own log in a temp
  dir), waited on, printing `✓ <suite>` / `✗ <suite>` and the full log of
  each failure; exit 1 on any failure.
- `check.sh --quick`: syntax checks plus the suites that need no tmux server
  or real timing (picker, recent, shared_files, worktree, github_repo,
  git_actions, jungleforge, tidy, link_scripts, menu items) — target < 30 s.
- Each suite already uses its own temp folder and tmux socket (`-L …$$`); any
  shared resource found while parallelizing (e.g. TMPDIR-wide files) gets its
  own TMPDIR per suite.

## 6. Merge (after 1–5 pass; outward-facing, confirm first)
- Push `feat/stabilize` (it contains the whole stack) and open one PR into
  `main` with `gh pr create`, the body listing each part (GitHub workspaces,
  jungleforge runs, background jobs, UX round, log panel, stabilize) and how
  to try it.
- jungleforge: push `feat/devws-targets` and open its PR into
  `fix/assets-stage-vendor` (the branch it was cut from and the setup runs).
- Confirm with the user before pushing.

## Files
- Create: `bin/devws`, `config/tmuxinator/link_scripts.sh`,
  `scripts/test_link_scripts.sh`
- Modify: `shell/devws.zsh` (prompt/EDITOR only), `install.sh`, `delete.sh`,
  `config/tmuxinator/reload_devws.sh`, `job.sh`, `session_picker.sh`,
  `scripts/check.sh`, `scripts/test_job.sh`, tests that source devws.zsh,
  `README.md` (install, `devws` on PATH, quick checks)
- Reuse: `install.sh` `link_file`/`backup_path`; `job.sh note`.

## Verification
- `scripts/check.sh` passes and reports per-suite results; time it (target:
  a few minutes). `scripts/check.sh --quick` < 30 s.
- Manual: delete `~/.tmuxinator/job_panel.sh`, run `devws` → it is linked
  again. In an already-open pane (old shell), `devws jobs` works without
  `source ~/.zshrc`. A job failing with exit 137 shows the memory hint in its
  log panel and notification.
- Then push + PRs, after confirmation.
