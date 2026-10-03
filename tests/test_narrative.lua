local narrative = require("structdiff.narrative")
local h = require("helpers")

test("load: absent, invalid, and normalized", function()
  local dir = h.tmpdir()
  eq({ nil, nil }, { narrative.load(dir) })
  h.write(dir, ".structdiff/narrative.json", "{not json")
  local n, err = narrative.load(dir)
  eq(nil, n)
  assert(err:match("invalid JSON"))
  h.write(dir, ".structdiff/narrative.json", vim.json.encode({
    version = 1, fingerprint = "abc", overall = "story",
    groups = { Source = "why", Bad = 3 }, files = { ["a.lua"] = "reason" },
    order = { "a.lua", 7 }, extra = true,
  }))
  eq({
    fingerprint = "abc", overall = "story", groups = { Source = "why" },
    files = { ["a.lua"] = "reason" }, order = { "a.lua" },
  }, narrative.load(dir))
end)

test("state", function()
  eq("none", narrative.state(nil, "x"))
  eq("unverified", narrative.state({ fingerprint = nil }, "x"))
  eq("fresh", narrative.state({ fingerprint = "x" }, "x"))
  eq("stale", narrative.state({ fingerprint = "y" }, "x"))
end)

test("ensure_excluded is idempotent", function()
  local dir = h.tmpdir()
  local ex = dir .. "/info/exclude"
  narrative.ensure_excluded(ex)
  narrative.ensure_excluded(ex)
  local text = table.concat(vim.fn.readfile(ex), "\n")
  local _, n = text:gsub("/%.structdiff/", "")
  eq(1, n)
end)

test("render includes overall, group why, and file reasons", function()
  local lines = narrative.render(
    { overall = "Big picture.", groups = { Source = "Because." }, files = { ["a.lua"] = "adds x" }, order = {} },
    { { name = "Source", files = { { path = "a.lua" }, { path = "b.lua" } } } },
    "stale"
  )
  local text = table.concat(lines, "\n")
  assert(text:find("Stale", 1, true))
  assert(text:find("Big picture.", 1, true))
  assert(text:find("## Source\n\nBecause.", 1, true))
  assert(text:find("- `a.lua` — adds x", 1, true))
  assert(text:find("- `b.lua`\n", 1, true) or text:sub(-9) == "- `b.lua`")
end)

test("narrative file name per range", function()
  eq("narrative.json", narrative.filename(""))
  eq("narrative.json", narrative.filename(nil))
  eq("narrative-main...HEAD.json", narrative.filename("main...HEAD"))
  eq("narrative-origin_main..HEAD_1.json", narrative.filename("origin/main..HEAD~1"))
end)
