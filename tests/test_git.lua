local git = require("structdiff.git")
local h = require("helpers")

test("parse_name_status handles renames and type changes", function()
  local out = "M\0a.lua\0R087\0old.lua\0new.lua\0D\0gone.txt\0T\0link\0"
  eq({
    { status = "M", path = "a.lua" },
    { status = "R", old_path = "old.lua", path = "new.lua" },
    { status = "D", path = "gone.txt" },
    { status = "M", path = "link" },
  }, git.parse_name_status(out))
end)

test("changed_files covers staged, unstaged, untracked, deleted, renamed", function()
  local dir = h.repo({ ["a.lua"] = "a\n", ["b.lua"] = "b\n", ["old name.lua"] = "x\ny\nz\n" })
  h.write(dir, "a.lua", "a2\n") -- unstaged
  h.write(dir, "staged.lua", "s\n")
  h.git(dir, "add", "staged.lua")
  os.remove(dir .. "/b.lua")
  h.git(dir, "mv", "old name.lua", "new name.lua")
  h.write(dir, "untracked.md", "u\n")
  local repo = assert(git.repo(dir))
  local range = assert(git.resolve_range(repo, ""))
  eq("HEAD", range.base)
  eq(nil, range.target)
  eq({
    { status = "M", path = "a.lua" },
    { status = "D", path = "b.lua" },
    { status = "R", old_path = "old name.lua", path = "new name.lua" },
    { status = "A", path = "staged.lua" },
    { status = "?", path = "untracked.md" },
  }, git.changed_files(repo, range))
end)

test("fingerprint changes with tracked and untracked edits", function()
  local dir = h.repo({ ["a.lua"] = "a\n" })
  local repo = assert(git.repo(dir))
  local range = assert(git.resolve_range(repo, ""))
  local function fp()
    return git.fingerprint(repo, range, git.changed_files(repo, range))
  end
  h.write(dir, "a.lua", "a2\n")
  h.write(dir, "new.txt", "1\n")
  local fp1 = fp()
  eq(fp1, fp())
  h.write(dir, "new.txt", "2\n")
  local fp2 = fp()
  assert(fp1 ~= fp2, "untracked edit should change fingerprint")
  h.write(dir, "a.lua", "a3\n")
  assert(fp2 ~= fp(), "tracked edit should change fingerprint")
end)

test("repo without commits diffs against the empty tree", function()
  local dir = h.tmpdir()
  h.git(dir, "init", "-q")
  h.write(dir, "x.lua", "x\n")
  h.git(dir, "add", "x.lua")
  local repo = assert(git.repo(dir))
  local range = assert(git.resolve_range(repo, ""))
  assert(range.base ~= "HEAD")
  eq("empty", range.left_label)
  eq({ { status = "A", path = "x.lua" } }, git.changed_files(repo, range))
end)

-- main: init -> m2 (touches shared.lua). feature branches off init and adds
-- f1 (edits a.lua, adds new.lua, deletes gone.md). Also an uncommitted edit.
local function branch_repo()
  local dir = h.repo({ ["a.lua"] = "a\n", ["shared.lua"] = "s\n", ["gone.md"] = "g\n" })
  h.git(dir, "branch", "-M", "main")
  h.git(dir, "checkout", "-qb", "feature")
  h.write(dir, "a.lua", "a feature\n")
  h.write(dir, "new.lua", "n\n")
  os.remove(dir .. "/gone.md")
  h.git(dir, "add", "-A")
  h.git(dir, "commit", "-qm", "f1")
  h.git(dir, "checkout", "-q", "main")
  h.write(dir, "shared.lua", "s main\n")
  h.git(dir, "commit", "-qam", "m2")
  h.git(dir, "checkout", "-q", "feature")
  h.write(dir, "a.lua", "a feature + uncommitted\n")
  h.write(dir, "scratch.txt", "x\n")
  return dir
end
_G.branch_repo = branch_repo

local function paths(files)
  return vim.tbl_map(function(f) return f.status .. " " .. f.path end, files)
end

test("A...B uses the merge base and ignores uncommitted changes", function()
  local dir = branch_repo()
  local repo = assert(git.repo(dir))
  local r = assert(git.resolve_range(repo, "main...HEAD"))
  eq(vim.trim(h.git(dir, "merge-base", "main", "HEAD")), r.base)
  eq(vim.trim(h.git(dir, "rev-parse", "HEAD")), r.target)
  eq("merge-base(main)", r.left_label)
  eq("HEAD", r.right_label)
  eq({ "M a.lua", "D gone.md", "A new.lua" }, paths(git.changed_files(repo, r)))
  eq("a feature\n", git.content(repo, r.target, "a.lua"))
  -- an empty right side means HEAD
  eq(r.base, assert(git.resolve_range(repo, "main...")).base)
end)

test("A..B compares the two tips directly", function()
  local dir = branch_repo()
  local repo = assert(git.repo(dir))
  local r = assert(git.resolve_range(repo, "main..feature"))
  eq(vim.trim(h.git(dir, "rev-parse", "main")), r.base)
  -- main's own commit shows up as a change too, unlike with "..."
  eq({ "M a.lua", "D gone.md", "A new.lua", "M shared.lua" }, paths(git.changed_files(repo, r)))
end)

test("a single rev compares it to the working tree", function()
  local dir = branch_repo()
  local repo = assert(git.repo(dir))
  local r = assert(git.resolve_range(repo, "main"))
  eq(nil, r.target)
  eq("worktree", r.right_label)
  eq({ "M a.lua", "D gone.md", "A new.lua", "? scratch.txt", "M shared.lua" }, paths(git.changed_files(repo, r)))
end)

test("unknown revisions are reported", function()
  local repo = assert(git.repo(branch_repo()))
  eq({ nil, "unknown revision: nope" }, { git.resolve_range(repo, "nope") })
  eq({ nil, "unknown revision: nope" }, { git.resolve_range(repo, "main...nope") })
end)

test("refs lists HEAD and branches", function()
  local refs = git.refs(assert(git.repo(branch_repo())))
  eq("HEAD", refs[1])
  assert(vim.tbl_contains(refs, "main") and vim.tbl_contains(refs, "feature"), vim.inspect(refs))
end)
