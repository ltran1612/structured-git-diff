//! Highlight groups, all `default` links so colorschemes and users can
//! override them.

use nvim_oxi::api;

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

/// `:highlight default link` for every group. Ex commands rather than
/// `api::set_hl`, whose options struct doesn't match Neovim 0.12.5. Called on
/// every redraw, which also restores the links after a colorscheme change
/// clears them (there is no ColorScheme autocmd; see the notes in lib.rs).
pub fn apply() {
    for (name, link) in HIGHLIGHTS {
        let _ = api::command(&format!("highlight default link {name} {link}"));
    }
}
