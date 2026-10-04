//! Loading the plugin: the Neovim version guard, user commands with range
//! completion, and the Lua module table.

use std::convert::Infallible;
use std::path::PathBuf;

use nvim_oxi::api::{self, opts::*, types::*};
use nvim_oxi::{Dictionary, Function, Object};
use structdiff_core::{Repo, git};

use crate::config::Config;
use crate::{actions, hl, state, ui};

/// Neovim versions whose C API this build was checked against, binding by
/// binding (see the compatibility notes in lib.rs). nvim-oxi's bindings broke
/// *within* the 0.12 series, so this is an exact allowlist, not a
/// minor-version range. Add a version only after re-checking every binding
/// the plugin uses against that version's sources.
pub const TESTED_NEOVIM: &[(u32, u32, u32)] = &[(0, 12, 5)];

fn is_neovim(major: u32, minor: u32, patch: u32) -> bool {
    let has = |feature: String| api::call_function::<_, i64>("has", (feature,)).unwrap_or(0) == 1;
    has(format!("nvim-{major}.{minor}.{patch}")) && !has(format!("nvim-{major}.{minor}.{}", patch + 1))
}

fn neovim_version() -> String {
    api::call_function::<_, String>("luaeval", ("tostring(vim.version())",)).unwrap_or_else(|_| "unknown".into())
}

/// Ok when this Neovim is on the allowlist, or the user explicitly opted out
/// of the check with `vim.g.structdiff_allow_untested_nvim = true`.
pub fn check_neovim(allowlist: &[(u32, u32, u32)]) -> Result<(), String> {
    if allowlist.iter().any(|&(a, b, c)| is_neovim(a, b, c)) {
        return Ok(());
    }
    let tested: Vec<String> = allowlist.iter().map(|(a, b, c)| format!("{a}.{b}.{c}")).collect();
    let msg = format!(
        "built and checked for Neovim {}, but this is {}. Untested versions can crash Neovim.",
        tested.join(", "),
        neovim_version()
    );
    let opted_out = api::eval::<i64>("!!get(g:, 'structdiff_allow_untested_nvim', 0)").unwrap_or(0) == 1;
    if opted_out {
        ui::notify(&format!("{msg} Loading anyway (structdiff_allow_untested_nvim is set)."), ui::WARN);
        return Ok(());
    }
    Err(format!("{msg} Not loading. Set vim.g.structdiff_allow_untested_nvim = true to override at your own risk."))
}

/// `setup(opts)` from Lua: replace the config, or report bad options.
pub fn setup(opts: Object) {
    match Config::from_object(opts) {
        Ok(cfg) => state::set_config(cfg),
        Err(e) => ui::notify(&e, ui::ERROR),
    }
}

/// Ref names, completing the part after ".." / "..." (main...fe<Tab>).
fn complete_range(arglead: String) -> Vec<String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Ok(repo) = Repo::discover(&cwd) else { return Vec::new() };
    let (prefix, rest) = git::split_completion(&arglead);
    repo.refs().into_iter().filter(|r| r.starts_with(rest)).map(|r| format!("{prefix}{r}")).collect()
}

fn command(name: &str, desc: &str, f: fn()) -> Result<(), api::Error> {
    api::create_user_command(
        name,
        move |_: CommandArgs| {
            f();
            Ok::<_, Infallible>(())
        },
        &CreateCommandOpts::builder().desc(desc).build(),
    )
}

fn range_command(name: &str, desc: &str, f: fn(Option<String>)) -> Result<(), api::Error> {
    let complete = Function::from_fn(|(lead, _line, _pos): (String, String, usize)| {
        Ok::<_, Infallible>(complete_range(lead))
    });
    api::create_user_command(
        name,
        move |args: CommandArgs| {
            f(args.args);
            Ok::<_, Infallible>(())
        },
        &CreateCommandOpts::builder()
            .desc(desc)
            .nargs(CommandNArgs::ZeroOrOne)
            .complete(CommandComplete::CustomList(complete))
            .build(),
    )
}

/// A Lua-callable wrapper around `f`. It takes its argument as a raw
/// `Object` and converts it here, so a missing or mistyped argument becomes
/// an error message. Letting nvim-oxi pop a typed argument instead would
/// raise a Lua error from Rust, which aborts Neovim.
fn lua_fn<A: nvim_oxi::conversion::FromObject + 'static>(name: &'static str, f: fn(A)) -> Object {
    Object::from(Function::<Object, ()>::from_fn(move |arg: Object| {
        match A::from_object(arg) {
            Ok(a) => f(a),
            Err(e) => ui::notify(&format!("{name}: bad argument: {e}"), ui::ERROR),
        }
        Ok::<_, Infallible>(())
    }))
}

/// Navigation step from Lua: a missing argument means 1.
fn step(delta: Option<i64>) -> i64 {
    delta.unwrap_or(1)
}

/// Register commands and highlights; return the module table.
pub fn init() -> nvim_oxi::Result<Dictionary> {
    if let Err(msg) = check_neovim(TESTED_NEOVIM) {
        // Talking to a Neovim with a different API layout could corrupt
        // memory, so do nothing at all.
        ui::notify(&msg, ui::ERROR);
        return Ok(Dictionary::new());
    }
    range_command("StructDiff", "Open grouped diff: [rev | A..B | A...B], default HEAD vs working tree", actions::open)?;
    command("StructDiffClose", "Close the StructDiff view", actions::close)?;
    command("StructDiffRefresh", "Re-scan changes and reload the narrative", actions::refresh)?;
    command("StructDiffNarrative", "Toggle the narrative split", actions::toggle_narrative)?;
    hl::apply();

    Ok(Dictionary::from_iter([
        ("setup", lua_fn("setup", setup)),
        ("open", lua_fn("open", actions::open)),
        ("close", lua_fn("close", |_: Object| actions::close())),
        ("refresh", lua_fn("refresh", |_: Object| actions::refresh())),
        ("reload_narrative", lua_fn("reload_narrative", |_: Object| actions::reload_narrative())),
        ("toggle_narrative", lua_fn("toggle_narrative", |_: Object| actions::toggle_narrative())),
        ("toggle_reasons", lua_fn("toggle_reasons", |_: Object| actions::toggle_reasons())),
        ("goto_file", lua_fn("goto_file", |d: Option<i64>| actions::goto_file(step(d)))),
        ("goto_group", lua_fn("goto_group", |d: Option<i64>| actions::goto_group(step(d)))),
        // True while git work is running in the background (for scripts).
        ("busy", Object::from(Function::<(), bool>::from_fn(|(): ()| Ok::<_, Infallible>(state::busy())))),
    ]))
}
