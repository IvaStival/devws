-- Autocmds are automatically loaded on the VeryLazy event
-- Default autocmds that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/autocmds.lua

-- Change highlights are only useful if the buffer shows the agent's latest
-- write. LazyVim runs :checktime on FocusGained, but focus events never fire
-- while the cursor sits in the agent's pane, so a timer polls as well.
local function reload_changed_buffers()
  if vim.fn.mode() == "c" or vim.fn.getcmdwintype() ~= "" then
    return
  end
  vim.cmd("silent! checktime")
end

vim.api.nvim_create_autocmd({ "BufEnter", "CursorHold", "CursorHoldI" }, {
  group = vim.api.nvim_create_augroup("devws_reload_changed_buffers", { clear = true }),
  callback = reload_changed_buffers,
})

local reload_timer = vim.uv.new_timer()
reload_timer:start(1000, 1000, vim.schedule_wrap(reload_changed_buffers))
