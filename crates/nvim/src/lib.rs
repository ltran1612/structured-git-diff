//! structdiff.nvim: a grouped diff viewer with an AI-written change narrative.
//!
//! Loaded by Neovim as `lua/structdiff.so`. `require("structdiff")` runs
//! [`init`], which registers the commands and returns the module table.
//!
//! # Compatibility with Neovim 0.12.5
//!
//! nvim-oxi binds Neovim's C API directly, and its `neovim-0-12` feature
//! predates some 0.12.x changes. Checked against the v0.12.5 sources, these
//! bindings are wrong and must not be used:
//!
//! - `api::create_autocmd`: 0.12.5 added a `buf` key to `Dict(create_autocmd)`,
//!   so `CreateAutocmdOpts` is misaligned (the callback is never seen).
//!   Replaced by the polling [`watch`] timer.
//! - `api::set_hl`: `Dict(highlight)` was reordered and extended. Replaced by
//!   `:highlight default link` in [`set_highlights`].
//! - `api::list_tabpages`: `nvim_list_tabpages` now takes an `Arena*`; calling
//!   it corrupts the heap. Replaced by `tabpagenr('$')` ([`ui::tab_count`]).
//!
//! [`init`] refuses to load on any Neovim other than 0.12.

mod bg;
pub mod config;
mod keys;
pub mod ui;
pub mod view;

use std::cell::{Cell, RefCell};
use std::convert::Infallible;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use nvim_oxi::api::{self, opts::*, types::*};
use nvim_oxi::libuv::TimerHandle;
use nvim_oxi::{Dictionary, Function, Object};
use structdiff_core::{Model, Repo, git, narrative};

use config::Config;
use view::{Item, View};

thread_local! {
    static VIEW: RefCell<Option<View>> = const { RefCell::new(None) };
    static CONFIG: RefCell<Option<Config>> = const { RefCell::new(None) };
    /// A view is being loaded on a background thread.
    static LOADING: Cell<bool> = const { Cell::new(false) };
}

/// Something to run on the main loop once background work has landed.
type Then = Option<Box<dyn FnOnce()>>;

/// True while a view is loading or rescanning in the background.
pub fn busy() -> bool {
    LOADING.with(Cell::get) || with_view(|v| v.pending_scans > 0).unwrap_or(false)
}

pub fn config() -> Config {
    CONFIG.with(|c| c.borrow().clone().unwrap_or_default())
}

/// Run `f` on the open view. None when there is no view, or when the view is
/// already borrowed (re-entrant call from an autocmd).
pub fn with_view<R>(f: impl FnOnce(&mut View) -> R) -> Option<R> {
    VIEW.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

fn report(res: Result<(), api::Error>) {
    if let Err(e) = res {
        ui::notify(&e.to_string(), ui::ERROR);
    }
}

fn notify_all(warnings: Vec<String>) {
    for w in warnings {
        ui::notify(&w, ui::WARN);
    }
}

// Module functions -----------------------------------------------------------

pub fn setup(opts: Object) {
    match Config::from_object(opts) {
        Ok(cfg) => CONFIG.with(|c| *c.borrow_mut() = Some(cfg)),
        Err(e) => ui::notify(&e, ui::ERROR),
    }
}

/// Open the view. `spec` picks what to compare (see `Repo::resolve_range`).
/// The same spec again refreshes; a different spec replaces the view. Git
/// runs on a background thread; the view appears when it's done.
pub fn open(spec: Option<String>) {
    open_then(spec.unwrap_or_default(), None);
}

fn open_then(spec: String, then: Then) {
    let spec = spec.trim().to_owned();
    let existing = with_view(|v| (v.model.spec.clone(), v.tab.clone()));
    if let Some((open_spec, tab)) = existing {
        if open_spec == spec && tab.is_valid() {
            report(api::set_current_tabpage(&tab));
            return refresh_then(then);
        }
        close();
    }
    if LOADING.with(Cell::get) {
        return ui::notify("already loading", ui::INFO);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo = match Repo::discover(&cwd) {
        Ok(r) => r,
        Err(e) => return ui::notify(&format!("not a git repository: {e}"), ui::ERROR),
    };
    let cfg = config();
    let (groups, other) = (cfg.groups.clone(), cfg.other_group.clone());
    LOADING.with(|l| l.set(true));
    let started = bg::spawn(
        move || Model::load(repo, &spec, &groups, &other),
        move |res| {
            LOADING.with(|l| l.set(false));
            if finish_open(res, cfg)
                && let Some(then) = then
            {
                then();
            }
        },
    );
    if let Err(e) = started {
        LOADING.with(|l| l.set(false));
        ui::notify(&e, ui::ERROR);
    }
}

/// Create the view for a freshly loaded model. False when there is nothing
/// to show.
fn finish_open(res: Result<(Model, Vec<String>), String>, cfg: Config) -> bool {
    let (model, warnings) = match res {
        Ok(m) => m,
        Err(e) => {
            ui::notify(&e, ui::ERROR);
            return false;
        }
    };
    notify_all(warnings);
    if model.file_count() == 0 {
        ui::notify(&format!("no changes in {}", model.range.describe()), ui::INFO);
        return false;
    }
    let has_narrative = model.narrative.is_some();
    let watch_path = narrative::path(&model.repo.root, &model.spec);
    let mut v = match View::new(model, cfg.clone()) {
        Ok(v) => v,
        Err(e) => {
            ui::notify(&e.to_string(), ui::ERROR);
            return false;
        }
    };
    keys::map_panel(&mut v.panel_buf, &cfg);
    v.watcher = watch(watch_path);
    VIEW.with(|c| *c.borrow_mut() = Some(v));
    with_view(|v| {
        report(v.show(0));
        if has_narrative {
            report(v.open_narrative());
        }
        report(api::set_current_win(&v.sidebar));
        v.focus_current();
    });
    true
}

/// Rescan git in the background and redraw. Rescans may overlap; only the
/// latest one's result is applied.
pub fn refresh() {
    refresh_then(None);
}

fn refresh_then(then: Then) {
    let Some((mut model, scan)) = with_view(|v| {
        v.scan_gen += 1;
        v.pending_scans += 1;
        (v.model.clone(), v.scan_gen)
    }) else {
        return;
    };
    let started = bg::spawn(
        move || {
            let res = model.rescan();
            (model, res)
        },
        move |(model, res)| {
            let latest = with_view(|v| {
                v.pending_scans -= 1;
                v.scan_gen == scan
            });
            if latest != Some(true) {
                return;
            }
            match res {
                Err(e) => return ui::notify(&e, ui::ERROR),
                Ok(w) => notify_all(w),
            }
            if model.file_count() == 0 {
                ui::notify("no changes left", ui::INFO);
                return close();
            }
            with_view(|v| {
                v.model = model;
                let idx = v.current_index().unwrap_or(0);
                report(v.show(idx));
            });
            if let Some(then) = then {
                then();
            }
        },
    );
    if let Err(e) = started {
        with_view(|v| v.pending_scans -= 1);
        ui::notify(&e, ui::ERROR);
    }
}

pub fn reload_narrative() {
    with_view(|v| {
        notify_all(v.model.reload_narrative());
        v.redraw();
    });
}

pub fn close() {
    let Some(v) = VIEW.with(|c| c.try_borrow_mut().ok().and_then(|mut g| g.take())) else { return };
    v.diffoff();
    if v.tab.is_valid() {
        if ui::tab_count() == 1 {
            let _ = api::command("tabnew");
        }
        if let Ok(n) = v.tab.get_number() {
            let _ = api::command(&format!("tabclose {n}"));
        }
    }
    v.cleanup();
}

pub fn goto_file(delta: i64) {
    with_view(|v| {
        let len = v.flat().len() as i64;
        let j = v.current_index().map_or(-1, |i| i as i64) + delta;
        if j < 0 || j >= len {
            return ui::notify(if delta > 0 { "last file" } else { "first file" }, ui::INFO);
        }
        report(v.show(j as usize));
    });
}

pub fn goto_group(delta: i64) {
    with_view(|v| {
        let flat = v.flat();
        let gi = v.current_index().map_or(-1, |i| flat[i].0 as i64);
        let target = gi + delta;
        if target < 0 || target >= v.model.groups.len() as i64 {
            return ui::notify(if delta > 0 { "last group" } else { "first group" }, ui::INFO);
        }
        if let Some(j) = flat.iter().position(|&(g, _)| g as i64 == target) {
            report(v.show(j));
        }
    });
}

pub fn toggle_narrative() {
    with_view(|v| report(v.toggle_narrative()));
}

pub fn toggle_reasons() {
    CONFIG.with(|c| {
        let mut c = c.borrow_mut();
        let cfg = c.get_or_insert_with(Config::default);
        cfg.show_reasons = !cfg.show_reasons;
    });
    with_view(|v| {
        v.cfg.show_reasons = !v.cfg.show_reasons;
        v.redraw();
    });
}

pub fn select_at_cursor() {
    with_view(|v| match v.item_at_cursor() {
        Some(Item::Group(gi)) => v.toggle_fold(gi),
        Some(Item::File(gi, fi)) => {
            let path = v.model.groups[gi].files[fi].path.clone();
            if let Some(idx) = v.model.index_of(&path) {
                report(v.show(idx));
            }
        }
        _ => {}
    });
}

pub fn fold_at_cursor() {
    with_view(|v| match v.item_at_cursor() {
        Some(Item::Group(gi) | Item::File(gi, _)) => v.toggle_fold(gi),
        _ => {}
    });
}

// Narrative watcher ----------------------------------------------------------

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Every 300ms: reload when this range's narrative file changes (so a
/// /diff-narrative run in any Claude session shows up here), and drop the
/// view if the user closed its tab.
///
/// This replaces TabClosed/ColorScheme autocmds: nvim-oxi's
/// `CreateAutocmdOpts` doesn't match Neovim 0.12.5's layout (0.12.5 added
/// `buf`), so creating autocmds through it is unsafe.
fn watch(path: PathBuf) -> Option<TimerHandle> {
    let mut last = mtime(&path);
    let every = Duration::from_millis(300);
    TimerHandle::start(every, every, move |_| {
        // Runs in libuv's fast context: only plain Rust here, the rest is
        // scheduled onto the main loop.
        let now = mtime(&path);
        let changed = now != last;
        last = now;
        nvim_oxi::schedule(move |_| tick(changed));
        Ok::<_, Infallible>(())
    })
    .ok()
}

fn tick(narrative_changed: bool) {
    let tab_gone = with_view(|v| !v.tab.is_valid()).unwrap_or(false);
    if tab_gone {
        if let Some(v) = VIEW.with(|c| c.try_borrow_mut().ok().and_then(|mut g| g.take())) {
            v.cleanup();
        }
        return;
    }
    if narrative_changed {
        reload_narrative();
    }
}

// Generate -------------------------------------------------------------------

/// Run the diff-narrative skill (config.generate_cmd) in the background and
/// reload when it finishes. With `spec`, (re)opens the view on that range.
pub fn generate(spec: Option<String>) {
    let spec = spec.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let open_spec = with_view(|v| v.model.spec.clone());
    if open_spec.is_none() || (spec.is_some() && spec != open_spec) {
        return open_then(spec.unwrap_or_default(), Some(Box::new(start_generate)));
    }
    start_generate();
}

fn start_generate() {
    let started = with_view(|v| {
        if v.generating {
            ui::notify("already generating", ui::INFO);
            return None;
        }
        v.generating = true;
        v.redraw();
        Some((v.model.clone(), v.cfg.generate_cmd_for(&v.model.spec)))
    })
    .flatten();
    let Some((mut model, cmd)) = started else { return };
    ui::notify("generating narrative…", ui::INFO);
    let spawned = bg::spawn(
        move || {
            // Refresh groups.json and the fingerprint for the skill.
            model.rescan()?;
            run_generate_cmd(&cmd, &model.repo.root)
        },
        generated,
    );
    if let Err(e) = spawned {
        with_view(|v| {
            v.generating = false;
            v.redraw();
        });
        ui::notify(&e, ui::ERROR);
    }
}

fn run_generate_cmd(cmd: &[String], root: &std::path::Path) -> Result<(), String> {
    let out = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {}: {e}", cmd[0]))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr });
    let tail: String = err.trim().chars().rev().take(500).collect::<Vec<_>>().into_iter().rev().collect();
    Err(format!("narrative generation failed: {tail}"))
}

fn generated(res: Result<(), String>) {
    match &res {
        Ok(()) => ui::notify("narrative ready", ui::INFO),
        Err(e) => ui::notify(e, ui::ERROR),
    }
    with_view(|v| v.generating = false);
    refresh_then(Some(Box::new(|| {
        with_view(|v| {
            if v.model.narrative.is_some() {
                report(v.open_narrative());
            }
        });
    })));
}

// Registration ---------------------------------------------------------------

const HIGHLIGHTS: &[(&str, &str)] = &[
    ("StructDiffTitle", "Title"),
    ("StructDiffRange", "Constant"),
    ("StructDiffGroup", "Directory"),
    ("StructDiffCount", "Comment"),
    ("StructDiffDir", "Comment"),
    ("StructDiffReason", "Comment"),
    ("StructDiffWhy", "Special"),
    ("StructDiffCurrent", "Visual"),
    ("StructDiffAdded", "Added"),
    ("StructDiffChanged", "Changed"),
    ("StructDiffRemoved", "Removed"),
    ("StructDiffFresh", "DiagnosticOk"),
    ("StructDiffStale", "DiagnosticWarn"),
    ("StructDiffNone", "Comment"),
];

/// `:highlight default link`, so colorschemes and user overrides win. Done
/// with Ex commands rather than `api::set_hl`, whose options struct doesn't
/// match Neovim 0.12.5. Re-run on every redraw, which also restores the links
/// after a colorscheme change clears them.
pub fn set_highlights() {
    for (name, link) in HIGHLIGHTS {
        let _ = api::command(&format!("highlight default link {name} {link}"));
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

/// Neovim versions whose C API structs match this build of nvim-oxi.
fn supported_neovim() -> bool {
    let has = |feature: &str| api::call_function::<_, i64>("has", (feature,)).unwrap_or(0) == 1;
    has("nvim-0.12") && !has("nvim-0.13")
}

/// Register commands and highlights; return the module table.
pub fn init() -> nvim_oxi::Result<Dictionary> {
    if !supported_neovim() {
        // Talking to a Neovim with a different API layout could corrupt
        // memory, so do nothing at all.
        ui::notify("this build supports Neovim 0.12 only; rebuild against your Neovim's nvim-oxi feature", ui::ERROR);
        return Ok(Dictionary::new());
    }
    range_command("StructDiff", "Open grouped diff: [rev | A..B | A...B], default HEAD vs working tree", open)?;
    command("StructDiffClose", "Close the StructDiff view", close)?;
    command("StructDiffRefresh", "Re-scan changes and reload the narrative", refresh)?;
    command("StructDiffNarrative", "Toggle the narrative split", toggle_narrative)?;
    range_command("StructDiffGenerate", "Generate the change narrative with Claude: [range]", generate)?;

    set_highlights();

    fn f0(f: fn()) -> Object {
        Object::from(Function::<(), ()>::from_fn(move |(): ()| {
            f();
            Ok::<_, Infallible>(())
        }))
    }
    fn f1(f: fn(Option<String>)) -> Object {
        Object::from(Function::<Option<String>, ()>::from_fn(move |spec: Option<String>| {
            f(spec);
            Ok::<_, Infallible>(())
        }))
    }
    fn fdelta(f: fn(i64)) -> Object {
        Object::from(Function::<i64, ()>::from_fn(move |d: i64| {
            f(d);
            Ok::<_, Infallible>(())
        }))
    }
    Ok(Dictionary::from_iter([
        (
            "setup",
            Object::from(Function::<Object, ()>::from_fn(|opts: Object| {
                setup(opts);
                Ok::<_, Infallible>(())
            })),
        ),
        ("open", f1(open)),
        ("generate", f1(generate)),
        ("close", f0(close)),
        ("refresh", f0(refresh)),
        ("reload_narrative", f0(reload_narrative)),
        ("toggle_narrative", f0(toggle_narrative)),
        ("toggle_reasons", f0(toggle_reasons)),
        ("goto_file", fdelta(goto_file)),
        ("goto_group", fdelta(goto_group)),
    ]))
}

#[nvim_oxi::plugin]
fn structdiff() -> nvim_oxi::Result<Dictionary> {
    init()
}
