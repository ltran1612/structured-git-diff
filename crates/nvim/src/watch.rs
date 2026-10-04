//! A poller that stands in for autocmds: it reloads the narrative when its
//! file changes (so a /diff-narrative run in any Claude session shows up) and
//! drops the view when the user closes its tab.
//!
//! The timer runs in libuv's fast context, where checking a file's mtime is
//! fine but Neovim's API (even `nvim_tabpage_is_valid`) is not. So it only
//! schedules main-loop work when the file changed, or every
//! [`TAB_CHECK_EVERY`] ticks to notice a closed tab.
//!
//! nvim-oxi's `CreateAutocmdOpts` doesn't match Neovim 0.12.5's layout
//! (0.12.5 added `buf`), so creating TabClosed autocmds through it is unsafe.

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use nvim_oxi::libuv::TimerHandle;

use crate::actions;
use crate::state::{self, with_view};

/// How often the narrative file's mtime is checked.
pub const TICK: Duration = Duration::from_millis(300);
/// Check for a closed tab every this many ticks (~1.2s).
pub const TAB_CHECK_EVERY: u32 = 4;

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Start polling `narrative_path`. The view stops the timer on cleanup.
pub fn start(narrative_path: PathBuf) -> Option<TimerHandle> {
    let mut last = mtime(&narrative_path);
    let mut ticks: u32 = 0;
    TimerHandle::start(TICK, TICK, move |_| {
        let now = mtime(&narrative_path);
        let changed = now != last;
        last = now;
        ticks = ticks.wrapping_add(1);
        if changed || ticks.is_multiple_of(TAB_CHECK_EVERY) {
            nvim_oxi::schedule(move |_| tick(changed));
        }
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
