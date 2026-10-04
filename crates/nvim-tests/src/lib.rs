//! Integration tests that run inside a real Neovim (via `#[nvim_oxi::test]`).
//! The pure logic is tested in crates/core.
//!
//! No `#[cfg(test)]` here: the harness builds this crate as a plain cdylib
//! and loads each test's entry point from it.

#![deny(clippy::disallowed_methods)]

mod helpers;
mod view;
