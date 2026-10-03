#!/bin/sh
# Build the Rust plugin and install it where Neovim's `require` finds it.
set -eu
cd "$(dirname "$0")"
cargo build --release -p structdiff
case "$(uname -s)" in
  Darwin) lib=target/release/libstructdiff.dylib ;;
  *) lib=target/release/libstructdiff.so ;;
esac
mkdir -p lua
# Copy to a temp name and rename, so a running Neovim that has the old
# library mapped keeps working until it restarts.
cp "$lib" lua/.structdiff.so.tmp
mv lua/.structdiff.so.tmp lua/structdiff.so
echo "installed lua/structdiff.so"
