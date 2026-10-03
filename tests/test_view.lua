local h = require("helpers")
local sd = require("structdiff")
local api = vim.api

local function panel_lines()
  return api.nvim_buf_get_lines(sd.state.panel_buf, 0, -1, false)
end

local function right_name()
  return vim.fn.fnamemodify(api.nvim_buf_get_name(api.nvim_win_get_buf(sd.state.right_win)), ":t")
end

local function in_view(dir, fn)
  local cwd = vim.fn.getcwd()
  vim.cmd.cd(dir)
  local ok, err = pcall(fn)
  sd.close()
  vim.cmd.cd(cwd)
  if not ok then
    error(err, 0)
  end
end

local function sample_repo()
  local dir = h.repo({
    ["lua/core.lua"] = "local M = {}\nreturn M\n",
    ["lua/util.lua"] = "return 1\n",
    ["tests/core_spec.lua"] = "-- spec\n",
    ["README.md"] = "# x\n",
  })
  h.write(dir, "lua/core.lua", "local M = {}\nM.retry = true\nreturn M\n")
  h.write(dir, "lua/backoff.lua", "return 2\n") -- untracked
  h.write(dir, "tests/core_spec.lua", "-- spec\n-- retry\n")
  os.remove(dir .. "/README.md")
  return dir
end

test("view groups files and navigates across groups", function()
  in_view(sample_repo(), function()
    sd.open()
    local s = sd.state
    eq({ "Tests", "Docs", "Source" }, vim.tbl_map(function(g) return g.name end, s.grouped))
    local lines = panel_lines()
    eq(" StructDiff  4 files", lines[1])
    eq(" HEAD → working tree", lines[2])
    assert(vim.tbl_contains(lines, "▾ Source (2)"), vim.inspect(lines))
    assert(vim.tbl_contains(lines, "  ? backoff.lua  lua"), vim.inspect(lines))
    eq("tests/core_spec.lua", s.current)
    assert(vim.wo[s.left_win].diff and vim.wo[s.right_win].diff, "diff mode on")
    eq("core_spec.lua", right_name())

    sd.goto_group(1)
    eq("README.md", s.current) -- deleted: right side is an empty scratch
    eq({ "" }, api.nvim_buf_get_lines(api.nvim_win_get_buf(s.right_win), 0, -1, false))
    eq({ "# x" }, api.nvim_buf_get_lines(api.nvim_win_get_buf(s.left_win), 0, -1, false))

    sd.goto_file(1)
    eq("lua/backoff.lua", s.current)
    eq({ "" }, api.nvim_buf_get_lines(api.nvim_win_get_buf(s.left_win), 0, -1, false))
    sd.goto_file(1)
    eq("lua/core.lua", s.current)
    eq("core.lua", right_name())
    eq("structdiff://HEAD/lua/core.lua", api.nvim_buf_get_name(api.nvim_win_get_buf(s.left_win)))
    sd.goto_group(-1)
    eq("README.md", s.current)

    -- the cursor in the sidebar follows the current file
    local lnum = api.nvim_win_get_cursor(s.sidebar_win)[1]
    eq("  D README.md", panel_lines()[lnum])

    -- .structdiff/ never shows up as a change
    for _, e in ipairs(s.flat) do
      assert(not e.file.path:match("^%.structdiff"), e.file.path)
    end
    local exported = vim.json.decode(table.concat(vim.fn.readfile(s.repo.root .. "/.structdiff/groups.json"), "\n"))
    eq(s.fingerprint, exported.fingerprint)
    eq("Tests", exported.groups[1].name)
  end)
end)

test("narrative file feeds reasons, order, and staleness", function()
  local dir = sample_repo()
  in_view(dir, function()
    sd.open()
    local fp = sd.state.fingerprint
    h.write(dir, ".structdiff/narrative.json", vim.json.encode({
      version = 1, fingerprint = fp, overall = "Add retry.",
      groups = { Source = "Retry needs a backoff helper." },
      files = { ["lua/core.lua"] = "turns retry on", ["lua/backoff.lua"] = "new helper" },
      order = { "lua/core.lua", "lua/backoff.lua" },
    }))
    sd.reload_narrative()
    local s = sd.state
    eq("fresh", s.narrative_state)
    eq({ "Source", "Tests", "Docs" }, vim.tbl_map(function(g) return g.name end, s.grouped))
    eq({ "lua/core.lua", "lua/backoff.lua" }, vim.tbl_map(function(f) return f.path end, s.grouped[1].files))
    local virt = {}
    for _, m in ipairs(api.nvim_buf_get_extmarks(s.panel_buf, -1, 0, -1, { details = true })) do
      for _, vl in ipairs(m[4].virt_lines or {}) do
        table.insert(virt, vim.trim(vl[1][1]))
      end
    end
    assert(vim.tbl_contains(virt, "turns retry on"), vim.inspect(virt))
    assert(vim.tbl_contains(virt, "Retry needs a backoff helper."), vim.inspect(virt))

    sd.toggle_narrative()
    assert(api.nvim_win_is_valid(s.narrative_win))
    eq(s.narrative_buf, api.nvim_win_get_buf(s.narrative_win))
    local nlines = api.nvim_buf_get_lines(s.narrative_buf, 0, -1, false)
    assert(vim.tbl_contains(nlines, "Add retry."), vim.inspect(nlines))

    h.write(dir, "lua/util.lua", "return 3\n")
    sd.refresh()
    eq("stale", sd.state.narrative_state)
    assert(panel_lines()[3]:match("stale"), panel_lines()[3])
  end)
end)

test("close removes the tab and real-buffer keymaps", function()
  in_view(sample_repo(), function()
    local tabs = #api.nvim_list_tabpages()
    sd.open()
    sd.goto_group(2) -- lua/backoff.lua, a real buffer
    local buf = sd.state.real_buf
    local mapped = vim.tbl_filter(function(m)
      return (m.desc or ""):match("^structdiff") ~= nil
    end, api.nvim_buf_get_keymap(buf, "n"))
    eq(4, #mapped) -- navigation only; q / R / gn stay untouched on real files
    sd.close()
    eq(nil, sd.state)
    eq(tabs, #api.nvim_list_tabpages())
    for _, m in ipairs(api.nvim_buf_get_keymap(buf, "n")) do
      assert(not (m.desc or ""):match("^structdiff"), m.lhs)
    end
  end)
end)

test("layout is rebuilt when a diff window is closed", function()
  in_view(sample_repo(), function()
    sd.open()
    api.nvim_win_close(sd.state.left_win, true)
    sd.goto_file(1)
    local s = sd.state
    assert(api.nvim_win_is_valid(s.left_win) and api.nvim_win_is_valid(s.sidebar_win))
    assert(vim.wo[s.left_win].diff)
  end)
end)

test("branch range: read-only revisions on both sides, own narrative file", function()
  local dir = branch_repo() -- from test_git.lua
  in_view(dir, function()
    sd.open("main...HEAD")
    local s = sd.state
    eq(" main...HEAD", panel_lines()[2])
    eq({ "gone.md", "a.lua", "new.lua" }, vim.tbl_map(function(e) return e.file.path end, s.flat))
    eq({ "" }, api.nvim_buf_get_lines(api.nvim_win_get_buf(s.right_win), 0, -1, false)) -- deleted on the branch
    eq({ "g" }, api.nvim_buf_get_lines(api.nvim_win_get_buf(s.left_win), 0, -1, false))
    sd.goto_file(1)
    eq("a.lua", s.current)
    eq("lua", vim.bo[api.nvim_win_get_buf(s.right_win)].filetype)
    local right = api.nvim_win_get_buf(s.right_win)
    eq("structdiff://HEAD/a.lua", api.nvim_buf_get_name(right))
    eq({ "a feature" }, api.nvim_buf_get_lines(right, 0, -1, false)) -- committed, not the worktree edit
    eq(false, vim.bo[right].modifiable)
    eq("structdiff://merge-base(main)/a.lua", api.nvim_buf_get_name(api.nvim_win_get_buf(s.left_win)))
    eq({ "a" }, api.nvim_buf_get_lines(api.nvim_win_get_buf(s.left_win), 0, -1, false))
    eq(nil, s.real_buf)

    local exported = vim.json.decode(table.concat(vim.fn.readfile(dir .. "/.structdiff/groups.json"), "\n"))
    eq("main...HEAD", exported.range.spec)
    eq(s.range.base, exported.range.base)
    eq(".structdiff/narrative-main...HEAD.json", exported.output)

    -- a narrative for the working tree must not leak into the branch view
    h.write(dir, ".structdiff/narrative.json", vim.json.encode({ version = 1, overall = "worktree story" }))
    sd.reload_narrative()
    eq("none", s.narrative_state)
    h.write(dir, ".structdiff/narrative-main...HEAD.json", vim.json.encode({ version = 1, fingerprint = s.fingerprint, overall = "branch story" }))
    sd.reload_narrative()
    eq("fresh", s.narrative_state)

    -- uncommitted edits don't make a branch narrative stale
    h.write(dir, "a.lua", "edited again\n")
    sd.refresh()
    eq("fresh", sd.state.narrative_state)

    -- switching range replaces the view
    sd.open("")
    assert(sd.state ~= s)
    eq(" HEAD → working tree", panel_lines()[2])
  end)
end)

test("generate_cmd substitutes the range", function()
  local cmd = sd.generate_cmd("main...HEAD")
  eq("/diff-narrative main...HEAD", cmd[3])
  eq("/diff-narrative", sd.generate_cmd("")[3])
end)

test("open reports an unknown revision without opening a tab", function()
  in_view(sample_repo(), function()
    local tabs = #api.nvim_list_tabpages()
    local msgs = {}
    local notify = vim.notify
    vim.notify = function(m) table.insert(msgs, m) end
    sd.open("nope...HEAD")
    vim.notify = notify
    eq(nil, sd.state)
    eq(tabs, #api.nvim_list_tabpages())
    eq({ "structdiff: unknown revision: nope" }, msgs)
  end)
end)
