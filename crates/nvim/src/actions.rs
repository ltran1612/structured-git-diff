//! The controller: what each command and key does. Runs git on background
//! threads, keeps the view's keymaps in step with what it shows, and reads the
//! config fresh on every action.

use std::path::PathBuf;

use nvim_oxi::api::{self, Buffer};
use structdiff_core::{Model, Repo, narrative};

use crate::config::Config;
use crate::state::{self, with_view};
use crate::view::{Item, View};
use crate::{bg, keys, ui, watch};

/// Something to run on the main loop once background work has landed.
pub(crate) type Then = Option<Box<dyn FnOnce()>>;

pub(crate) fn report(res: Result<(), api::Error>) {
    if let Err(e) = res {
        ui::notify(&e.to_string(), ui::ERROR);
    }
}

pub(crate) fn notify_all(warnings: Vec<String>) {
    for w in warnings {
        ui::notify(&w, ui::WARN);
    }
}

// Showing files ----------------------------------------------------------------

/// Show the idx-th file and give the panes their keys. The previous real
/// file loses its navigation keys, so q / R / gn keep their usual meaning
/// when you edit it outside the view.
fn show(v: &mut View, idx: usize, cfg: &Config) -> Result<(), api::Error> {
    if let Some((mut old, maps)) = v.take_real_buf()
        && old.is_valid()
    {
        ui::restore_maps(&mut old, maps);
    }
    let Some(shown) = v.show(idx, cfg)? else { return Ok(()) };
    let mut right = shown.right;
    if shown.right_is_real {
        let saved = keys::map_nav(&mut right, cfg);
        v.set_real_maps(saved);
    } else {
        keys::map_view(&mut right, cfg);
    }
    let mut left = shown.left;
    keys::map_view(&mut left, cfg);
    Ok(())
}

fn map_new_narrative(created: Result<Option<Buffer>, api::Error>, cfg: &Config) {
    match created {
        Ok(Some(mut buf)) => keys::map_narrative(&mut buf, cfg),
        Ok(None) => {}
        Err(e) => ui::notify(&e.to_string(), ui::ERROR),
    }
}

pub(crate) fn open_narrative(v: &mut View, cfg: &Config) {
    map_new_narrative(v.open_narrative(cfg), cfg);
}

// Opening, refreshing, closing -----------------------------------------------------

/// Open the view. `spec` picks what to compare (see `Repo::resolve_range`).
/// The same spec again refreshes; a different spec replaces the view. Git
/// runs on a background thread; the view appears when it's done.
pub fn open(spec: Option<String>) {
    open_then(spec.unwrap_or_default(), None);
}

pub(crate) fn open_then(spec: String, then: Then) {
    let spec = spec.trim().to_owned();
    let existing = with_view(|v| (v.model().spec.clone(), v.tab().clone()));
    if let Some((open_spec, tab)) = existing {
        if open_spec == spec && tab.is_valid() {
            report(api::set_current_tabpage(&tab));
            return refresh_then(then);
        }
        close();
    }
    if state::loading() {
        return ui::notify("already loading", ui::INFO);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo = match Repo::discover(&cwd) {
        Ok(r) => r,
        Err(e) => return ui::notify(&e.to_string(), ui::ERROR),
    };
    let grouping = state::config().grouping();
    state::set_loading(true);
    let started = bg::spawn(
        move || Model::load(repo, &spec, &grouping),
        move |res| {
            state::set_loading(false);
            if finish_open(res)
                && let Some(then) = then
            {
                then();
            }
        },
    );
    if let Err(e) = started {
        state::set_loading(false);
        ui::notify(&e, ui::ERROR);
    }
}

/// Create the view for a freshly loaded model. False when there is nothing
/// to show.
fn finish_open(res: structdiff_core::error::Result<(Model, Vec<String>)>) -> bool {
    let (model, warnings) = match res {
        Ok(m) => m,
        Err(e) => {
            ui::notify(&e.to_string(), ui::ERROR);
            return false;
        }
    };
    notify_all(warnings);
    if model.file_count() == 0 {
        ui::notify(&format!("no changes in {}", model.range.describe()), ui::INFO);
        return false;
    }
    let cfg = state::config();
    let has_narrative = model.narrative.is_some();
    let watch_path = narrative::path(&model.repo.root, &model.spec);
    let mut v = match View::new(model, &cfg) {
        Ok(v) => v,
        Err(e) => {
            ui::notify(&e.to_string(), ui::ERROR);
            return false;
        }
    };
    keys::map_panel(v.panel_buf_mut(), &cfg);
    v.set_watcher(watch::start(watch_path));
    state::put_view(v);
    with_view(|v| {
        report(show(v, 0, &cfg));
        if has_narrative {
            open_narrative(v, &cfg);
        }
        report(api::set_current_win(v.sidebar()));
        v.focus_current();
    });
    true
}

/// Rescan git in the background and redraw. Rescans may overlap; only the
/// latest one's result is applied.
pub fn refresh() {
    refresh_then(None);
}

pub(crate) fn refresh_then(then: Then) {
    let grouping = state::config().grouping();
    let Some((mut model, scan)) = with_view(|v| (v.model().clone(), v.begin_scan())) else { return };
    let started = bg::spawn(
        move || {
            let res = model.rescan(&grouping);
            (model, res)
        },
        move |(model, res)| {
            if with_view(|v| v.finish_scan(scan)) != Some(true) {
                return;
            }
            match res {
                Err(e) => return ui::notify(&e.to_string(), ui::ERROR),
                Ok(w) => notify_all(w),
            }
            if model.file_count() == 0 {
                ui::notify("no changes left", ui::INFO);
                return close();
            }
            let cfg = state::config();
            with_view(|v| {
                v.set_model(model);
                let idx = v.current_index().unwrap_or(0);
                report(show(v, idx, &cfg));
            });
            if let Some(then) = then {
                then();
            }
        },
    );
    if let Err(e) = started {
        with_view(|v| v.finish_scan(scan));
        ui::notify(&e, ui::ERROR);
    }
}

/// Reload only the narrative file (cheap, so on the main thread).
pub fn reload_narrative() {
    let cfg = state::config();
    with_view(|v| {
        notify_all(v.model_mut().reload_narrative(&cfg.grouping()));
        v.redraw(&cfg);
    });
}

/// Close the view and its tab.
pub fn close() {
    let Some(v) = state::take_view() else { return };
    v.diffoff();
    if v.tab().is_valid() {
        if ui::tab_count() == 1 {
            let _ = api::command("tabnew");
        }
        if let Ok(n) = v.tab().get_number() {
            let _ = api::command(&format!("tabclose {n}"));
        }
    }
    discard(v);
}

/// Free a view whose tab is already gone (or being closed).
pub(crate) fn discard(mut v: View) {
    if let Some((mut real, maps)) = v.take_real_buf()
        && real.is_valid()
    {
        ui::restore_maps(&mut real, maps);
    }
    v.cleanup();
}

// Navigation ---------------------------------------------------------------------

pub fn goto_file(delta: i64) {
    let cfg = state::config();
    with_view(|v| {
        let len = v.flat().len() as i64;
        let j = v.current_index().map_or(-1, |i| i as i64) + delta;
        if j < 0 || j >= len {
            return ui::notify(if delta > 0 { "last file" } else { "first file" }, ui::INFO);
        }
        report(show(v, j as usize, &cfg));
    });
}

pub fn goto_group(delta: i64) {
    let cfg = state::config();
    with_view(|v| {
        let flat = v.flat();
        let gi = v.current_index().map_or(-1, |i| flat[i].0 as i64);
        let target = gi + delta;
        if target < 0 || target >= v.model().groups.len() as i64 {
            return ui::notify(if delta > 0 { "last group" } else { "first group" }, ui::INFO);
        }
        if let Some(j) = flat.iter().position(|&(g, _)| g as i64 == target) {
            report(show(v, j, &cfg));
        }
    });
}

pub fn toggle_narrative() {
    let cfg = state::config();
    with_view(|v| map_new_narrative(v.toggle_narrative(&cfg), &cfg));
}

pub fn toggle_reasons() {
    state::update_config(|c| c.show_reasons = !c.show_reasons);
    let cfg = state::config();
    with_view(|v| v.redraw(&cfg));
}

pub fn select_at_cursor() {
    let cfg = state::config();
    with_view(|v| match v.item_at_cursor() {
        Some(Item::Group(gi)) => v.toggle_fold(gi, &cfg),
        Some(Item::File(gi, fi)) => {
            let path = v.model().groups[gi].files[fi].path.clone();
            if let Some(idx) = v.model().index_of(&path) {
                report(show(v, idx, &cfg));
            }
        }
        _ => {}
    });
}

pub fn fold_at_cursor() {
    let cfg = state::config();
    with_view(|v| {
        if let Some(Item::Group(gi) | Item::File(gi, _)) = v.item_at_cursor() {
            v.toggle_fold(gi, &cfg);
        }
    });
}
