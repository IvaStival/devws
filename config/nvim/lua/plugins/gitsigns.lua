-- Agents edit files from another tmux pane, so uncommitted changes are
-- highlighted in full (line background plus changed words) instead of only in
-- the sign column, making it easy to read what the agent touched.

-- The colorscheme's diff backgrounds are too bright to read text on. A dark
-- green, like `git diff`, keeps the code's own colours legible; changed lines
-- use it too, since git diff shows a change as added lines.
local function set_change_highlights()
  local line_green = "#1f3a2a"
  local word_green = "#2f6b40"
  vim.api.nvim_set_hl(0, "GitSignsAddLn", { bg = line_green })
  vim.api.nvim_set_hl(0, "GitSignsChangeLn", { bg = line_green })
  vim.api.nvim_set_hl(0, "GitSignsAddLnInline", { bg = word_green })
  vim.api.nvim_set_hl(0, "GitSignsChangeLnInline", { bg = word_green })
end

local function toggle_change_highlights()
  local gitsigns = require("gitsigns")
  gitsigns.toggle_linehl()
  gitsigns.toggle_word_diff()
end

return {
  {
    "lewis6991/gitsigns.nvim",
    opts = {
      linehl = true,
      word_diff = true,
    },
    -- <leader>gh is LazyVim's git hunks group, so the toggle lives on gv.
    keys = {
      { "<leader>gv", toggle_change_highlights, desc = "Toggle change highlights" },
    },
    init = function()
      set_change_highlights()
      vim.api.nvim_create_autocmd("ColorScheme", { callback = set_change_highlights })
    end,
  },
}
