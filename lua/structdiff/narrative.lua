-- The narrative contract: the diff-narrative skill writes
-- <root>/.structdiff/narrative.json; this module reads it. The viewer writes
-- <root>/.structdiff/groups.json so the skill uses the same grouping.
local M = {}

M.VERSION = 1

function M.dir(root)
  return root .. "/.structdiff"
end

function M.path(root)
  return M.dir(root) .. "/narrative.json"
end

--- Keep .structdiff/ out of `git status` without touching .gitignore.
function M.ensure_excluded(exclude_file)
  local line = "/.structdiff/"
  local fd = io.open(exclude_file, "r")
  if fd then
    for l in fd:lines() do
      if l == line then
        fd:close()
        return
      end
    end
    fd:close()
  end
  vim.fn.mkdir(vim.fs.dirname(exclude_file), "p")
  fd = assert(io.open(exclude_file, "a"))
  fd:write("\n# structdiff.nvim scratch data\n" .. line .. "\n")
  fd:close()
end

local function write_json(path, data)
  vim.fn.mkdir(vim.fs.dirname(path), "p")
  local fd = assert(io.open(path, "w"))
  fd:write(vim.json.encode(data))
  fd:close()
end

function M.export_groups(root, grouped, fingerprint)
  local out = { version = M.VERSION, fingerprint = fingerprint, groups = {} }
  for _, g in ipairs(grouped) do
    local files = {}
    for _, f in ipairs(g.files) do
      table.insert(files, { path = f.path, status = f.status, old_path = f.old_path })
    end
    table.insert(out.groups, { name = g.name, files = files })
  end
  write_json(M.dir(root) .. "/groups.json", out)
end

local function str(v)
  return type(v) == "string" and v or nil
end

local function str_map(v)
  local out = {}
  if type(v) == "table" then
    for k, s in pairs(v) do
      if type(k) == "string" and type(s) == "string" then
        out[k] = s
      end
    end
  end
  return out
end

--- Normalize decoded JSON into the shape the viewer uses. Unknown keys and
--- wrongly typed fields are dropped.
function M.normalize(data)
  if type(data) ~= "table" then
    return nil, "not a JSON object"
  end
  local order = {}
  if type(data.order) == "table" then
    for _, p in ipairs(data.order) do
      if type(p) == "string" then
        table.insert(order, p)
      end
    end
  end
  return {
    fingerprint = str(data.fingerprint),
    overall = str(data.overall) or "",
    groups = str_map(data.groups),
    files = str_map(data.files),
    order = order,
  }
end

--- @return table|nil narrative, string|nil error (nil, nil when absent)
function M.load(root)
  local fd = io.open(M.path(root), "r")
  if not fd then
    return nil, nil
  end
  local text = fd:read("*a")
  fd:close()
  local ok, data = pcall(vim.json.decode, text)
  if not ok then
    return nil, "invalid JSON in " .. M.path(root)
  end
  return M.normalize(data)
end

--- "none" | "fresh" | "stale" | "unverified"
function M.state(narrative, fingerprint)
  if not narrative then
    return "none"
  end
  if not narrative.fingerprint or narrative.fingerprint == "" then
    return "unverified"
  end
  return narrative.fingerprint == fingerprint and "fresh" or "stale"
end

--- Markdown for the narrative split.
function M.render(narrative, grouped, state)
  local lines = { "# Change narrative" }
  if state == "stale" then
    vim.list_extend(lines, { "", "> **Stale:** the diff changed since this was written. Run `:StructDiffGenerate`." })
  elseif state == "unverified" then
    vim.list_extend(lines, { "", "> Written without a fingerprint; it may not match the current diff." })
  end
  if not narrative then
    vim.list_extend(lines, { "", "No narrative yet. Run `:StructDiffGenerate` or `/diff-narrative` in Claude Code." })
    return lines
  end
  if narrative.overall ~= "" then
    table.insert(lines, "")
    vim.list_extend(lines, vim.split(narrative.overall, "\n", { plain = true }))
  end
  for _, g in ipairs(grouped) do
    vim.list_extend(lines, { "", "## " .. g.name })
    local why = narrative.groups[g.name]
    if why then
      table.insert(lines, "")
      vim.list_extend(lines, vim.split(why, "\n", { plain = true }))
    end
    table.insert(lines, "")
    for _, f in ipairs(g.files) do
      local reason = narrative.files[f.path]
      table.insert(lines, ("- `%s`%s"):format(f.path, reason and (" — " .. reason) or ""))
    end
  end
  return lines
end

return M
