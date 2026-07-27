# Spec: Markdown viewing with Glow

- **Date:** 27-07-2026
- **Branch:** main
- **Status:** Executed

## Context

devws needs a readable terminal rendering workflow for Markdown files on both
macOS and Linux. Glow will provide rendering, with entry points from the devws
shell command and LunarVim.

## Requirements

1. Add Glow to the Homebrew dependencies used on macOS and Linux.
2. Track whether `./install.sh --deps` installed Glow so `delete.sh` removes
   it only when devws added it; preserve a pre-existing Glow installation.
3. Add `devws md [file]` to render a supplied `.md` or `.markdown` file with
   Glow's pager.
4. When `devws md` has no file argument, list Markdown files below the current
   directory and use `fzf` to choose one.
5. Reject missing, unreadable, non-file, non-Markdown, and extra arguments with
   concise actionable errors.
6. Add a LunarVim `<leader>mp` mapping that saves the current Markdown buffer
   and opens it with Glow in a temporary terminal split.
7. Close the temporary LunarVim terminal split when Glow exits and leave other
   buffers/windows untouched.
8. Report an actionable error from either entry point when Glow is missing.
9. Document installation, command usage, picker behavior, shortcut, and
   uninstall ownership.

## Constraints

- Homebrew remains the only automatic package manager on macOS and Linux.
- A normal `./install.sh` does not install optional Glow; `--deps` is the
  explicit installation path.
- Do not remove a Glow installation that existed before devws.
- Do not introduce a second Markdown renderer or editor plugin.
- File paths containing spaces must work.
- The shell command operates from the caller's current directory, not only
  from a running tmux workspace.

## Files Involved

| File | Action |
| ---- | ------ |
| `Brewfile` | Modify — add the cross-platform Glow formula |
| `install.sh` / `delete.sh` | Modify — record and remove only devws-owned Glow |
| `shell/devws.zsh` | Modify — add `devws md` and the interactive picker |
| `config/lvim/config.lua` | Modify — add the Markdown preview mapping |
| `scripts/` checks/tests | Modify/Create — cover CLI and dependency ownership behavior |
| `README.md` | Modify — document both opening workflows |

## Success Criteria

- [ ] `./install.sh --deps` makes Glow available on macOS and Linux through Homebrew.
- [ ] Uninstall removes devws-installed Glow and preserves pre-existing Glow.
- [ ] `devws md README.md` renders the file with Glow.
- [ ] `devws md` selects a Markdown file through `fzf`.
- [ ] Invalid paths, extensions, arguments, or missing commands fail clearly.
- [ ] `<leader>mp` previews the current saved Markdown buffer and cleans up its terminal split.
- [ ] Paths containing spaces work in both entry points.
- [ ] Automated checks and isolated lifecycle tests pass.
