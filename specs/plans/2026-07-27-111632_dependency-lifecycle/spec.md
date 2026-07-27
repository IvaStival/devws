# Spec: Safe dependency lifecycle during installation

- **Date:** 27-07-2026
- **Branch:** main
- **Status:** Executed

## Context

An installation can inherit an outdated Homebrew-managed tmuxinator or reach
the workspace picker without `fzf`, causing a repeated `command not found`
loop. The installer must distinguish dependencies that existed before devws
from dependencies installed by devws so uninstall removes only what devws
added.

## Requirements

1. Detect a Homebrew-managed tmuxinator during dependency installation and
   upgrade it automatically without prompting; preserve non-Homebrew
   installations and report that they were not modified.
2. Check for `fzf` on every installation, including runs without `--deps`.
3. When `fzf` is missing, explain that it is required and ask permission
   before installing it with Homebrew; abort before changing configuration if
   permission is declined or installation fails.
4. Record that devws installed `fzf` in `~/.devws-state/` without overwriting
   existing backup state.
5. During uninstall, remove the Homebrew `fzf` formula only when installation
   state proves devws installed it. Never remove a pre-existing `fzf`.
6. Make the picker fail once with an actionable message if `fzf` is still
   unavailable instead of entering a repeated error loop.
7. Finish installation with blank-line separation and a distinct colored
   `source ~/.zshrc` instruction; do not attempt to source the parent shell
   from the child installer.
8. Document prompting, automatic tmuxinator upgrades, dependency ownership,
   shell reload, and uninstall behavior in the README.

## Constraints

- Homebrew remains the only supported automatic package installer.
- Do not downgrade or uninstall a tmuxinator that existed before devws.
- Do not remove `fzf` unless the devws state explicitly marks it as installed
  by devws.
- Preserve the existing configuration backup and restoration behavior.
- Keep non-interactive installation safe: missing `fzf` without an interactive
  input must fail with instructions instead of assuming consent.

## Files Involved

| File | Action |
| ---- | ------ |
| `install.sh` | Modify — validate, upgrade, prompt, install, and record dependency ownership |
| `delete.sh` | Modify — remove only a devws-installed `fzf` and clean dependency state |
| `config/tmuxinator/session_picker.sh` | Modify — fail cleanly when `fzf` is unavailable |
| `scripts/check.sh` and installer tests | Modify/Create — cover dependency lifecycle behavior |
| `README.md` | Modify — document installation, reload, and uninstall semantics |

## Success Criteria

- [ ] A pre-existing Homebrew tmuxinator is upgraded without a prompt.
- [ ] A non-Homebrew tmuxinator is preserved with a clear notice.
- [ ] Missing `fzf` prompts before any configuration links are changed.
- [ ] Declining the prompt leaves configuration untouched and exits non-zero.
- [ ] Accepting installs `fzf` and records devws ownership.
- [ ] Uninstall removes a devws-installed `fzf` but preserves a pre-existing one.
- [ ] The picker emits one actionable missing-`fzf` error and exits.
- [ ] Installation prints a visually separated colored `source ~/.zshrc`.
- [ ] Automated checks and isolated install/delete lifecycle tests pass.
