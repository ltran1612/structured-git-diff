-- nvim --headless -l tests/run.lua
local root = vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2))))
vim.opt.rtp:prepend(root)
package.path = root .. "/tests/?.lua;" .. package.path

local failures, count = 0, 0
_G.test = function(name, fn)
  count = count + 1
  local ok, err = xpcall(fn, debug.traceback)
  if ok then
    io.write("ok   " .. name .. "\n")
  else
    failures = failures + 1
    io.write("FAIL " .. name .. "\n" .. err .. "\n")
  end
end
_G.eq = function(want, got)
  if not vim.deep_equal(want, got) then
    error(("expected %s\n     got %s"):format(vim.inspect(want), vim.inspect(got)), 2)
  end
end

for _, f in ipairs(arg) do
  dofile(f)
end
if #arg == 0 then
  for name in vim.fs.dir(root .. "/tests") do
    if name:match("^test_.*%.lua$") then
      dofile(root .. "/tests/" .. name)
    end
  end
end
io.write(("%d tests, %d failed\n"):format(count, failures))
os.exit(failures == 0 and 0 or 1)
