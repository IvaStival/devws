-- Dotfiles like .env are what we most often need to open, and they are usually
-- gitignored too, so showing only hidden files would still leave them out.
-- Show both in the explorer and in the file pickers.
return {
  "folke/snacks.nvim",
  opts = {
    picker = {
      sources = {
        explorer = { hidden = true, ignored = true },
        files = { hidden = true, ignored = true },
      },
    },
  },
}
