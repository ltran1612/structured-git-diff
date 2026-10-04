#!/bin/sh
# Build the Rust plugin and install it where Neovim's `require` finds it, and
# install the `structdiff` CLI (used by the diff-narrative skill) on PATH.
# Set STRUCTDIFF_BIN_DIR to choose where the CLI goes (default ~/.local/bin).
set -eu
cd "$(dirname "$0")"
cargo build --release -p structdiff -p structdiff-cli
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

bin_dir="${STRUCTDIFF_BIN_DIR:-$HOME/.local/bin}"
mkdir -p "$bin_dir"
cp target/release/structdiff "$bin_dir/.structdiff.tmp"
mv "$bin_dir/.structdiff.tmp" "$bin_dir/structdiff"
echo "installed $bin_dir/structdiff"
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "note: $bin_dir is not on PATH; the diff-narrative skill needs \`structdiff\` there" ;;
esac
