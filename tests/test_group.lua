local config = require("structdiff.config")
local group = require("structdiff.group")

local function names(grouped)
  local out = {}
  for _, g in ipairs(grouped) do
    out[g.name] = vim.tbl_map(function(f) return f.path end, g.files)
  end
  return out
end

local function files(...)
  return vim.tbl_map(function(p) return { path = p } end, { ... })
end

test("default groups classify common paths", function()
  local g = group.assign(
    files("lua/foo.lua", "tests/foo_spec.lua", "src/x_test.go", "README.md", "package.json",
      ".github/workflows/ci.yml", "assets/logo.png", "docs/guide.txt", "requirements-dev.txt"),
    config.defaults.groups,
    "Other"
  )
  eq({
    Source = { "lua/foo.lua" },
    Tests = { "tests/foo_spec.lua", "src/x_test.go" },
    Docs = { "README.md", "docs/guide.txt" },
    ["Config/Build"] = { "package.json", "requirements-dev.txt" },
    CI = { ".github/workflows/ci.yml" },
    Other = { "assets/logo.png" },
  }, names(g))
end)

test("first matching group wins and empty groups are dropped", function()
  local groups = {
    { name = "API", patterns = { [[^api/]] } },
    { name = "Lua", patterns = { [[\.lua$]] } },
    { name = "Empty", patterns = { [[^nothing/]] } },
  }
  local g = group.assign(files("api/a.lua", "b.lua", "c.txt"), groups, "Rest")
  eq({ "API", "Lua", "Rest" }, vim.tbl_map(function(x) return x.name end, g))
  eq({ API = { "api/a.lua" }, Lua = { "b.lua" }, Rest = { "c.txt" } }, names(g))
end)

test("invalid patterns are skipped, not fatal", function()
  local notify = vim.notify
  vim.notify = function() end
  local g = group.assign(files("a.lua"), { { name = "Bad", patterns = { [[(]] } } }, "Other")
  vim.notify = notify
  eq({ Other = { "a.lua" } }, names(g))
end)

test("sort_by_order is stable for unlisted files", function()
  local g = { { name = "S", files = files("a", "b", "c", "d") } }
  group.sort_by_order(g, { "c", "a" })
  eq({ S = { "c", "a", "b", "d" } }, names(g))
end)

test("sort_by_order puts the group holding the first file first", function()
  local g = {
    { name = "Tests", files = files("t") },
    { name = "Docs", files = files("d") },
    { name = "Source", files = files("s") },
    { name = "Other", files = files("o") },
  }
  group.sort_by_order(g, { "s", "t" })
  eq({ "Source", "Tests", "Docs", "Other" }, vim.tbl_map(function(x) return x.name end, g))
end)

test("setup replaces groups instead of merging by index", function()
  config.setup({ groups = { { name = "Only", patterns = { "." } } } })
  eq(1, #config.options.groups)
  eq("]g", config.options.keymaps.next_group)
  config.setup({})
  eq(#config.defaults.groups, #config.options.groups)
end)
