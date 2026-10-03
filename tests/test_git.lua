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
  eq("HEAD", repo.base)
  eq({
    { status = "M", path = "a.lua" },
    { status = "D", path = "b.lua" },
    { status = "R", old_path = "old name.lua", path = "new name.lua" },
    { status = "A", path = "staged.lua" },
    { status = "?", path = "untracked.md" },
  }, git.changed_files(repo))
end)

test("fingerprint changes with tracked and untracked edits", function()
  local dir = h.repo({ ["a.lua"] = "a\n" })
  local repo = assert(git.repo(dir))
  h.write(dir, "a.lua", "a2\n")
  h.write(dir, "new.txt", "1\n")
  local fp1 = git.fingerprint(repo, git.changed_files(repo))
  eq(fp1, git.fingerprint(repo, git.changed_files(repo)))
  h.write(dir, "new.txt", "2\n")
  local fp2 = git.fingerprint(repo, git.changed_files(repo))
  assert(fp1 ~= fp2, "untracked edit should change fingerprint")
  h.write(dir, "a.lua", "a3\n")
  assert(fp2 ~= git.fingerprint(repo, git.changed_files(repo)), "tracked edit should change fingerprint")
end)

test("repo without commits diffs against the empty tree", function()
  local dir = h.tmpdir()
  h.git(dir, "init", "-q")
  h.write(dir, "x.lua", "x\n")
  h.git(dir, "add", "x.lua")
  local repo = assert(git.repo(dir))
  assert(repo.base ~= "HEAD")
  eq({ { status = "A", path = "x.lua" } }, git.changed_files(repo))
end)
