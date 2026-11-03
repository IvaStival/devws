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

local function is_floating(window)
  return vim.api.nvim_win_get_config(window).relative ~= ""
end

local function shows_a_file(window)
  return not is_floating(window) and vim.bo[vim.api.nvim_win_get_buf(window)].buftype == ""
end

-- The window to show `path` in, never a floating one (the explorer's list,
-- a picker): one showing it already, else the current one if it shows a
-- file, else any that does. With none (LazyVim opened on a folder shows only
-- its dashboard and the explorer), the largest window that is not part of a
-- sidebar, such as the dashboard's.
local function file_window(path)
  local buffer = vim.fn.bufnr(path)
  if buffer ~= -1 then
    for _, shown in ipairs(vim.fn.win_findbuf(buffer)) do
      if not is_floating(shown) then
        return shown
      end
    end
  end
  local current = vim.api.nvim_get_current_win()
  if shows_a_file(current) then
    return current
  end
  local windows = vim.api.nvim_tabpage_list_wins(0)
  for _, window in ipairs(windows) do
    if shows_a_file(window) then
      return window
    end
  end
  local largest, largest_area = nil, -1
  for _, window in ipairs(windows) do
    local filetype = vim.bo[vim.api.nvim_win_get_buf(window)].filetype
    local area = vim.api.nvim_win_get_width(window) * vim.api.nvim_win_get_height(window)
    if not is_floating(window) and not filetype:match("^snacks_layout") and area > largest_area then
      largest, largest_area = window, area
    end
  end
  return largest or current
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

-- The lines the agent changed since the user's last prompt, by file:
-- { [path] = { { first, last }, ... } }, marked in the sign column of each
-- file shown, now or later (devws's hooks send them, agent_changes.sh).
local marks_namespace = vim.api.nvim_create_namespace("devws_follow_marks")
local changed = {}
vim.api.nvim_set_hl(0, "DevwsAgentChange", { link = "Changed", default = true })

-- A file's one name: Neovim names buffers by their real path (macOS's /var
-- is /private/var), the hooks may not.
local function real_path(path)
  return vim.fs.normalize(vim.fn.resolve(vim.fn.fnamemodify(path, ":p")))
end

local function mark_buffer(buffer)
  if not vim.api.nvim_buf_is_loaded(buffer) then
    return
  end
  vim.api.nvim_buf_clear_namespace(buffer, marks_namespace, 0, -1)
  local ranges = changed[real_path(vim.api.nvim_buf_get_name(buffer))]
  if not ranges then
    return
  end
  local count = vim.api.nvim_buf_line_count(buffer)
  for _, range in ipairs(ranges) do
    for line = range[1], math.min(range[2], count) do
      vim.api.nvim_buf_set_extmark(buffer, marks_namespace, line - 1, 0, {
        sign_text = "▍",
        sign_hl_group = "DevwsAgentChange",
      })
    end
  end
end

-- A file read again (opened, or reloaded after a change on disk) loses its
-- marks with its old lines: they are drawn again.
vim.api.nvim_create_autocmd({ "BufReadPost", "FileChangedShellPost" }, {
  group = vim.api.nvim_create_augroup("devws_follow_marks", { clear = true }),
  callback = function(event)
    mark_buffer(event.buf)
  end,
})

--- Marks `ranges` ({ { path, first, last }, ... }) as changed by the agent.
function M.mark(ranges)
  local touched = {}
  for _, range in ipairs(ranges) do
    local path = real_path(range[1])
    changed[path] = changed[path] or {}
    table.insert(changed[path], { range[2], range[3] })
    touched[path] = true
  end
  for path in pairs(touched) do
    local buffer = vim.fn.bufnr(path)
    if buffer ~= -1 then
      mark_buffer(buffer)
    end
  end
  return vim.tbl_count(touched)
end

--- Marks `ranges` and shows `path` (the file changed last) at `line`, as
--- show() does. Returns what show() returns.
function M.show_changes(ranges, path, line, notify_only, stamp)
  M.mark(ranges)
  return M.show(path, line, "", notify_only, stamp)
end

--- Clears every mark: the user's new prompt starts afresh.
function M.clear_marks()
  changed = {}
  for _, buffer in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(buffer) then
      vim.api.nvim_buf_clear_namespace(buffer, marks_namespace, 0, -1)
    end
  end
  return 0
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

  -- The window's own buffer: a plugin may have moved the focus while the
  -- file opened, and the cursor and the flash belong where the file is.
  vim.api.nvim_set_current_win(window)
  local buffer = vim.api.nvim_win_get_buf(window)
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
