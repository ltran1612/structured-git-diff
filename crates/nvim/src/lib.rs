//! structdiff.nvim: a grouped diff viewer with an AI-written change narrative.
//!
//! Loaded by Neovim as `lua/structdiff.so`. `require("structdiff")` runs
//! [`init`], which registers the commands and returns the module table.
//!
//! # Layout
//!
//! Dependencies only point down this list:
//!
//! - `register`: version guard, commands, the Lua module table.
//! - `actions`, `generate`, `watch`: the controller. What commands and keys
//!   do, background git work, keymap wiring.
//! - `keys`: which key runs which action.
//! - `view`: windows, buffers and rendering. Draws only; owns no config.
//! - `state`, `config`, `hl`, `ui`, `bg`: leaves.
//!
//! # Compatibility with Neovim 0.12.5
//!
//! nvim-oxi binds Neovim's C API directly, and its `neovim-0-12` feature
//! predates some 0.12.x changes. Checked against the v0.12.5 sources, these
//! bindings are wrong and must not be used:
//!
//! - `api::create_autocmd`: 0.12.5 added a `buf` key to `Dict(create_autocmd)`,
//!   so `CreateAutocmdOpts` is misaligned (the callback is never seen).
//!   Replaced by the polling timer in `watch`.
//! - `api::set_hl`: `Dict(highlight)` was reordered and extended. Replaced by
//!   `:highlight default link` in `hl`.
//! - `api::list_tabpages`: `nvim_list_tabpages` now takes an `Arena*`; calling
//!   it corrupts the heap. Replaced by `tabpagenr('$')` ([`ui::tab_count`]).
//!
//! [`init`] refuses to load on any Neovim not in [`TESTED_NEOVIM`], and
//! `clippy.toml` forbids calling the three bindings above.

// Turn clippy.toml's ban on ABI-broken nvim-oxi bindings into a hard error.
#![deny(clippy::disallowed_methods)]

mod actions;
mod bg;
pub mod config;
mod generate;
mod hl;
mod keys;
mod register;
mod state;
pub mod ui;
pub mod view;
mod watch;

pub use actions::{
    close, fold_at_cursor, goto_file, goto_group, open, refresh, reload_narrative, select_at_cursor,
    toggle_narrative, toggle_reasons,
};
pub use generate::generate;
pub use register::{TESTED_NEOVIM, check_neovim, init, setup};
pub use state::{busy, config, with_view};

#[nvim_oxi::plugin]
fn structdiff() -> nvim_oxi::Result<nvim_oxi::Dictionary> {
    init()
}
