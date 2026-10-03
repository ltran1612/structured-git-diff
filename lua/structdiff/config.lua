local M = {}

-- Patterns are Vim regexes in very-magic mode (\v is prepended), matched
-- against the repo-relative path. Groups are tried in order; first match wins.
M.defaults = {
  groups = {
    {
      name = "Tests",
      patterns = {
        [[(^|/)(tests?|spec|__tests__)/]],
        [[_test\.\w+$]],
        [[\.(test|spec)\.\w+$]],
        [[(^|/)test_[^/]+\.py$]],
      },
    },
    {
      name = "CI",
      patterns = { [[^\.github/]], [[^\.gitlab-ci]], [[^\.circleci/]] },
    },
    {
      name = "Config/Build",
      patterns = {
        [[(^|/)(Makefile|CMakeLists\.txt|Dockerfile|Justfile|flake\.nix)$]],
        [[(^|/)requirements[^/]*\.txt$]],
        [[\.(lock|json|ya?ml|toml|ini|cfg|conf)$]],
        [[(^|/)\.[^/]+rc$]],
      },
    },
    {
      name = "Docs",
      patterns = {
        [[(^|/)docs?/]],
        [[\.(md|rst|txt|adoc)$]],
        [[(^|/)(README|CHANGELOG|LICENSE)[^/]*$]],
      },
    },
    {
      name = "Source",
      patterns = {
        [[\.(lua|py|go|rs|js|jsx|mjs|ts|tsx|c|h|cc|cpp|hpp|java|kt|rb|sh|bash|vim|zig|cs|swift|php|ex|exs|hs|ml|scala|sql|html|css|scss|vue|svelte)$]],
      },
    },
  },
  other_group = "Other",
  sidebar_width = 40,
  narrative_height = 15,
  show_reasons = true,
  -- Run by :StructDiffGenerate from the repo root. The skill writes
  -- .structdiff/narrative.json, which the viewer then reloads.
  generate_cmd = {
    "claude",
    "-p",
    "/diff-narrative",
    "--permission-mode",
    "acceptEdits",
    "--allowedTools",
    "Bash(git:*)",
    "Read",
    "Write",
  },
  keymaps = {
    next_group = "]g",
    prev_group = "[g",
    next_file = "]f",
    prev_file = "[f",
    select = "<CR>",
    toggle_fold = { "za", "<Tab>" },
    toggle_narrative = "gn",
    toggle_reasons = "gr",
    refresh = "R",
    close = "q",
  },
}

M.options = vim.deepcopy(M.defaults)

function M.setup(opts)
  opts = opts or {}
  M.options = vim.tbl_deep_extend("force", vim.deepcopy(M.defaults), opts)
  -- Lists must replace, not merge by index.
  if opts.groups then
    M.options.groups = vim.deepcopy(opts.groups)
  end
  if opts.generate_cmd then
    M.options.generate_cmd = vim.deepcopy(opts.generate_cmd)
  end
end

--- Groups for a repo: <root>/.structdiff.json {"groups": [...]} replaces the
--- configured groups when present.
function M.groups_for(root)
  local path = root .. "/.structdiff.json"
  local fd = io.open(path, "r")
  if not fd then
    return M.options.groups
  end
  local text = fd:read("*a")
  fd:close()
  local ok, data = pcall(vim.json.decode, text)
  if ok and type(data) == "table" and type(data.groups) == "table" then
    return data.groups
  end
  vim.notify("structdiff: ignoring invalid " .. path, vim.log.levels.WARN)
  return M.options.groups
end

return M
