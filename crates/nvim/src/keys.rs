//! Buffer-local keymaps. Callbacks go through the crate's public functions,
//! which borrow the view only for as long as they run.

use nvim_oxi::api::Buffer;

use crate::config::{Config, Keys};
use crate::ui;

fn bind(buf: &mut Buffer, keys: &Keys, desc: &str, f: fn()) {
    for lhs in keys.list() {
        ui::map(buf, lhs, desc, f);
    }
}

fn nav(cfg: &Config) -> [(&Keys, &'static str, fn()); 4] {
    let k = &cfg.keymaps;
    [
        (&k.next_group, "next group", || crate::goto_group(1)),
        (&k.prev_group, "previous group", || crate::goto_group(-1)),
        (&k.next_file, "next file", || crate::goto_file(1)),
        (&k.prev_file, "previous file", || crate::goto_file(-1)),
    ]
}

/// ]g [g ]f [f only: for the real file on the right.
pub fn map_nav(buf: &mut Buffer, cfg: &Config) {
    for (keys, desc, f) in nav(cfg) {
        bind(buf, keys, desc, f);
    }
}

pub fn unmap_nav(buf: &mut Buffer, cfg: &Config) {
    for (keys, _, _) in nav(cfg) {
        for lhs in keys.list() {
            ui::unmap(buf, lhs);
        }
    }
}

/// Navigation plus the view keys, for buffers the view owns.
pub fn map_view(buf: &mut Buffer, cfg: &Config) {
    map_nav(buf, cfg);
    let k = &cfg.keymaps;
    bind(buf, &k.toggle_narrative, "toggle narrative", crate::toggle_narrative);
    bind(buf, &k.toggle_reasons, "toggle reasons", crate::toggle_reasons);
    bind(buf, &k.refresh, "refresh", crate::refresh);
    bind(buf, &k.close, "close", crate::close);
}

pub fn map_panel(buf: &mut Buffer, cfg: &Config) {
    map_view(buf, cfg);
    bind(buf, &cfg.keymaps.select, "open file / toggle group", crate::select_at_cursor);
    bind(buf, &cfg.keymaps.toggle_fold, "toggle group", crate::fold_at_cursor);
}

/// In the narrative split, `q` closes just the split.
pub fn map_narrative(buf: &mut Buffer, cfg: &Config) {
    map_view(buf, cfg);
    bind(buf, &cfg.keymaps.close, "close narrative", crate::toggle_narrative);
}
