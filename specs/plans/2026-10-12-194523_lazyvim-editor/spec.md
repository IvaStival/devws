# Spec: Replace LunarVim with LazyVim

- **Date:** 12-10-2026
- **Branch:** feat/lazyvim-editor
- **Status:** Executed

## Context

LunarVim is the default devws editor, but it is no longer maintained: its last
release targets Neovim 0.9, and `config/lvim/config.lua` carries ~150 lines of
runtime patches to stop nvim-treesitter and indent-blankline crashing on
Neovim 0.12. LazyVim is actively maintained (v16.0.1, 08-09-2026), supports
Neovim ≥ 0.11.2, and is built on the same pieces (lazy.nvim, which-key,
`Space` leader, gitsigns), so habits carry over. It also makes the planned
VS Code-style improvements easier through its "extras".

Decisions taken with the user:

- LunarVim is removed from devws (editor list, installer, uninstaller, docs).
  The LunarVim install on each machine is left untouched.
- LazyVim is the standard Neovim config: `~/.config/nvim` links to the repo and
  the editor is chosen as `nvim`.

## Requirements

1. The repo ships a LazyVim config (`config/nvim/`, starter layout) that
   `install.sh` links to `~/.config/nvim`. On first start lazy.nvim bootstraps
   LazyVim and its plugins without manual steps.
2. `nvim` is the default devws editor; the editor list becomes
   `nvim vim code zed`. Saved workspaces that used `lvim` reopen with `nvim`.
3. Every devws customisation of LunarVim works the same in LazyVim:
   - uncommitted changes highlighted with dark-green line and word
     backgrounds, with a key to toggle the highlights;
   - buffers reload automatically when an agent changes a file on disk
     (focus events plus a 1 s poll);
   - `<leader>mp` replaces a Markdown buffer with its Glow rendering in the
     same window and toggles back; Mermaid diagrams also open in the browser;
   - PHP uses intelephense, plus phpactor (diagnostics silenced) for
     "go to implementation";
   - starting the editor on a directory (`nvim .`) shows the file explorer.
4. LunarVim-only code is deleted: the treesitter/indent-blankline/nvim-tree
   patches, the markdown highlights query override, and `config/lvim/`.
5. `install.sh --deps` installs what LazyVim needs (ripgrep, fd,
   tree-sitter CLI) instead of LunarVim; the normal install and `delete.sh`
   link and restore `~/.config/nvim`.
6. Tests, `scripts/check.sh` and the README reflect the new editor.

## Constraints

- Do not uninstall LunarVim or touch `~/.config/lvim` contents beyond what
  `delete.sh` already restores for links devws created.
- Do not overwrite an existing `~/.config/nvim` silently — reuse `install.sh`'s
  existing backup-then-link behaviour.
- Keep the editor-agnostic parts of devws (vim/code/zed support, tmux, picker
  behaviour) unchanged apart from the editor list.
- No new VS Code-style shortcuts in this change; that is follow-up work.
- Key bindings must not shadow LazyVim defaults (e.g. `<leader>gh` is
  LazyVim's git-hunks group, so the highlight toggle moves to a free key).

## Files Involved

| File | Action |
| ---- | ------ |
| `config/nvim/init.lua`, `lua/config/{lazy,options,keymaps,autocmds}.lua` | Create — LazyVim starter + devws options/keys/autocmds |
| `config/nvim/lua/plugins/*.lua` | Create — gitsigns colours, PHP, explorer overrides |
| `config/nvim/lua/devws/markdown_preview.lua` | Create — ported Glow preview toggle |
| `config/nvim/lazy-lock.json`, `lazyvim.json` | Create — pinned plugin versions and enabled extras |
| `config/lvim/` | Delete |
| `config/tmuxinator/.env`, `editor_runner.sh`, `restore_state.sh`, `session_picker.sh`, `dev.yml` | Modify — `nvim` default, drop `lvim` |
| `shell/devws.zsh`, `shell/render_mermaid_preview.sh` | Modify — editor list default, comment path |
| `config/tmux/tmux.conf` | Modify — comment mentioning lvim |
| `Brewfile` | Modify — add ripgrep, fd, tree-sitter |
| `install.sh`, `delete.sh` | Modify — link/restore `~/.config/nvim`, drop LunarVim |
| `scripts/check.sh`, `scripts/test_install_lifecycle.sh`, `scripts/test_workspace_state.sh` | Modify — new paths and editor |
| `README.md` | Modify — LazyVim instead of LunarVim |

## Success Criteria

- [ ] `nvim` started in a project opens LazyVim with the explorer, no errors in `:messages`, `:checkhealth lazyvim` without errors.
- [ ] Change highlights, auto-reload, `<leader>mp` toggle and Mermaid preview verified in a real nvim (same scripted checks used for lvim).
- [ ] PHP buffer attaches intelephense and phpactor.
- [ ] A new workspace and a restored `lvim` workspace both open `nvim`.
- [ ] No `lvim`/LunarVim references remain outside `specs/`.
- [ ] `scripts/check.sh` passes.
