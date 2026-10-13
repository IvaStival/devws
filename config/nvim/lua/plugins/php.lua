-- intelephense does not implement textDocument/implementation; phpactor does,
-- so it is enabled alongside just for "Goto Implementation" (gI), with its
-- diagnostics suppressed to avoid duplicating intelephense's.
return {
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        phpactor = {
          enabled = true,
          handlers = {
            ["textDocument/publishDiagnostics"] = function() end,
          },
        },
      },
    },
  },
}
