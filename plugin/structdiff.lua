if vim.g.loaded_structdiff then
  return
end
vim.g.loaded_structdiff = true

local function cmd(name, fn, desc)
  vim.api.nvim_create_user_command(name, function()
    require("structdiff")[fn]()
  end, { desc = desc })
end

cmd("StructDiff", "open", "Open grouped working-tree diff")
cmd("StructDiffClose", "close", "Close the StructDiff view")
cmd("StructDiffRefresh", "refresh", "Re-scan changes and reload the narrative")
cmd("StructDiffNarrative", "toggle_narrative", "Toggle the narrative split")
cmd("StructDiffGenerate", "generate", "Generate the change narrative with Claude")
