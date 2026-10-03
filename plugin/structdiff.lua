if vim.g.loaded_structdiff then
  return
end
vim.g.loaded_structdiff = true

-- Complete ref names, including the side after ".." or "..." (main...fe<Tab>).
local function complete_range(arglead)
  local git = require("structdiff.git")
  local repo = git.repo(vim.fn.getcwd())
  if not repo then
    return {}
  end
  local prefix, rest = arglead:match("^(.-%.%.%.?)(.*)$")
  prefix, rest = prefix or "", rest or arglead
  return vim.tbl_map(function(ref)
    return prefix .. ref
  end, vim.tbl_filter(function(ref)
    return vim.startswith(ref, rest)
  end, git.refs(repo)))
end

local function cmd(name, fn, desc, range)
  vim.api.nvim_create_user_command(name, function(opts)
    require("structdiff")[fn](range and opts.args or nil)
  end, { desc = desc, nargs = range and "?" or 0, complete = range and complete_range or nil })
end

cmd("StructDiff", "open", "Open grouped diff: [rev | A..B | A...B], default HEAD vs working tree", true)
cmd("StructDiffClose", "close", "Close the StructDiff view")
cmd("StructDiffRefresh", "refresh", "Re-scan changes and reload the narrative")
cmd("StructDiffNarrative", "toggle_narrative", "Toggle the narrative split")
cmd("StructDiffGenerate", "generate", "Generate the change narrative with Claude: [range]", true)
