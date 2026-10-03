local config = require("structdiff.config")
local git = require("structdiff.git")
local group = require("structdiff.group")
local narrative = require("structdiff.narrative")
local panel = require("structdiff.panel")
local diff = require("structdiff.diff")

local M = {}
local api = vim.api

--- The active view, or nil. Fields: repo, spec, range, files, fingerprint,
--- grouped, flat, narrative, narrative_state, current (path), collapsed, tab,
--- *_win, panel_buf.
M.state = nil

local augroup = api.nvim_create_augroup("structdiff", { clear = true })

function M.setup(opts)
  config.setup(opts)
end

local function notify(msg, level)
  vim.notify("structdiff: " .. msg, level or vim.log.levels.INFO)
end

-- Group files, apply the narrative's reading order, and flatten for ]f / [f.
local function rebuild(s)
  s.grouped = group.assign(s.files, config.groups_for(s.repo.root), config.options.other_group)
  if s.narrative then
    group.sort_by_order(s.grouped, s.narrative.order)
  end
  s.flat = {}
  for gi, g in ipairs(s.grouped) do
    for fi, f in ipairs(g.files) do
      table.insert(s.flat, { gi = gi, fi = fi, file = f })
    end
  end
end

local function load_narrative(s)
  local nar, err = narrative.load(s.repo.root, s.spec)
  if err then
    notify(err, vim.log.levels.WARN)
  end
  s.narrative = nar
  s.narrative_state = narrative.state(nar, s.fingerprint)
end

-- Re-resolve the range (branches move), rescan, and export groups.json.
local function load(s)
  local range, err = git.resolve_range(s.repo, s.spec)
  if not range then
    notify(err, vim.log.levels.ERROR)
    return false
  end
  s.range = range
  narrative.ensure_excluded(s.repo.exclude)
  s.files = git.changed_files(s.repo, range)
  s.fingerprint = git.fingerprint(s.repo, range, s.files)
  load_narrative(s)
  rebuild(s)
  narrative.export_groups(s.repo.root, range, s.grouped, s.fingerprint)
  return true
end

local function index_of(s, path)
  for i, e in ipairs(s.flat) do
    if e.file.path == path then
      return i
    end
  end
end

-- Keymaps -------------------------------------------------------------------

local function map(buf, lhs, fn, desc)
  for _, l in ipairs(type(lhs) == "table" and lhs or { lhs }) do
    vim.keymap.set("n", l, fn, { buffer = buf, nowait = true, silent = true, desc = "structdiff: " .. desc })
  end
end

local function nav_keys()
  local k = config.options.keymaps
  return {
    { k.next_group, function() M.goto_group(1) end, "next group" },
    { k.prev_group, function() M.goto_group(-1) end, "previous group" },
    { k.next_file, function() M.goto_file(1) end, "next file" },
    { k.prev_file, function() M.goto_file(-1) end, "previous file" },
  }
end

local function view_keys()
  local k = config.options.keymaps
  return {
    { k.toggle_narrative, M.toggle_narrative, "toggle narrative" },
    { k.toggle_reasons, M.toggle_reasons, "toggle reasons" },
    { k.refresh, M.refresh, "refresh" },
    { k.close, M.close, "close" },
  }
end

local function map_all(buf, sets)
  for _, set in ipairs(sets) do
    for _, m in ipairs(set) do
      map(buf, m[1], m[2], m[3])
    end
  end
end

-- Real files only get navigation keys, and only while shown in the view, so
-- q / R / gn keep their normal meaning when editing.
local function unmap_real(s)
  local buf = s.real_buf
  s.real_buf = nil
  if not (buf and api.nvim_buf_is_valid(buf)) then
    return
  end
  for _, m in ipairs(nav_keys()) do
    for _, l in ipairs(type(m[1]) == "table" and m[1] or { m[1] }) do
      pcall(vim.keymap.del, "n", l, { buffer = buf })
    end
  end
end

local function map_panel(s)
  local k = config.options.keymaps
  map_all(s.panel_buf, { nav_keys(), view_keys() })
  map(s.panel_buf, k.select, function()
    local item = panel.item_at_cursor(s)
    if not item then
      return
    end
    if item.kind == "group" then
      M.toggle_fold(item.gi)
    elseif item.kind == "file" then
      M.show(index_of(s, s.grouped[item.gi].files[item.fi].path))
    end
  end, "open file / toggle group")
  map(s.panel_buf, k.toggle_fold, function()
    local item = panel.item_at_cursor(s)
    if item and item.gi then
      M.toggle_fold(item.gi)
    end
  end, "toggle group")
end

-- Narrative split -----------------------------------------------------------

local function render_narrative(s)
  if not (s.narrative_buf and api.nvim_buf_is_valid(s.narrative_buf)) then
    return
  end
  local lines = narrative.render(s.narrative, s.grouped, s.narrative_state, s.spec)
  vim.bo[s.narrative_buf].modifiable = true
  api.nvim_buf_set_lines(s.narrative_buf, 0, -1, false, lines)
  vim.bo[s.narrative_buf].modifiable = false
end

local function open_narrative(s)
  if diff.valid_win(s, s.narrative_win) then
    return
  end
  if not (s.narrative_buf and api.nvim_buf_is_valid(s.narrative_buf)) then
    s.narrative_buf = api.nvim_create_buf(false, true)
    api.nvim_buf_set_name(s.narrative_buf, "structdiff://narrative")
    vim.bo[s.narrative_buf].bufhidden = "hide"
    vim.bo[s.narrative_buf].filetype = "markdown"
    map_all(s.narrative_buf, { nav_keys(), view_keys() })
    map(s.narrative_buf, config.options.keymaps.close, M.toggle_narrative, "close narrative")
  end
  render_narrative(s)
  local cur = api.nvim_get_current_win()
  api.nvim_win_call(s.right_win, function()
    vim.cmd("botright " .. config.options.narrative_height .. "split")
    s.narrative_win = api.nvim_get_current_win()
  end)
  api.nvim_win_set_buf(s.narrative_win, s.narrative_buf)
  vim.wo[s.narrative_win].wrap = true
  vim.wo[s.narrative_win].linebreak = true
  vim.wo[s.narrative_win].winfixheight = true
  vim.wo[s.narrative_win].conceallevel = 2
  if api.nvim_win_is_valid(cur) then
    api.nvim_set_current_win(cur)
  end
end

function M.toggle_narrative()
  local s = M.state
  if not s then
    return
  end
  if diff.valid_win(s, s.narrative_win) then
    api.nvim_win_close(s.narrative_win, true)
    s.narrative_win = nil
  else
    open_narrative(s)
  end
end

-- Narrative file watcher: picks up /diff-narrative runs from any Claude session.
local function watch(s)
  local dir = narrative.dir(s.repo.root)
  vim.fn.mkdir(dir, "p")
  local handle = vim.uv.new_fs_event()
  local timer = vim.uv.new_timer()
  if not handle or not timer then
    return
  end
  local want = narrative.filename(s.spec)
  handle:start(dir, {}, function(err, fname)
    if err or fname ~= want then
      return
    end
    timer:stop()
    timer:start(200, 0, vim.schedule_wrap(function()
      if M.state == s then
        M.reload_narrative()
      end
    end))
  end)
  s.watcher, s.watch_timer = handle, timer
end

local function unwatch(s)
  for _, h in ipairs({ s.watcher, s.watch_timer }) do
    if h and not h:is_closing() then
      h:close()
    end
  end
  s.watcher, s.watch_timer = nil, nil
end

-- View ----------------------------------------------------------------------

local function redraw(s)
  panel.render(s)
  panel.focus_current(s)
  render_narrative(s)
end

--- Show the idx-th file of the flattened list.
function M.show(idx)
  local s = M.state
  local entry = s and s.flat[idx]
  if not entry then
    return
  end
  diff.ensure_layout(s)
  panel.attach(s)
  s.current = entry.file.path
  s.collapsed[s.grouped[entry.gi].name] = nil

  unmap_real(s)
  local buf, real = diff.show(s, entry.file)
  if real then
    s.real_buf = buf
    map_all(buf, { nav_keys() })
  else
    map_all(buf, { nav_keys(), view_keys() })
  end
  map_all(api.nvim_win_get_buf(s.left_win), { nav_keys(), view_keys() })
  redraw(s)
end

function M.goto_file(delta)
  local s = M.state
  if not s then
    return
  end
  local j = (index_of(s, s.current) or 0) + delta
  if j < 1 or j > #s.flat then
    notify(delta > 0 and "last file" or "first file")
    return
  end
  M.show(j)
end

function M.goto_group(delta)
  local s = M.state
  if not s then
    return
  end
  local i = index_of(s, s.current)
  local target = (i and s.flat[i].gi or 0) + delta
  if target < 1 or target > #s.grouped then
    notify(delta > 0 and "last group" or "first group")
    return
  end
  for j, e in ipairs(s.flat) do
    if e.gi == target then
      return M.show(j)
    end
  end
end

function M.toggle_fold(gi)
  local s = M.state
  local name = s.grouped[gi].name
  s.collapsed[name] = not s.collapsed[name] or nil
  panel.render(s)
  for lnum, item in pairs(s.line_items) do
    if item.kind == "group" and item.gi == gi then
      api.nvim_win_set_cursor(s.sidebar_win, { lnum, 0 })
    end
  end
end

function M.toggle_reasons()
  config.options.show_reasons = not config.options.show_reasons
  if M.state then
    redraw(M.state)
  end
end

function M.reload_narrative()
  local s = M.state
  if not s then
    return
  end
  load_narrative(s)
  rebuild(s)
  redraw(s)
end

--- Open the view. `spec` selects what to compare (see git.resolve_range):
--- nil/"" = HEAD vs working tree, "main" = main vs working tree,
--- "main..HEAD", "main...HEAD" = commits only. Reopening with the same spec
--- refreshes; a different spec replaces the view.
function M.open(spec)
  spec = vim.trim(spec or "")
  local s = M.state
  if s and s.tab and api.nvim_tabpage_is_valid(s.tab) then
    if s.spec == spec then
      api.nvim_set_current_tabpage(s.tab)
      return M.refresh()
    end
    M.close()
  end
  local repo, err = git.repo(vim.fn.getcwd())
  if not repo then
    return notify("not a git repository: " .. (err or ""), vim.log.levels.ERROR)
  end
  s = { repo = repo, spec = spec, collapsed = {} }
  if not load(s) then
    return
  end
  if #s.flat == 0 then
    local r = s.range
    return notify(("no changes in %s"):format(r.target and spec or (r.left_label .. " → working tree")))
  end
  M.state = s
  M.show(1)
  map_panel(s)
  watch(s)
  if s.narrative then
    open_narrative(s)
  end
  api.nvim_set_current_win(s.sidebar_win)
  panel.focus_current(s)
end

function M.refresh()
  local s = M.state
  if not s or not load(s) then
    return
  end
  if #s.flat == 0 then
    notify("no changes left")
    return M.close()
  end
  M.show(index_of(s, s.current) or 1)
end

local function cleanup(s)
  unwatch(s)
  unmap_real(s)
  for _, b in ipairs({ s.panel_buf, s.narrative_buf }) do
    if b and api.nvim_buf_is_valid(b) then
      pcall(api.nvim_buf_delete, b, { force = true })
    end
  end
  if M.state == s then
    M.state = nil
  end
end

function M.close()
  local s = M.state
  if not s then
    return
  end
  diff.diffoff(s)
  if s.tab and api.nvim_tabpage_is_valid(s.tab) then
    if #api.nvim_list_tabpages() == 1 then
      vim.cmd("tabnew")
    end
    vim.cmd("tabclose " .. api.nvim_tabpage_get_number(s.tab))
  end
  cleanup(s)
end

--- generate_cmd with "{range}" replaced by the current spec.
function M.generate_cmd(spec)
  return vim.tbl_map(function(arg)
    if not arg:find("{range}", 1, true) then
      return arg
    end
    return vim.trim((arg:gsub("{range}", (spec:gsub("%%", "%%%%")))))
  end, config.options.generate_cmd)
end

--- Run the diff-narrative skill (config.generate_cmd) and reload when done.
--- With `spec`, (re)opens the view on that range first.
function M.generate(spec)
  if not M.state or (spec and spec ~= "" and vim.trim(spec) ~= M.state.spec) then
    M.open(spec)
  end
  local s = M.state
  if not s then
    return
  end
  if s.generating then
    return notify("already generating")
  end
  if not load(s) then -- refresh groups.json + fingerprint for the skill
    return
  end
  s.generating = true
  redraw(s)
  notify("generating narrative…")
  local cmd = M.generate_cmd(s.spec)
  local ok, err = pcall(vim.system, cmd, { cwd = s.repo.root, text = true }, vim.schedule_wrap(function(res)
    s.generating = false
    if M.state ~= s then
      return
    end
    if res.code ~= 0 then
      local msg = vim.trim((res.stderr ~= "" and res.stderr) or res.stdout or "")
      notify("narrative generation failed: " .. msg:sub(-500), vim.log.levels.ERROR)
    else
      notify("narrative ready")
    end
    M.reload_narrative()
    if s.narrative then
      open_narrative(s)
    end
  end))
  if not ok then
    s.generating = false
    redraw(s)
    notify("cannot run " .. cmd[1] .. ": " .. tostring(err), vim.log.levels.ERROR)
  end
end

api.nvim_create_autocmd("TabClosed", {
  group = augroup,
  callback = function()
    local s = M.state
    if s and not api.nvim_tabpage_is_valid(s.tab) then
      cleanup(s)
    end
  end,
})

api.nvim_create_autocmd("ColorScheme", { group = augroup, callback = panel.set_hl })

return M
