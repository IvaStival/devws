-- Options are automatically loaded before lazy.nvim startup
-- Default options that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/options.lua
-- Add any additional options here

-- intelephense is the main PHP server; phpactor runs beside it only for
-- "Goto Implementation" (see plugins/php.lua). Read when the PHP extra loads.
vim.g.lazyvim_php_lsp = "intelephense"
