#!/bin/sh
# Everything CI would run: lints (including the ban on ABI-broken nvim-oxi
# bindings in clippy.toml) and both test suites.
set -eu
cd "$(dirname "$0")"
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p structdiff-core -p structdiff-cli
cargo test -p structdiff-nvim-tests
