use std::path::Path;

use nvim_oxi::api::{self, Buffer, Window};

pub use structdiff_testutil::*;

/// Register the plugin and cd into `root`.
pub fn start(root: &Path) {
    structdiff::init().unwrap();
    api::set_current_dir(root).unwrap();
}

pub fn lines(buf: &Buffer) -> Vec<String> {
    buf.get_lines(.., false).unwrap().map(|s| s.to_string_lossy().into_owned()).collect()
}

pub fn win_lines(win: &Window) -> Vec<String> {
    lines(&win.get_buf().unwrap())
}

pub fn name(win: &Window) -> String {
    structdiff::ui::buf_name(&win.get_buf().unwrap())
}

pub fn panel() -> Vec<String> {
    structdiff::with_view(|v| lines(v.panel_buf())).expect("view open")
}

pub fn current() -> Option<String> {
    structdiff::with_view(|v| v.current().map(str::to_owned)).flatten()
}

/// Let Neovim run its event loop (timers, scheduled callbacks) until `done`.
pub fn wait_until(ms: u64, mut done: impl FnMut() -> bool) -> bool {
    let mut waited = 0;
    while waited < ms {
        if done() {
            return true;
        }
        api::command("sleep 20m").unwrap();
        waited += 20;
    }
    done()
}

/// All virtual-line texts in the sidebar, trimmed.
pub fn virt_texts() -> Vec<String> {
    use nvim_oxi::api::opts::GetExtmarksOpts;
    use nvim_oxi::api::types::ExtmarkPosition;
    structdiff::with_view(|v| {
        let ns = api::create_namespace("structdiff");
        let opts = GetExtmarksOpts::builder().details(true).build();
        v.panel_buf()
            .get_extmarks(ns, ExtmarkPosition::ByTuple((0, 0)), ExtmarkPosition::ByTuple((usize::MAX >> 33, 0)), &opts)
            .unwrap()
            .flat_map(|(_, _, _, details)| {
                details.and_then(|d| d.virt_lines).unwrap_or_default().into_iter().flatten().map(|(t, _)| t.trim().to_owned())
            })
            .collect()
    })
    .unwrap()
}


/// `open`, then wait for the background load.
pub fn open_wait(spec: Option<&str>) {
    structdiff::open(spec.map(str::to_owned));
    assert!(wait_until(5000, || !structdiff::busy()), "open timed out");
}

/// `refresh`, then wait for the background rescan.
pub fn refresh_wait() {
    structdiff::refresh();
    assert!(wait_until(5000, || !structdiff::busy()), "refresh timed out");
}

/// Route `vim.notify` into a list readable with [`notifications`].
pub fn capture_notifications() {
    let _: nvim_oxi::Object = api::call_function(
        "luaeval",
        ("(function() _G.__structdiff_msgs = {}; vim.notify = function(m) table.insert(_G.__structdiff_msgs, m) end end)()",),
    )
    .unwrap();
}

pub fn notifications() -> Vec<String> {
    api::call_function("luaeval", ("_G.__structdiff_msgs",)).unwrap()
}
