-- Open the file explorer (nvim-tree) automatically when lvim is started on a directory,
-- so it behaves like an IDE (e.g. `lvim .`).
lvim.builtin.nvimtree.setup.view.width = 30
lvim.builtin.nvimtree.setup.view.side = "left"

-- nvim-tree bundled with LunarVim calls vim.diagnostic.is_disabled(), which
-- was removed in Neovim 0.12 in favor of vim.diagnostic.is_enabled().
if vim.diagnostic.is_disabled == nil and vim.diagnostic.is_enabled ~= nil then
  vim.diagnostic.is_disabled = function(bufnr, namespace)
    local filter = {}
    if bufnr ~= nil then
      filter.bufnr = bufnr
    end
    if namespace ~= nil then
      filter.ns_id = namespace
    end
    return not vim.diagnostic.is_enabled(filter)
  end
end

vim.api.nvim_create_autocmd({ "VimEnter" }, {
  callback = function(data)
    local directory = vim.fn.isdirectory(data.file) == 1

    if not directory then
      return
    end

    vim.cmd.cd(data.file)
    require("nvim-tree.api").tree.open()
  end,
})

-- indent-blankline (v2.20.8, bundled with LunarVim's core plugins.lua) crashes on
-- Neovim 0.12.4: its treesitter-based indent detection (init.lua:476, ts_indent.get_indent)
-- calls vim.treesitter.get_node_text/get_range in a way that no longer exists in nvim's
-- current treesitter API. LunarVim itself is stalled on branch release-1.4/neovim-0.9
-- (last commit May 2024, targets Neovim 0.9) so this won't be fixed upstream.
-- Disabling treesitter-based indent/context detection avoids the crash; indent guides
-- still render, just computed from raw whitespace instead of the syntax tree.
lvim.builtin.indentlines.options.use_treesitter = false
lvim.builtin.indentlines.options.show_current_context = false

-- nvim-treesitter (bundled with LunarVim) crashes on Neovim 0.12: `match[id]` inside
-- query predicate/directive callbacks changed from a single TSNode to a list of
-- TSNodes (to support quantified captures), and nvim-treesitter's own callbacks
-- (query_predicates.lua) still call node methods directly on that list, plus a
-- separate Neovim-core treesitter-engine crash on markdown's `conceal_lines` query
-- directive. Both are confirmed upstream, closed "not planned":
--   https://github.com/neovim/neovim/issues/39032
--   https://github.com/nvim-treesitter/nvim-treesitter/issues/8636
-- Rather than hand-patch the plugin's own files (which :TSUpdate/:Lazy sync would
-- silently revert), both are fixed here at runtime, applied EAGERLY at startup
-- (not on lazy.nvim's `User LazyLoad` event, which fires via vim.schedule() —
-- i.e. deferred to the next event-loop tick, which is AFTER the first buffer's
-- initial treesitter highlight attempt already crashed synchronously; confirmed
-- by testing — the LazyLoad-event version self-healed after ~1 tick but still
-- crashed once on the very first file opened). Eagerly requiring nvim-treesitter's
-- predicates submodule ourselves — and immediately overriding what it registers —
-- runs synchronously at startup, before any buffer/highlighter exists, so there's
-- no race window at all. require() is idempotent (module-cached), so when
-- lazy.nvim later "officially" lazy-loads nvim-treesitter for real (on first
-- buffer needing it), requiring this same submodule again is a no-op and does
-- NOT re-run its (buggy) top-level registrations.
--   1. Re-register the 6 affected predicates/directives with force = true, routed
--      through a get_node() unwrap helper (restores the old single-node shape).
--   2. Fully replace markdown's highlights query via vim.treesitter.query.set() —
--      the officially documented override API. A passive file-drop override
--      wouldn't work here: nvim-treesitter's plugin dir gets PREPENDED to
--      runtimepath at lazy-load time, ranking ahead of ~/.config/lvim, so it would
--      win Neovim's "first file found" query resolution regardless; query.set() is
--      checked before file-based resolution, sidestepping that entirely.
-- The replacement query text (queries/markdown/highlights.scm, deployed alongside
-- this file) is upstream's content with the two crashing `(#set! conceal_lines "")`
-- directives removed (sibling `(#set! conceal "")` is unaffected and kept).
do
  local runtime_dir = os.getenv("LUNARVIM_RUNTIME_DIR") or (os.getenv("HOME") .. "/.local/share/lunarvim")
  local ts_path = runtime_dir .. "/site/pack/lazy/opt/nvim-treesitter"
  vim.opt.rtp:prepend(ts_path)
  -- Loading this submodule directly runs nvim-treesitter's own (buggy, pristine)
  -- add_predicate/add_directive calls; we override them immediately below.
  require("nvim-treesitter.query_predicates")

  do
    local ts_query = vim.treesitter.query

    local html_script_type_languages = {
      ["importmap"] = "json",
      ["module"] = "javascript",
      ["application/ecmascript"] = "javascript",
      ["text/ecmascript"] = "javascript",
    }
    local non_filetype_match_injection_language_aliases = {
      ex = "elixir",
      pl = "perl",
      sh = "bash",
      uxn = "uxntal",
      ts = "typescript",
    }
    local function get_parser_from_markdown_info_string(injection_alias)
      local match = vim.filetype.match({ filename = "a." .. injection_alias })
      return match or non_filetype_match_injection_language_aliases[injection_alias] or injection_alias
    end

    local function get_node(match, id)
      local val = match[id]
      if not val then
        return nil
      end
      if type(val) == "table" then
        return val[1]
      end
      return val
    end

    local function valid_args(name, pred, count, strict_count)
      local arg_count = #pred - 1
      if strict_count then
        if arg_count ~= count then
          vim.api.nvim_err_writeln(string.format("%s must have exactly %d arguments", name, count))
          return false
        end
      elseif arg_count < count then
        vim.api.nvim_err_writeln(string.format("%s must have at least %d arguments", name, count))
        return false
      end
      return true
    end

    ts_query.add_predicate("nth?", function(match, _pattern, _bufnr, pred)
      if not valid_args("nth?", pred, 2, true) then
        return
      end
      local node = get_node(match, pred[2])
      local n = tonumber(pred[3])
      if node and node:parent() and node:parent():named_child_count() > n then
        return node:parent():named_child(n) == node
      end
      return false
    end, { force = true })

    ts_query.add_predicate("is?", function(match, _pattern, bufnr, pred)
      if not valid_args("is?", pred, 2) then
        return
      end
      local locals = require("nvim-treesitter.locals")
      local node = get_node(match, pred[2])
      local types = { unpack(pred, 3) }
      if not node then
        return true
      end
      local _, _, kind = locals.find_definition(node, bufnr)
      return vim.tbl_contains(types, kind)
    end, { force = true })

    ts_query.add_predicate("kind-eq?", function(match, _pattern, _bufnr, pred)
      if not valid_args(pred[1], pred, 2) then
        return
      end
      local node = get_node(match, pred[2])
      local types = { unpack(pred, 3) }
      if not node then
        return true
      end
      return vim.tbl_contains(types, node:type())
    end, { force = true })

    ts_query.add_directive("set-lang-from-mimetype!", function(match, _, bufnr, pred, metadata)
      local node = get_node(match, pred[2])
      if not node then
        return
      end
      local type_attr_value = vim.treesitter.get_node_text(node, bufnr)
      local configured = html_script_type_languages[type_attr_value]
      if configured then
        metadata["injection.language"] = configured
      else
        local parts = vim.split(type_attr_value, "/", {})
        metadata["injection.language"] = parts[#parts]
      end
    end, { force = true })

    ts_query.add_directive("set-lang-from-info-string!", function(match, _, bufnr, pred, metadata)
      local node = get_node(match, pred[2])
      if not node then
        return
      end
      local injection_alias = vim.treesitter.get_node_text(node, bufnr):lower()
      metadata["injection.language"] = get_parser_from_markdown_info_string(injection_alias)
    end, { force = true })

    ts_query.add_directive("downcase!", function(match, _, bufnr, pred, metadata)
      local id = pred[2]
      local node = get_node(match, id)
      if not node then
        return
      end
      local text = vim.treesitter.get_node_text(node, bufnr, { metadata = metadata[id] }) or ""
      if not metadata[id] then
        metadata[id] = {}
      end
      metadata[id].text = string.lower(text)
    end, { force = true })

  end

  local config_dir = os.getenv("LUNARVIM_CONFIG_DIR") or (os.getenv("HOME") .. "/.config/lvim")
  local f = io.open(config_dir .. "/queries/markdown/highlights.scm", "r")
  if f then
    vim.treesitter.query.set("markdown", "highlights", f:read("*a"))
    f:close()
  end
end

-- intelephense (main PHP server) does not implement textDocument/implementation
-- (server_capabilities.implementationProvider = false). phpactor does, so it's added
-- alongside intelephense just for "Goto Implementation" (gI); its diagnostics are
-- suppressed to avoid duplicating intelephense's.
vim.api.nvim_create_autocmd("FileType", {
  pattern = "php",
  callback = function()
    require("lvim.lsp.manager").setup("phpactor", {
      handlers = {
        ["textDocument/publishDiagnostics"] = function() end,
      },
    })
  end,
})
