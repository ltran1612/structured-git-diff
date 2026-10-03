-- Sidebar: groups -> files, with narrative reasons as virtual lines.
local config = require("structdiff.config")

local M = {}
local api = vim.api
local ns = api.nvim_create_namespace("structdiff")

local hl_links = {
  StructDiffTitle = "Title",
  StructDiffGroup = "Directory",
  StructDiffCount = "Comment",
  StructDiffDir = "Comment",
  StructDiffReason = "Comment",
  StructDiffWhy = "Special",
  StructDiffCurrent = "Visual",
  StructDiffAdded = "Added",
  StructDiffChanged = "Changed",
  StructDiffRemoved = "Removed",
  StructDiffFresh = "DiagnosticOk",
  StructDiffStale = "DiagnosticWarn",
  StructDiffNone = "Comment",
}
function M.set_hl()
  for name, link in pairs(hl_links) do
    api.nvim_set_hl(0, name, { link = link, default = true })
  end
end
M.set_hl()

local status_hl = {
  M = "StructDiffChanged",
  R = "StructDiffChanged",
  C = "StructDiffChanged",
  U = "StructDiffRemoved",
  A = "StructDiffAdded",
  ["?"] = "StructDiffAdded",
  D = "StructDiffRemoved",
}

local narrative_label = {
  none = { "no narrative · :StructDiffGenerate", "StructDiffNone" },
  fresh = { "narrative up to date", "StructDiffFresh" },
  stale = { "narrative stale · :StructDiffGenerate", "StructDiffStale" },
  unverified = { "narrative (unverified)", "StructDiffStale" },
  generating = { "generating narrative…", "StructDiffStale" },
}

--- Greedy word wrap.
function M.wrap(text, width)
  local out, line = {}, ""
  for word in text:gsub("%s+", " "):gmatch("%S+") do
    if line == "" then
      line = word
    elseif #line + 1 + #word <= width then
      line = line .. " " .. word
    else
      table.insert(out, line)
      line = word
    end
  end
  if line ~= "" then
    table.insert(out, line)
  end
  return out
end

local function virt(lines, indent, hl)
  return vim.tbl_map(function(l)
    return { { indent .. l, hl } }
  end, lines)
end

function M.ensure_buf(s)
  if s.panel_buf and api.nvim_buf_is_valid(s.panel_buf) then
    return s.panel_buf
  end
  local buf = api.nvim_create_buf(false, true)
  api.nvim_buf_set_name(buf, "structdiff://panel")
  vim.bo[buf].bufhidden = "hide"
  vim.bo[buf].modifiable = false
  vim.bo[buf].filetype = "structdiff"
  s.panel_buf = buf
  return buf
end

function M.attach(s)
  local buf = M.ensure_buf(s)
  local win = s.sidebar_win
  if api.nvim_win_get_buf(win) ~= buf then
    api.nvim_win_set_buf(win, buf)
  end
  local wo = vim.wo[win]
  wo.number, wo.relativenumber, wo.wrap, wo.spell, wo.list = false, false, false, false, false
  wo.signcolumn, wo.foldcolumn, wo.cursorline, wo.winfixwidth = "no", "0", true, true
  wo.statuscolumn = ""
  api.nvim_win_set_width(win, config.options.sidebar_width)
  api.nvim_win_call(win, function()
    vim.cmd("wincmd =")
  end)
end

--- Redraw the sidebar. Fills s.line_items[lnum] = { kind, gi, fi }.
function M.render(s)
  local buf = M.ensure_buf(s)
  local width = (s.sidebar_win and api.nvim_win_is_valid(s.sidebar_win)) and api.nvim_win_get_width(s.sidebar_win)
    or config.options.sidebar_width
  local nar = s.narrative
  local show_reasons = config.options.show_reasons and nar
  local lines, marks, items = {}, {}, {}

  local function add(text, item)
    table.insert(lines, text)
    items[#lines] = item
    return #lines - 1 -- 0-based row
  end

  local row = add((" StructDiff  %d file%s"):format(#s.flat, #s.flat == 1 and "" or "s"), { kind = "header" })
  table.insert(marks, { row, 0, { end_col = #lines[row + 1], hl_group = "StructDiffTitle" } })
  local label = narrative_label[s.generating and "generating" or s.narrative_state]
  row = add(" " .. label[1], { kind = "header" })
  table.insert(marks, { row, 0, { end_col = #lines[row + 1], hl_group = label[2] } })

  for gi, g in ipairs(s.grouped) do
    add("", { kind = "blank" })
    local collapsed = s.collapsed[g.name]
    local head = (collapsed and "▸ " or "▾ ") .. g.name
    row = add(head .. (" (%d)"):format(#g.files), { kind = "group", gi = gi })
    table.insert(marks, { row, 0, { end_col = #head, hl_group = "StructDiffGroup" } })
    table.insert(marks, { row, #head, { end_col = #lines[row + 1], hl_group = "StructDiffCount" } })
    local why = show_reasons and nar.groups[g.name]
    if why then
      table.insert(marks, { row, 0, { virt_lines = virt(M.wrap(why, width - 3), "  ", "StructDiffWhy") } })
    end
    if not collapsed then
      for fi, f in ipairs(g.files) do
        local name = vim.fs.basename(f.path)
        local dir = vim.fs.dirname(f.path)
        local text = ("  %s %s"):format(f.status, name)
        local start_dir = #text
        if dir ~= "." then
          text = text .. "  " .. dir
        end
        if f.old_path then
          text = text .. " ← " .. f.old_path
        end
        row = add(text, { kind = "file", gi = gi, fi = fi })
        table.insert(marks, { row, 2, { end_col = 3, hl_group = status_hl[f.status] or "StructDiffChanged" } })
        if #text > start_dir then
          table.insert(marks, { row, start_dir, { end_col = #text, hl_group = "StructDiffDir" } })
        end
        if f.path == s.current then
          table.insert(marks, { row, 0, { line_hl_group = "StructDiffCurrent" } })
        end
        local reason = show_reasons and nar.files[f.path]
        if reason then
          table.insert(marks, { row, 0, { virt_lines = virt(M.wrap(reason, width - 7), "      ", "StructDiffReason") } })
        end
      end
    end
  end

  vim.bo[buf].modifiable = true
  api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  api.nvim_buf_clear_namespace(buf, ns, 0, -1)
  for _, m in ipairs(marks) do
    api.nvim_buf_set_extmark(buf, ns, m[1], m[2], m[3])
  end
  s.line_items = items
end

function M.item_at_cursor(s)
  if not (s.sidebar_win and api.nvim_win_is_valid(s.sidebar_win)) then
    return nil
  end
  return s.line_items[api.nvim_win_get_cursor(s.sidebar_win)[1]]
end

--- Move the sidebar cursor to the current file.
function M.focus_current(s)
  if not (s.sidebar_win and api.nvim_win_is_valid(s.sidebar_win)) then
    return
  end
  for lnum, item in pairs(s.line_items) do
    if item.kind == "file" and s.grouped[item.gi].files[item.fi].path == s.current then
      api.nvim_win_set_cursor(s.sidebar_win, { lnum, 2 })
      return
    end
  end
end

return M
