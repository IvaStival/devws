-- Shows the agent's changes: devws's editor_open.sh calls show() through
-- Neovim's server socket when the agent edits a file or asks to show one.

local M = {}

local namespace = vim.api.nvim_create_namespace("devws_follow")
local highlight_duration_ms = 1500

-- The stamp of the latest request acted on: requests arrive in the background
-- and can overtake each other, so an older one must not move the editor back.
local latest_stamp = 0

-- The user's own typing comes first: the agent only gets a notification then.
-- So do their unsaved changes to the file the agent changed, which reloading
-- it would clash with; unsaved changes to other files don't matter.
local function user_is_busy(path)
  local mode = vim.api.nvim_get_mode()
  if mode.blocking then
    return true
  end
  local kind = mode.mode:sub(1, 1)
  if kind == "i" or kind == "R" or kind == "c" or kind == "t" then
    return true
  end
  local buffer = vim.fn.bufnr(path)
  return buffer ~= -1 and vim.bo[buffer].modified
end

-- A window showing a file: the current one if it does, else the first such
-- window, so the file never opens in the explorer or a terminal.
local function file_window(path)
  local buffer = vim.fn.bufnr(path)
  if buffer ~= -1 then
    local shown = vim.fn.win_findbuf(buffer)[1]
    if shown then
      return shown
    end
  end
  local function shows_a_file(window)
    local config = vim.api.nvim_win_get_config(window)
    return config.relative == "" and vim.bo[vim.api.nvim_win_get_buf(window)].buftype == ""
  end
  local current = vim.api.nvim_get_current_win()
  if shows_a_file(current) then
    return current
  end
  for _, window in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    if shows_a_file(window) then
      return window
    end
  end
  return current
end

-- The number of the first of `lines` containing `text`, ignoring surrounding whitespace.
local function line_containing(lines, text)
  local needle = vim.trim(text or "")
  if needle == "" then
    return nil
  end
  for number, content in ipairs(lines) do
    if content:find(needle, 1, true) then
      return number
    end
  end
  return nil
end

local function flash(buffer, line)
  vim.api.nvim_buf_clear_namespace(buffer, namespace, 0, -1)
  vim.api.nvim_buf_set_extmark(buffer, namespace, line - 1, 0, { line_hl_group = "IncSearch" })
  vim.defer_fn(function()
    if vim.api.nvim_buf_is_valid(buffer) then
      vim.api.nvim_buf_clear_namespace(buffer, namespace, 0, -1)
    end
  end, highlight_duration_ms)
end

--- Opens `path` at `line` (0: unknown), or at the first line containing `text`.
--- With `notify_only` (the "notify" follow mode), or while the user is typing,
--- it only names the place in a notification. `stamp` (optional) orders the
--- requests: one older than the latest is ignored. Returns "shown", "notified"
--- or "outdated".
function M.show(path, line, text, notify_only, stamp)
  if stamp then
    if stamp < latest_stamp then
      return "outdated"
    end
    latest_stamp = stamp
  end
  if notify_only or user_is_busy(path) then
    if line == 0 and vim.fn.filereadable(path) == 1 then
      line = line_containing(vim.fn.readfile(path), text) or 0
    end
    local label = vim.fn.fnamemodify(path, ":~:.") .. (line > 0 and ":" .. line or "")
    vim.notify("Agent: " .. label, vim.log.levels.INFO, { title = "devws" })
    return "notified"
  end

  local window = file_window(path)
  vim.api.nvim_set_current_win(window)
  if vim.fn.bufnr(path) == vim.api.nvim_win_get_buf(window) then
    vim.cmd("silent! checktime")
  else
    vim.cmd.edit(vim.fn.fnameescape(path))
  end

  local buffer = vim.api.nvim_get_current_buf()
  local target = line > 0 and line
    or line_containing(vim.api.nvim_buf_get_lines(buffer, 0, -1, false), text)
    or 1
  target = math.min(target, vim.api.nvim_buf_line_count(buffer))
  vim.api.nvim_win_set_cursor(window, { target, 0 })
  vim.cmd("normal! zz")
  flash(buffer, target)
  return "shown"
end

return M
