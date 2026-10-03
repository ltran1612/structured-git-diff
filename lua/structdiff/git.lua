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
  local base = run(root, { "rev-parse", "--verify", "-q", "HEAD" }) and "HEAD" or EMPTY_TREE
  return { root = root, exclude = exclude, base = base }
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

--- Changed files in the working tree relative to HEAD (staged + unstaged +
--- untracked). Status is one of M A D R C U, or ? for untracked.
function M.changed_files(repo)
  local out = assert(run(repo.root, { "diff", repo.base, "--name-status", "-z", "-M", "--no-ext-diff" }))
  local files = M.parse_name_status(out)
  local untracked = assert(run(repo.root, { "ls-files", "--others", "--exclude-standard", "-z" }))
  for _, p in ipairs(vim.split(untracked, "\0", { plain = true, trimempty = true })) do
    table.insert(files, { status = "?", path = p })
  end
  table.sort(files, function(a, b)
    return a.path < b.path
  end)
  return files
end

--- Hash of the full change set. Changes whenever any diff content or any
--- untracked file's content changes.
function M.fingerprint(repo, files)
  local diff = run(repo.root, { "diff", repo.base, "--no-color", "--no-ext-diff", "--binary" }) or ""
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

--- File content at the base revision, or nil when it doesn't exist there.
function M.base_content(repo, path)
  return run(repo.root, { "show", repo.base .. ":" .. path })
end

return M
