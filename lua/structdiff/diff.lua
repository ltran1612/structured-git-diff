-- Tab layout and the native side-by-side diff:
--   | sidebar | base (HEAD) | working tree |
local git = require("structdiff.git")

local M = {}
local api = vim.api

local function valid_win(s, win)
  return win and api.nvim_win_is_valid(win) and api.nvim_win_get_tabpage(win) == s.tab
end
M.valid_win = valid_win

--- Create the tab, or rebuild its windows if the user closed some of them.
function M.ensure_layout(s)
  if s.tab and api.nvim_tabpage_is_valid(s.tab) then
    if valid_win(s, s.sidebar_win) and valid_win(s, s.left_win) and valid_win(s, s.right_win) then
      return
    end
    api.nvim_set_current_tabpage(s.tab)
    pcall(vim.cmd, "only")
  else
    vim.cmd("tabnew")
    s.tab = api.nvim_get_current_tabpage()
    vim.bo.bufhidden = "wipe"
  end
  s.right_win = api.nvim_get_current_win()
  vim.cmd("leftabove vsplit")
  s.left_win = api.nvim_get_current_win()
  vim.cmd("topleft vsplit")
  s.sidebar_win = api.nvim_get_current_win()
  s.narrative_win = nil
end

local function is_binary(content)
  return content:sub(1, 8000):find("\0", 1, true) ~= nil
end

local function to_lines(content)
  if is_binary(content) then
    return { "[binary file]" }
  end
  local lines = vim.split(content, "\n", { plain = true })
  if lines[#lines] == "" then
    table.remove(lines)
  end
  return lines
end

local function find_buf(name)
  for _, b in ipairs(api.nvim_list_bufs()) do
    if api.nvim_buf_get_name(b) == name then
      return b
    end
  end
end

--- A read-only scratch buffer named `name`, reused if it already exists.
function M.scratch(name, lines, path)
  local buf = find_buf(name)
  if not buf then
    buf = api.nvim_create_buf(false, true)
    api.nvim_buf_set_name(buf, name)
    vim.bo[buf].bufhidden = "wipe"
  end
  vim.bo[buf].modifiable = true
  api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  vim.bo[buf].modified = false
  if path then
    local ft = vim.filetype.match({ buf = buf, filename = path })
    if ft and vim.bo[buf].filetype ~= ft then
      vim.bo[buf].filetype = ft
    end
  end
  return buf
end

local function read_head(abs)
  local fd = io.open(abs, "rb")
  if not fd then
    return nil
  end
  local head = fd:read(8000) or ""
  fd:close()
  return head
end

local function rev_lines(s, rev, path)
  local content = git.content(s.repo, rev, path)
  return content and to_lines(content) or {}
end

--- Show `file` in the diff windows. Returns the right-hand buffer and whether
--- it is the real file (as opposed to a scratch buffer). The right side is the
--- real file only when comparing against the working tree.
function M.show(s, file)
  local r = s.range

  local left_lines = {}
  if file.status ~= "?" and file.status ~= "A" then
    left_lines = rev_lines(s, r.base, file.old_path or file.path)
  end
  local left = M.scratch(("structdiff://%s/%s"):format(r.left_label, file.path), left_lines, file.path)

  api.nvim_win_call(s.left_win, function()
    vim.cmd("diffoff!")
  end)
  api.nvim_win_set_buf(s.left_win, left)

  local right, real = nil, false
  local abs = s.repo.root .. "/" .. file.path
  local head = not r.target and file.status ~= "D" and read_head(abs)
  if r.target then
    local lines = file.status == "D" and {} or rev_lines(s, r.target, file.path)
    right = M.scratch(("structdiff://%s/%s"):format(r.right_label, file.path), lines, file.path)
    api.nvim_win_set_buf(s.right_win, right)
  elseif not head then
    right = M.scratch("structdiff://deleted/" .. file.path, {}, file.path)
    api.nvim_win_set_buf(s.right_win, right)
  elseif is_binary(head) then
    right = M.scratch("structdiff://worktree/" .. file.path, { "[binary file]" })
    api.nvim_win_set_buf(s.right_win, right)
  else
    local ok, err = pcall(api.nvim_win_call, s.right_win, function()
      vim.cmd("edit " .. vim.fn.fnameescape(abs))
    end)
    if not ok then
      vim.notify("structdiff: " .. tostring(err), vim.log.levels.ERROR)
    end
    right, real = api.nvim_win_get_buf(s.right_win), true
  end

  for _, win in ipairs({ s.left_win, s.right_win }) do
    api.nvim_win_call(win, function()
      vim.cmd("diffthis")
    end)
  end
  api.nvim_win_call(s.right_win, function()
    vim.cmd("normal! gg")
    if vim.fn.diff_hlID(1, 1) == 0 then
      pcall(vim.cmd, "normal! ]c")
    end
  end)
  return right, real
end

function M.diffoff(s)
  if valid_win(s, s.left_win) then
    api.nvim_win_call(s.left_win, function()
      vim.cmd("diffoff!")
    end)
  end
end

return M
