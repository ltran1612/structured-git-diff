-- The plugin is Rust (lua/structdiff.so, built by ./build.sh). Neovim can
-- only load native modules through `require`, so this one line is the whole
-- Lua side: loading the module registers :StructDiff and friends.
require("structdiff")
