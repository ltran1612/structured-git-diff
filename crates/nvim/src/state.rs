//! Plugin-wide state on Neovim's main thread: the open view, the user's
//! config, and whether a view is loading.

use std::cell::{Cell, RefCell};

use std::panic::Location;

use crate::config::Config;
use crate::ui;
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

/// A call reached the view while it was already in use: something called back
/// into the plugin from inside an action. The call is dropped (running it
/// would alias the view), but loudly, with the caller's location.
fn reentrant(caller: &Location<'_>) {
    ui::notify(
        &format!("internal: re-entrant call from {}:{} ignored (the view was busy)", caller.file(), caller.line()),
        ui::WARN,
    );
}

/// Run `f` on the open view. None when there is no view, or (with a warning)
/// when the view is already in use by an outer call.
#[track_caller]
pub fn with_view<R>(f: impl FnOnce(&mut View) -> R) -> Option<R> {
    let caller = Location::caller();
    VIEW.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => {
            reentrant(caller);
            None
        }
    })
}

#[track_caller]
pub fn put_view(view: View) {
    let caller = Location::caller();
    VIEW.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => *guard = Some(view),
        Err(_) => reentrant(caller),
    });
}

#[track_caller]
pub fn take_view() -> Option<View> {
    let caller = Location::caller();
    VIEW.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.take(),
        Err(_) => {
            reentrant(caller);
            None
        }
    })
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
