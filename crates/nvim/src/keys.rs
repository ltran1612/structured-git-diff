//! Buffer-local keymaps: which key runs which action. Callbacks go through
//! `actions`, which borrows the view only for as long as they run.

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
        (&k.next_group, "next group", || crate::actions::goto_group(1)),
        (&k.prev_group, "previous group", || crate::actions::goto_group(-1)),
        (&k.next_file, "next file", || crate::actions::goto_file(1)),
        (&k.prev_file, "previous file", || crate::actions::goto_file(-1)),
    ]
}

fn bind_nav(buf: &mut Buffer, cfg: &Config) {
    for (keys, desc, f) in nav(cfg) {
        bind(buf, keys, desc, f);
    }
}

/// ]g [g ]f [f only, for the real file on the right. The buffer's own maps
/// on those keys are saved first; give the result to `ui::restore_maps`
/// when the file leaves the view.
pub fn map_nav(buf: &mut Buffer, cfg: &Config) -> ui::SavedMaps {
    let keys: Vec<&str> = nav(cfg).iter().flat_map(|(k, _, _)| k.list()).collect();
    let saved = ui::save_maps(buf, &keys);
    bind_nav(buf, cfg);
    saved
}

/// Navigation plus the view keys, for buffers the view owns.
pub fn map_view(buf: &mut Buffer, cfg: &Config) {
    bind_nav(buf, cfg);
    let k = &cfg.keymaps;
    bind(buf, &k.toggle_narrative, "toggle narrative", crate::actions::toggle_narrative);
    bind(buf, &k.toggle_reasons, "toggle reasons", crate::actions::toggle_reasons);
    bind(buf, &k.refresh, "refresh", crate::actions::refresh);
    bind(buf, &k.close, "close", crate::actions::close);
}

pub fn map_panel(buf: &mut Buffer, cfg: &Config) {
    map_view(buf, cfg);
    bind(buf, &cfg.keymaps.select, "open file / toggle group", crate::actions::select_at_cursor);
    bind(buf, &cfg.keymaps.toggle_fold, "toggle group", crate::actions::fold_at_cursor);
}

/// In the narrative split, `q` closes just the split.
pub fn map_narrative(buf: &mut Buffer, cfg: &Config) {
    map_view(buf, cfg);
    bind(buf, &cfg.keymaps.close, "close narrative", crate::actions::toggle_narrative);
}
