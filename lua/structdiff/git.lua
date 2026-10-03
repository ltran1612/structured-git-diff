local M = {}

-- `git hash-object -t tree /dev/null`: the base when the repo has no commits.
local EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"

local function run(cwd, args, stdin)
  local cmd = { "git", "-C", cwd }
  vim.list_extend(cmd, args)
  local res = vim.system(cmd, { stdin = stdin }):wait()
  if res.code ~= 0 then
    return nil, vim.trim(res.stderr or "")
  end
  return res.stdout or ""
end
M.run = run

--- Repo info for a directory, or nil + error when it is not in a work tree.
function M.repo(dir)
  local out, err = run(dir, { "rev-parse", "--show-toplevel" })
  if not out then
    return nil, err
  end
  local root = vim.trim(out)
  local exclude = vim.trim(assert(run(root, { "rev-parse", "--path-format=absolute", "--git-path", "info/exclude" })))
  return { root = root, exclude = exclude }
end

local function commit(repo, rev)
  local out = run(repo.root, { "rev-parse", "--verify", "-q", rev .. "^{commit}" })
  return out and vim.trim(out)
end

--- Resolve what to compare. `spec` is one of:
---   ""        HEAD vs working tree (empty tree if there are no commits)
---   "A"       A vs working tree
---   "A..B"    A vs B
---   "A...B"   merge-base(A, B) vs B, i.e. what B adds on top of A
--- An empty side of ".." / "..." means HEAD. Returns a range with `base`
--- (left rev), `target` (right rev, or nil for the working tree) and labels,
--- or nil + error.
function M.resolve_range(repo, spec)
  spec = vim.trim(spec or "")
  if spec == "" then
    local head = commit(repo, "HEAD")
    return { spec = "", base = head and "HEAD" or EMPTY_TREE, left_label = head and "HEAD" or "empty", right_label = "worktree" }
  end
  local a, dots, b = spec:match("^(.-)(%.%.%.?)(.*)$")
  if not dots then
    local sha = commit(repo, spec)
    if not sha then
      return nil, "unknown revision: " .. spec
    end
    return { spec = spec, base = sha, left_label = spec, right_label = "worktree" }
  end
  a, b = a ~= "" and a or "HEAD", b ~= "" and b or "HEAD"
  local asha, bsha = commit(repo, a), commit(repo, b)
  if not asha or not bsha then
    return nil, "unknown revision: " .. (asha and b or a)
  end
  local base, left_label = asha, a
  if dots == "..." then
    local mb = run(repo.root, { "merge-base", asha, bsha })
    if not mb then
      return nil, ("no merge base between %s and %s"):format(a, b)
    end
    base, left_label = vim.trim(mb), "merge-base(" .. a .. ")"
  end
  return { spec = spec, base = base, target = bsha, left_label = left_label, right_label = b }
end

local function diff_args(range, extra)
  local args = { "diff", range.base }
  if range.target then
    table.insert(args, range.target)
  end
  return vim.list_extend(args, extra)
end

--- Parse `git diff --name-status -z` output.
function M.parse_name_status(out)
  local toks = vim.split(out, "\0", { plain = true, trimempty = true })
  local files, i = {}, 1
  while i <= #toks do
    local code = toks[i]
    local letter = code:sub(1, 1)
    if letter == "R" or letter == "C" then
      table.insert(files, { status = letter, old_path = toks[i + 1], path = toks[i + 2] })
      i = i + 3
    else
      if letter == "T" then
        letter = "M"
      end
      table.insert(files, { status = letter, path = toks[i + 1] })
      i = i + 2
    end
  end
  return files
end

--- Changed files in `range`. Against the working tree this includes staged,
--- unstaged and untracked files. Status is one of M A D R C U, or ? for
--- untracked.
function M.changed_files(repo, range)
  local out = assert(run(repo.root, diff_args(range, { "--name-status", "-z", "-M", "--no-ext-diff" })))
  local files = M.parse_name_status(out)
  if not range.target then
    local untracked = assert(run(repo.root, { "ls-files", "--others", "--exclude-standard", "-z" }))
    for _, p in ipairs(vim.split(untracked, "\0", { plain = true, trimempty = true })) do
      table.insert(files, { status = "?", path = p })
    end
  end
  table.sort(files, function(a, b)
    return a.path < b.path
  end)
  return files
end

--- Hash of the full change set. Changes whenever any diff content or any
--- untracked file's content changes.
function M.fingerprint(repo, range, files)
  local diff = run(repo.root, diff_args(range, { "--no-color", "--no-ext-diff", "--binary" })) or ""
  local untracked = {}
  for _, f in ipairs(files) do
    if f.status == "?" then
      table.insert(untracked, f.path)
    end
  end
  local parts = { diff }
  if #untracked > 0 then
    local hashes = run(repo.root, { "hash-object", "--stdin-paths" }, table.concat(untracked, "\n") .. "\n") or ""
    local hs = vim.split(hashes, "\n", { trimempty = true })
    for i, p in ipairs(untracked) do
      table.insert(parts, p .. " " .. (hs[i] or ""))
    end
  end
  return vim.fn.sha256(table.concat(parts, "\0"))
end

--- File content at `rev`, or nil when it doesn't exist there.
function M.content(repo, rev, path)
  return run(repo.root, { "show", rev .. ":" .. path })
end

--- Ref names for command-line completion.
function M.refs(repo)
  local out = run(repo.root, { "for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes", "refs/tags" })
  local refs = vim.split(out or "", "\n", { trimempty = true })
  table.insert(refs, 1, "HEAD")
  return refs
end

return M
