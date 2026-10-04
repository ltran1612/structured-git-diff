//! A 300ms poller that stands in for autocmds: it reloads the narrative when
//! its file changes (so a /diff-narrative run in any Claude session shows up)
//! and drops the view when the user closes its tab.
//!
//! nvim-oxi's `CreateAutocmdOpts` doesn't match Neovim 0.12.5's layout
//! (0.12.5 added `buf`), so creating TabClosed autocmds through it is unsafe.

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use nvim_oxi::libuv::TimerHandle;

use crate::actions;
use crate::state::{self, with_view};

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Start polling `narrative_path`. The view stops the timer on cleanup.
pub fn start(narrative_path: PathBuf) -> Option<TimerHandle> {
    let mut last = mtime(&narrative_path);
    let every = Duration::from_millis(300);
    TimerHandle::start(every, every, move |_| {
        // Runs in libuv's fast context: only plain Rust here, the rest is
        // scheduled onto the main loop.
        let now = mtime(&narrative_path);
        let changed = now != last;
        last = now;
        nvim_oxi::schedule(move |_| tick(changed));
        Ok::<_, Infallible>(())
    })
    .ok()
}

fn tick(narrative_changed: bool) {
    let tab_gone = with_view(|v| !v.tab().is_valid()).unwrap_or(false);
    if tab_gone {
        if let Some(v) = state::take_view() {
            actions::discard(v);
        }
        return;
    }
    if narrative_changed {
        actions::reload_narrative();
    }
}
