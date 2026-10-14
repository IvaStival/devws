-- The tab's close button is a single cell, and in iTerm the click lands a little
-- off where the × is drawn, so it was easy to miss. Bufferline makes the whole
-- close icon clickable: padding it with spaces gives a three-cell target.
-- Middle-click closes a tab as in a browser (right-click already does, in LazyVim).
return {
  "akinsho/bufferline.nvim",
  opts = {
    options = {
      buffer_close_icon = " \u{EA76} ",
      middle_mouse_command = function(buffer)
        Snacks.bufdelete(buffer)
      end,
    },
  },
}
