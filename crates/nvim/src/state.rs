//! Plugin-wide state on Neovim's main thread: the open view, the user's
//! config, and whether a view is loading.

use std::cell::{Cell, RefCell};

use crate::config::Config;
use crate::view::View;

thread_local! {
    static VIEW: RefCell<Option<View>> = const { RefCell::new(None) };
    static CONFIG: RefCell<Option<Config>> = const { RefCell::new(None) };
    static LOADING: Cell<bool> = const { Cell::new(false) };
}

/// The current config (defaults until `setup()` is called). The single source
/// of truth: the view reads it on every render.
pub fn config() -> Config {
    CONFIG.with(|c| c.borrow().clone().unwrap_or_default())
}

pub fn set_config(cfg: Config) {
    CONFIG.with(|c| *c.borrow_mut() = Some(cfg));
}

pub fn update_config(f: impl FnOnce(&mut Config)) {
    CONFIG.with(|c| f(c.borrow_mut().get_or_insert_with(Config::default)));
}

/// Run `f` on the open view. None when there is no view, or when the view is
/// already borrowed (a re-entrant call).
pub fn with_view<R>(f: impl FnOnce(&mut View) -> R) -> Option<R> {
    VIEW.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

pub fn put_view(view: View) {
    VIEW.with(|c| *c.borrow_mut() = Some(view));
}

pub fn take_view() -> Option<View> {
    VIEW.with(|c| c.try_borrow_mut().ok().and_then(|mut g| g.take()))
}

pub fn loading() -> bool {
    LOADING.with(Cell::get)
}

pub fn set_loading(on: bool) {
    LOADING.with(|l| l.set(on));
}

/// True while a view is loading or rescanning in the background.
pub fn busy() -> bool {
    loading() || with_view(|v| v.scanning()).unwrap_or(false)
}
