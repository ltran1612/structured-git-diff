local M = {}

--- Compile group definitions into { name, regexes } entries. Invalid patterns
--- are reported and skipped rather than failing the whole view.
function M.compile(groups)
  local compiled = {}
  for _, g in ipairs(groups) do
    local regexes = {}
    for _, pat in ipairs(g.patterns or {}) do
      local ok, re = pcall(vim.regex, "\\v" .. pat)
      if ok then
        table.insert(regexes, re)
      else
        vim.notify(("structdiff: bad pattern in group %q: %s"):format(g.name, pat), vim.log.levels.WARN)
      end
    end
    table.insert(compiled, { name = g.name, regexes = regexes })
  end
  return compiled
end

function M.match(compiled, path)
  for _, g in ipairs(compiled) do
    for _, re in ipairs(g.regexes) do
      if re:match_str(path) then
        return g.name
      end
    end
  end
end

--- Split files into non-empty groups, in config order, with the fallback last.
--- @param files table[] entries with a `path` field
--- @return table[] { name = string, files = table[] }
function M.assign(files, groups, other_name)
  local compiled = M.compile(groups)
  local by_name, result = {}, {}
  for _, g in ipairs(compiled) do
    by_name[g.name] = { name = g.name, files = {} }
    table.insert(result, by_name[g.name])
  end
  local other = { name = other_name, files = {} }
  for _, f in ipairs(files) do
    local name = M.match(compiled, f.path)
    table.insert(name and by_name[name].files or other.files, f)
  end
  table.insert(result, other)
  return vim.tbl_filter(function(g)
    return #g.files > 0
  end, result)
end

--- Stable-sort each group's files by their position in `order` (files not in
--- it keep their relative order after the listed ones), then sort the groups
--- by their earliest file so the story's root cause comes first.
function M.sort_by_order(grouped, order)
  if type(order) ~= "table" or #order == 0 then
    return
  end
  local rank = {}
  for i, p in ipairs(order) do
    rank[p] = rank[p] or i
  end
  for _, g in ipairs(grouped) do
    local keyed = {}
    for i, f in ipairs(g.files) do
      keyed[i] = { f = f, r = rank[f.path] or math.huge, i = i }
    end
    table.sort(keyed, function(a, b)
      if a.r ~= b.r then
        return a.r < b.r
      end
      return a.i < b.i
    end)
    for i, k in ipairs(keyed) do
      g.files[i] = k.f
    end
    g.rank = keyed[1] and keyed[1].r or math.huge
  end
  local idx = {}
  for i, g in ipairs(grouped) do
    idx[g] = i
  end
  table.sort(grouped, function(a, b)
    if a.rank ~= b.rank then
      return a.rank < b.rank
    end
    return idx[a] < idx[b]
  end)
end

return M
