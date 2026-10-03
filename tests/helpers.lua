local M = {}

function M.tmpdir()
  local dir = vim.fn.tempname()
  vim.fn.mkdir(dir, "p")
  return dir
end

function M.git(dir, ...)
  local res = vim.system({ "git", "-C", dir, "-c", "user.name=t", "-c", "user.email=t@t", ... }):wait()
  assert(res.code == 0, res.stderr)
  return res.stdout
end

function M.write(dir, path, content)
  vim.fn.mkdir(vim.fs.dirname(dir .. "/" .. path), "p")
  local fd = assert(io.open(dir .. "/" .. path, "w"))
  fd:write(content)
  fd:close()
end

--- A repo with one commit containing `files`.
function M.repo(files)
  local dir = M.tmpdir()
  M.git(dir, "init", "-q")
  for p, c in pairs(files) do
    M.write(dir, p, c)
  end
  M.git(dir, "add", "-A")
  M.git(dir, "commit", "-qm", "init")
  return dir
end

return M
