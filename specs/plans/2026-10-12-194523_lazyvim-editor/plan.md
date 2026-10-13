# Plan: Replace LunarVim with LazyVim

- **Date:** 12-10-2026
- **Branch:** feat/lazyvim-editor
- **Status:** Executed

## Context

Implements `spec.md` in this folder: ship a LazyVim config as the standard
`~/.config/nvim`, make `nvim` the default devws editor, port every devws
LunarVim customisation, and delete LunarVim from devws.

## Step 1 — LazyVim starter in the repo

**Files:**
- `config/nvim/init.lua`, `config/nvim/lua/config/{lazy,options,keymaps,autocmds}.lua`, `config/nvim/stylua.toml`, `config/nvim/.gitignore`

**Actions:**
- Copy the official LazyVim starter (`github.com/LazyVim/starter`) into
  `config/nvim/` without its `.git`, LICENSE or README.
- Keep `lazy.lua` as shipped, except `checker.enabled = false` so plugin
  versions only move when `lazy-lock.json` is updated deliberately (the repo
  syncs machines through the lock file).

**Verification:**
- `luac -p` passes on every Lua file.

## Step 2 — Port the editor behaviour

**Files:**
- `config/nvim/lua/config/autocmds.lua`, `config/nvim/lua/config/keymaps.lua`
- `config/nvim/lua/plugins/gitsigns.lua`, `config/nvim/lua/plugins/php.lua`, `config/nvim/lua/plugins/explorer.lua`
- `config/nvim/lua/devws/markdown_preview.lua`

**Actions:**
- **Change highlights** (`plugins/gitsigns.lua`): extend LazyVim's gitsigns
  opts with `linehl = true`, `word_diff = true`; set `GitSignsAddLn`,
  `GitSignsChangeLn` (`#1f3a2a`) and their `Inline` groups (`#2f6b40`) on
  load and on `ColorScheme`. Toggle on `<leader>gv` ("Toggle change
  highlights"), since `<leader>gh` is LazyVim's hunks group.
- **Auto-reload** (`autocmds.lua`): LazyVim already runs `checktime` on
  FocusGained/TermClose/TermLeave; add BufEnter/CursorHold(I) and the 1 s
  timer, skipping command-line mode, as in the lvim config.
- **Markdown preview** (`devws/markdown_preview.lua` + `keymaps.lua`): move
  the lvim functions over unchanged in behaviour (`<leader>mp` toggle, Glow
  with forced colours, Mermaid browser preview). Locate
  `shell/render_mermaid_preview.sh` via
  `vim.fn.resolve(vim.fn.stdpath("config"))` → repo root `:h:h`.
- **PHP** (`plugins/php.lua`): import the `lazyvim.plugins.extras.lang.php`
  extra with `vim.g.lazyvim_php_lsp = "intelephense"`, and enable phpactor
  alongside with its diagnostics handler silenced.
- **Explorer on a directory** (`plugins/explorer.lua`): confirm LazyVim's
  snacks explorer opens for `nvim .`; if it doesn't, add a VimEnter autocmd
  that opens `Snacks.explorer()` when the argument is a directory.

**Verification:**
- Same scripted checks used for lvim, run in a real nvim inside an isolated
  tmux server: external write reloads the buffer and gitsigns reports the
  hunk; highlight groups hold the green values; `<leader>mp` opens and
  closes the preview with colours; `nvim .` shows the explorer.

## Step 3 — Bootstrap and lock

**Files:**
- `config/nvim/lazy-lock.json`, `config/nvim/lazyvim.json`
- `Brewfile`

**Actions:**
- Add `ripgrep`, `fd` and `tree-sitter-cli` to the Brewfile and install them.
- Link `~/.config/nvim` (via Step 4's installer) and start nvim headless with
  `Lazy! sync` to install plugins and write the lock file; run `:TSUpdate`.

**Verification:**
- `:checkhealth lazyvim` reports no errors; `:messages` is clean on startup.

## Step 4 — Installer and uninstaller

**Files:**
- `install.sh`, `delete.sh`

**Actions:**
- `install.sh`: link the directory `config/nvim` → `~/.config/nvim` with the
  existing `link_file` (backs up an existing config first). Remove the
  LunarVim installer from `--deps`; switch the post-install warning to
  check `nvim`. Remove stale links devws created under `~/.config/lvim` that
  point into `config/lvim/` (they would dangle once it is deleted), restoring
  any pre-devws backup the same way `delete.sh` does.
- `delete.sh`: `restore_link` for `~/.config/nvim`; keep the old lvim
  `restore_link` lines so uninstalling from an older install still cleans up.

**Verification:**
- `scripts/test_install_lifecycle.sh` extended: install links
  `~/.config/nvim`, removes a pre-existing devws lvim link, backs up a
  pre-existing `~/.config/nvim`; delete removes the link and restores the
  backup.

## Step 5 — Editor list and defaults

**Files:**
- `config/tmuxinator/.env`, `editor_runner.sh`, `restore_state.sh`, `session_picker.sh`, `dev.yml`
- `shell/devws.zsh`, `shell/render_mermaid_preview.sh`, `config/tmux/tmux.conf`

**Actions:**
- Replace `lvim nvim vim code zed` with `nvim vim code zed` and the `lvim`
  default with `nvim` everywhere; drop `lvim` from the picker's
  editor-detection fallback; update the lvim mentions in comments.

**Verification:**
- `scripts/test_workspace_state.sh` updated: a saved `lvim` workspace
  restores with `editor=nvim`.

## Step 6 — Remove LunarVim config, update checks and docs

**Files:**
- `config/lvim/` (delete), `scripts/check.sh`, `README.md`

**Actions:**
- Delete `config/lvim/`.
- `check.sh`: `luac -p` every Lua file under `config/nvim`.
- README: LazyVim in requirements, install, usage (`Space e` explorer,
  `<leader>gv`, `<leader>mp`), configuration paths; note that LunarVim is no
  longer used and can be uninstalled manually.

**Verification:**
- `grep -rniE "lvim|lunarvim"` outside `specs/` and the delete.sh migration
  lines returns nothing.

## Final Verification

1. `./scripts/check.sh` passes.
2. Real workspace: `devws <project>` opens LazyVim in the editor pane; a
   restored former-lvim workspace opens nvim; change highlights, auto-reload,
   Markdown preview and PHP go-to-implementation work.
3. `:checkhealth lazyvim` without errors.

## Execution Notes

- Step 2: LazyVim's snacks explorer already opens for `nvim .`, so no
  `plugins/explorer.lua` was needed.
- Step 3: the PHP extra is imported in `lua/config/lazy.lua` rather than via
  `:LazyExtras`; `lazyvim.json` only holds LazyVim's version and news state.
- Step 4: on this machine the retired lvim links were replaced by the
  pre-devws backups (links into `personalWorkspace`).
- Verified in a real nvim: change highlights (`#1f3a2a`/`#2f6b40`,
  `<leader>gv`), auto-reload after an external write, `<leader>mp` toggle with
  colours, intelephense + phpactor attached to a PHP buffer,
  `:checkhealth lazyvim` all OK, `scripts/check.sh` passes.
