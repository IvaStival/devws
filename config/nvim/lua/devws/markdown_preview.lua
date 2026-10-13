-- Glow-rendered Markdown preview that replaces the source in the same window.

local M = {}

-- ~/.config/nvim is a symlink into the devws clone, so resolving it finds
-- sibling repo files such as shell/render_mermaid_preview.sh.
local function repo_root()
  return vim.fn.fnamemodify(vim.fn.resolve(vim.fn.stdpath("config")), ":h:h")
end

-- Glow can't render Mermaid diagrams graphically; this opens a CDN-backed
-- HTML preview in the system browser as well, but only when the file
-- actually has a Mermaid fence (the script no-ops otherwise).
local function run_mermaid_preview(markdown_file)
  local script = repo_root() .. "/shell/render_mermaid_preview.sh"
  if vim.fn.executable(script) == 1 then
    vim.fn.system({ script, markdown_file })
  end
end

-- The preview replaces the Markdown source in the same window, and the same
-- key swaps the source back. Glow renders once without its pager into a
-- job-less terminal buffer, so the preview stays in normal mode: Vim motions
-- scroll it and <leader>mp still reaches this mapping instead of the pager.
local function render_markdown_with_glow(markdown_file, width)
  -- Glow drops its colours when stdout is not a TTY unless forced to keep them.
  local result = vim.system(
    { "glow", "--style", "dark", "--width", tostring(width), markdown_file },
    { text = true, env = { CLICOLOR_FORCE = "1" } }
  ):wait()
  if result.code ~= 0 then
    return nil, result.stderr
  end
  return result.stdout
end

local function close_markdown_preview(window)
  local preview = vim.w[window].markdown_preview
  vim.w[window].markdown_preview = nil
  if vim.api.nvim_buf_is_valid(preview.source_buffer) then
    vim.api.nvim_win_set_buf(window, preview.source_buffer)
  end
end

function M.toggle()
  local window = vim.api.nvim_get_current_win()
  if vim.w[window].markdown_preview then
    close_markdown_preview(window)
    return
  end

  if vim.bo.filetype ~= "markdown" then
    vim.notify("Glow preview is available only for Markdown buffers", vim.log.levels.WARN)
    return
  end
  if vim.fn.executable("glow") ~= 1 then
    vim.notify("Glow is required; run ./install.sh --deps", vim.log.levels.ERROR)
    return
  end

  local markdown_file = vim.api.nvim_buf_get_name(0)
  if markdown_file == "" then
    vim.notify("Save the Markdown file before previewing it", vim.log.levels.WARN)
    return
  end
  if vim.bo.modified then
    vim.cmd.write()
  end

  local rendered, render_error = render_markdown_with_glow(markdown_file, vim.api.nvim_win_get_width(window) - 2)
  if not rendered then
    vim.notify("Glow failed: " .. (render_error or "unknown error"), vim.log.levels.ERROR)
    return
  end

  run_mermaid_preview(markdown_file)

  local source_buffer = vim.api.nvim_get_current_buf()
  local preview_buffer = vim.api.nvim_create_buf(false, true)
  vim.bo[preview_buffer].bufhidden = "wipe"
  vim.api.nvim_win_set_buf(window, preview_buffer)
  vim.w[window].markdown_preview = { source_buffer = source_buffer }

  local terminal = vim.api.nvim_open_term(preview_buffer, {})
  vim.api.nvim_chan_send(terminal, (rendered:gsub("\n", "\r\n")))
  vim.api.nvim_win_set_cursor(window, { 1, 0 })

  -- Leaving the preview any other way (:bnext, picking a file in the tree)
  -- must not leave a stale marker that would swallow the next <leader>mp.
  vim.api.nvim_create_autocmd("BufWinLeave", {
    buffer = preview_buffer,
    once = true,
    callback = function()
      if vim.api.nvim_win_is_valid(window) and vim.w[window].markdown_preview then
        vim.w[window].markdown_preview = nil
      end
    end,
  })
end

return M
