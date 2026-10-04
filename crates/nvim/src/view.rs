//! The open view: one tab with | sidebar | base | target |, plus an optional
//! narrative split along the bottom.
//!
//! This layer only draws. It owns no config (callers pass `&Config`) and
//! installs no keymaps: [`View::show`] and [`View::open_narrative`] return
//! the buffers that need keys, and the controller (`actions`) maps them.

use std::collections::HashSet;

use nvim_oxi::api::{self, Buffer, TabPage, Window, opts::SetExtmarkOpts};
use nvim_oxi::libuv::TimerHandle;
use structdiff_core::narrative::{self, State};
use structdiff_core::{ChangedFile, Model};

use crate::config::Config;
use crate::{hl, ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Header,
    Blank,
    Group(usize),
    File(usize, usize),
}

enum Mark {
    Hl { row: usize, start: usize, end: usize, group: &'static str },
    Virt { row: usize, lines: Vec<String>, group: &'static str },
}

/// The current-file highlight has its own namespace, so moving between files
/// only moves this one mark.
fn current_ns() -> u32 {
    api::create_namespace("structdiff-current")
}

/// Identifies a background rescan: the view that started it, and that
/// view's rescan counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanToken {
    view: u64,
    generation: u64,
}

static NEXT_VIEW_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct View {
    /// Unique per view, so background work started by a closed view can't
    /// act on its replacement.
    id: u64,
    model: Model,
    current: Option<String>,
    collapsed: HashSet<String>,
    tab: TabPage,
    sidebar: Window,
    left: Window,
    right: Window,
    narrative_win: Option<Window>,
    panel_buf: Buffer,
    narrative_buf: Option<Buffer>,
    line_items: Vec<Item>,
    /// The working-tree file shown on the right, which carries navigation
    /// keys only while it is shown.
    real_buf: Option<Buffer>,
    /// The real buffer's own maps that structdiff's navigation keys replaced.
    real_maps: ui::SavedMaps,
    generating: bool,
    watcher: Option<TimerHandle>,
    /// Bumped per background rescan; only the latest result is applied.
    scan_gen: u64,
    pending_scans: u32,
    /// The sidebar no longer matches the model and must be fully redrawn.
    panel_dirty: bool,
}

/// The buffers [`View::show`] put in the diff panes.
pub struct Shown {
    pub left: Buffer,
    pub right: Buffer,
    /// The right side is the real working-tree file (not a scratch buffer).
    pub right_is_real: bool,
}

/// The first 8000 bytes of a file, enough to tell binary from text.
fn read_head(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.take(8000).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn status_hl(status: char) -> &'static str {
    match status {
        'A' | '?' => "StructDiffAdded",
        'D' | 'U' => "StructDiffRemoved",
        _ => "StructDiffChanged",
    }
}

impl View {
    /// Open a new tab for `model`. The caller shows the first file.
    pub fn new(model: Model, cfg: &Config) -> Result<Self, api::Error> {
        api::command("tabnew")?;
        let tab = api::get_current_tabpage();
        ui::buf_opt(&api::get_current_buf(), "bufhidden", "wipe");
        let mut panel_buf = api::create_buf(false, true)?;
        panel_buf.set_name("structdiff://panel")?;
        ui::buf_opt(&panel_buf, "bufhidden", "hide");
        ui::buf_opt(&panel_buf, "modifiable", false);
        ui::buf_opt(&panel_buf, "filetype", "structdiff");
        let current = Window::current();
        let mut view = Self {
            id: NEXT_VIEW_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            model,
            current: None,
            collapsed: HashSet::new(),
            tab,
            sidebar: current.clone(),
            left: current.clone(),
            right: current,
            narrative_win: None,
            panel_buf,
            narrative_buf: None,
            line_items: Vec::new(),
            real_buf: None,
            real_maps: ui::SavedMaps::default(),
            generating: false,
            watcher: None,
            scan_gen: 0,
            pending_scans: 0,
            panel_dirty: true,
        };
        view.split_windows(cfg)?;
        Ok(view)
    }

    // Accessors ------------------------------------------------------------

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn set_model(&mut self, model: Model) {
        self.model = model;
        self.panel_dirty = true;
    }

    pub fn model_mut(&mut self) -> &mut Model {
        self.panel_dirty = true;
        &mut self.model
    }

    pub fn tab(&self) -> &TabPage {
        &self.tab
    }

    pub fn sidebar(&self) -> &Window {
        &self.sidebar
    }

    pub fn left(&self) -> &Window {
        &self.left
    }

    pub fn right(&self) -> &Window {
        &self.right
    }

    pub fn panel_buf(&self) -> &Buffer {
        &self.panel_buf
    }

    pub fn panel_buf_mut(&mut self) -> &mut Buffer {
        &mut self.panel_buf
    }

    pub fn narrative_buf(&self) -> Option<&Buffer> {
        self.narrative_buf.as_ref()
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn real_buf(&self) -> Option<&Buffer> {
        self.real_buf.as_ref()
    }

    /// Hand over the real file buffer so its keys can be removed.
    pub fn take_real_buf(&mut self) -> Option<(Buffer, ui::SavedMaps)> {
        let buf = self.real_buf.take()?;
        Some((buf, std::mem::take(&mut self.real_maps)))
    }

    /// Remember the maps structdiff replaced on the real buffer.
    pub fn set_real_maps(&mut self, maps: ui::SavedMaps) {
        self.real_maps = maps;
    }

    pub fn generating(&self) -> bool {
        self.generating
    }

    pub fn set_generating(&mut self, on: bool) {
        self.generating = on;
        self.panel_dirty = true;
    }

    pub fn set_watcher(&mut self, watcher: Option<TimerHandle>) {
        self.watcher = watcher;
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// Start a background rescan.
    pub fn begin_scan(&mut self) -> ScanToken {
        self.scan_gen += 1;
        self.pending_scans += 1;
        ScanToken { view: self.id, generation: self.scan_gen }
    }

    /// Finish a rescan; true when its result should be applied: it was
    /// started by this view and is the latest one. A rescan started by a
    /// view that has since been replaced is ignored.
    pub fn finish_scan(&mut self, token: ScanToken) -> bool {
        if token.view != self.id {
            return false;
        }
        self.pending_scans = self.pending_scans.saturating_sub(1);
        self.scan_gen == token.generation
    }

    pub fn scanning(&self) -> bool {
        self.pending_scans > 0
    }

    // Layout ---------------------------------------------------------------

    fn split_windows(&mut self, cfg: &Config) -> Result<(), api::Error> {
        self.right = Window::current();
        api::command("leftabove vsplit")?;
        self.left = Window::current();
        api::command("topleft vsplit")?;
        self.sidebar = Window::current();
        self.narrative_win = None;
        self.sidebar.set_buf(&self.panel_buf)?;
        for (opt, val) in [("number", false), ("relativenumber", false), ("wrap", false), ("spell", false), ("list", false)] {
            ui::win_opt(&self.sidebar, opt, val);
        }
        ui::win_opt(&self.sidebar, "cursorline", true);
        ui::win_opt(&self.sidebar, "winfixwidth", true);
        ui::win_opt(&self.sidebar, "signcolumn", "no");
        ui::win_opt(&self.sidebar, "foldcolumn", "0");
        ui::win_opt(&self.sidebar, "statuscolumn", "");
        // Keep <C-o>, <C-^>, :edit etc. from replacing the panel.
        ui::win_opt(&self.sidebar, "winfixbuf", true);
        self.sidebar.set_width(cfg.sidebar_width)?;
        api::command("wincmd =")
    }

    fn sidebar_shows_panel(&self) -> bool {
        self.sidebar.get_buf().is_ok_and(|b| b == self.panel_buf)
    }

    /// Rebuild the windows if the user closed some of them.
    fn ensure_layout(&mut self, cfg: &Config) -> Result<(), api::Error> {
        if [&self.sidebar, &self.left, &self.right].iter().all(|w| ui::same_tab(w, &self.tab)) {
            if !self.sidebar_shows_panel() {
                // Something got past 'winfixbuf' (the API, or the user
                // turned it off): put the panel back.
                ui::win_opt(&self.sidebar, "winfixbuf", false);
                self.sidebar.set_buf(&self.panel_buf)?;
                ui::win_opt(&self.sidebar, "winfixbuf", true);
            }
            return Ok(());
        }
        api::set_current_tabpage(&self.tab)?;
        let _ = api::command("silent! only");
        self.split_windows(cfg)
    }

    pub fn flat(&self) -> &[(usize, usize)] {
        self.model.flat()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current.as_deref().and_then(|p| self.model.index_of(p))
    }

    // Diff panes -----------------------------------------------------------

    /// A revision's file, as display lines. Against the working tree the
    /// left side is read as a checkout would write it (line endings,
    /// smudge filters), so it lines up with the real file on the right.
    fn rev_lines(&self, rev: &str, path: &str) -> Vec<String> {
        let bytes = if self.model.range.target.is_none() {
            self.model.repo.content_checked_out(rev, path)
        } else {
            self.model.repo.content(rev, path)
        };
        bytes.map(|b| ui::to_lines(&b)).unwrap_or_default()
    }

    /// Fill both panes for `file` and diff them. Returns the right-hand buffer
    /// and whether it is the real file (only against the working tree).
    fn show_diff(&mut self, file: &ChangedFile) -> Result<(Buffer, bool), api::Error> {
        let r = self.model.range.clone();
        let left_lines = if matches!(file.status, '?' | 'A') {
            Vec::new()
        } else {
            self.rev_lines(&r.base, file.old_path.as_deref().unwrap_or(&file.path))
        };
        let left = ui::scratch(&format!("structdiff://{}/{}", r.left_label, file.path), &left_lines, Some(&file.path));
        ui::win_cmd(&self.left, "diffoff!")?;
        self.left.set_buf(&left)?;

        let abs = self.model.repo.root.join(&file.path);
        let head = (r.target.is_none() && file.status != 'D').then(|| read_head(&abs)).flatten();
        let (right, real) = if let Some(target) = &r.target {
            let lines = if file.status == 'D' { Vec::new() } else { self.rev_lines(target, &file.path) };
            (ui::scratch(&format!("structdiff://{}/{}", r.right_label, file.path), &lines, Some(&file.path)), false)
        } else if head.as_ref().is_some_and(|b| !ui::is_binary(b)) {
            let escaped: String = api::call_function("fnameescape", (abs.to_string_lossy().into_owned(),))?;
            if let Err(e) = ui::win_cmd(&self.right, &format!("edit {escaped}")) {
                ui::notify(&e.to_string(), ui::ERROR);
            }
            (self.right.get_buf()?, true)
        } else if head.is_some() {
            (ui::scratch(&format!("structdiff://worktree/{}", file.path), &["[binary file]".into()], None), false)
        } else {
            (ui::scratch(&format!("structdiff://deleted/{}", file.path), &[], Some(&file.path)), false)
        };
        if !real {
            self.right.set_buf(&right)?;
        }

        ui::win_cmd(&self.left, "diffthis")?;
        ui::win_cmd(&self.right, "diffthis")?;
        ui::in_win(&self.right, || {
            api::command("normal! gg")?;
            let hl: i64 = api::call_function("diff_hlID", (1, 1))?;
            if hl == 0 {
                api::command("silent! normal! ]c")?;
            }
            Ok(())
        })?;
        Ok((right, real))
    }

    pub fn diffoff(&self) {
        if ui::same_tab(&self.left, &self.tab) {
            let _ = ui::win_cmd(&self.left, "diffoff!");
        }
    }

    /// Show the idx-th file of the display order. The caller must have taken
    /// the previous real buffer ([`View::take_real_buf`]) to unmap it.
    pub fn show(&mut self, idx: usize, cfg: &Config) -> Result<Option<Shown>, api::Error> {
        let Some(&pos) = self.flat().get(idx) else { return Ok(None) };
        let file = self.model.file(pos).clone();
        self.current = Some(file.path.clone());
        let unfolded = self.collapsed.remove(&self.model.groups[pos.0].name);
        self.ensure_layout(cfg)?;

        let (right, right_is_real) = self.show_diff(&file)?;
        if right_is_real {
            self.real_buf = Some(right.clone());
        }
        let left = self.left.get_buf()?;
        if self.panel_dirty || unfolded {
            self.redraw(cfg);
        } else {
            // Only the current file changed: move its highlight and the cursor.
            hl::ensure();
            self.mark_current();
            self.focus_current();
        }
        Ok(Some(Shown { left, right, right_is_real }))
    }

    // Sidebar --------------------------------------------------------------

    /// Fully redraw the sidebar and the narrative split.
    pub fn redraw(&mut self, cfg: &Config) {
        hl::ensure();
        self.render_panel(cfg);
        self.focus_current();
        self.render_narrative();
    }

    fn render_panel(&mut self, cfg: &Config) {
        let width = if self.sidebar.is_valid() {
            self.sidebar.get_width().unwrap_or(cfg.sidebar_width)
        } else {
            cfg.sidebar_width
        } as usize;
        let reasons = if cfg.show_reasons { self.model.narrative.as_ref() } else { None };
        let mut lines: Vec<String> = Vec::new();
        let mut items = Vec::new();
        let mut marks = Vec::new();
        let mut add = |text: String, item: Item, lines: &mut Vec<String>| {
            lines.push(text);
            items.push(item);
            lines.len() - 1
        };

        let n = self.model.file_count();
        let row = add(format!(" StructDiff  {n} file{}", if n == 1 { "" } else { "s" }), Item::Header, &mut lines);
        marks.push(Mark::Hl { row, start: 0, end: lines[row].len(), group: "StructDiffTitle" });
        let row = add(format!(" {}", self.model.range.describe()), Item::Header, &mut lines);
        marks.push(Mark::Hl { row, start: 0, end: lines[row].len(), group: "StructDiffRange" });
        let (label, hl) = if self.generating {
            ("generating narrative…", "StructDiffStale")
        } else {
            match self.model.state {
                State::None => ("no narrative · :StructDiffGenerate", "StructDiffNone"),
                State::Fresh => ("narrative up to date", "StructDiffFresh"),
                State::Stale => ("narrative stale · :StructDiffGenerate", "StructDiffStale"),
                State::Unverified => ("narrative (unverified)", "StructDiffStale"),
            }
        };
        let row = add(format!(" {label}"), Item::Header, &mut lines);
        marks.push(Mark::Hl { row, start: 0, end: lines[row].len(), group: hl });

        for (gi, g) in self.model.groups.iter().enumerate() {
            add(String::new(), Item::Blank, &mut lines);
            let collapsed = self.collapsed.contains(&g.name);
            let head = format!("{} {}", if collapsed { "▸" } else { "▾" }, g.name);
            let row = add(format!("{head} ({})", g.files.len()), Item::Group(gi), &mut lines);
            marks.push(Mark::Hl { row, start: 0, end: head.len(), group: "StructDiffGroup" });
            marks.push(Mark::Hl { row, start: head.len(), end: lines[row].len(), group: "StructDiffCount" });
            if let Some(why) = reasons.and_then(|n| n.groups.get(&g.name)) {
                let wrapped = narrative::wrap(why, width.saturating_sub(3));
                marks.push(Mark::Virt { row, lines: wrapped.iter().map(|l| format!("  {l}")).collect(), group: "StructDiffWhy" });
            }
            if collapsed {
                continue;
            }
            for (fi, f) in g.files.iter().enumerate() {
                let (dir, name) = match f.path.rsplit_once('/') {
                    Some((d, n)) => (Some(d), n),
                    None => (None, f.path.as_str()),
                };
                let mut text = format!("  {} {name}", f.status);
                let dir_start = text.len();
                if let Some(d) = dir {
                    text.push_str("  ");
                    text.push_str(d);
                }
                if let Some(old) = &f.old_path {
                    text.push_str(" ← ");
                    text.push_str(old);
                }
                let row = add(text, Item::File(gi, fi), &mut lines);
                marks.push(Mark::Hl { row, start: 2, end: 2 + f.status.len_utf8(), group: status_hl(f.status) });
                if lines[row].len() > dir_start {
                    marks.push(Mark::Hl { row, start: dir_start, end: lines[row].len(), group: "StructDiffDir" });
                }
                if let Some(reason) = reasons.and_then(|n| n.files.get(&f.path)) {
                    let wrapped = narrative::wrap(reason, width.saturating_sub(7));
                    marks.push(Mark::Virt { row, lines: wrapped.iter().map(|l| format!("      {l}")).collect(), group: "StructDiffReason" });
                }
            }
        }

        ui::set_lines(&mut self.panel_buf, &lines);
        let ns = api::create_namespace("structdiff");
        let _ = self.panel_buf.clear_namespace(ns, ..);
        for m in marks {
            let (row, opts) = match m {
                Mark::Hl { row, start, end, group } => {
                    (row, (start, SetExtmarkOpts::builder().end_col(end).hl_group(group).build()))
                }
                Mark::Virt { row, lines, group } => {
                    let virt = lines.into_iter().map(|l| [(l, group)]);
                    (row, (0, SetExtmarkOpts::builder().virt_lines(virt).build()))
                }
            };
            let _ = self.panel_buf.set_extmark(ns, row, opts.0, &opts.1);
        }
        self.line_items = items;
        self.panel_dirty = false;
        self.mark_current();
    }

    /// Row (0-based) of the current file in the sidebar, if visible.
    fn current_row(&self) -> Option<usize> {
        let cur = self.current.as_deref()?;
        self.line_items.iter().position(|it| match *it {
            Item::File(gi, fi) => self.model.groups[gi].files[fi].path == cur,
            _ => false,
        })
    }

    /// Highlight the current file's line.
    fn mark_current(&mut self) {
        let ns = current_ns();
        let _ = self.panel_buf.clear_namespace(ns, ..);
        if let Some(row) = self.current_row() {
            let opts = SetExtmarkOpts::builder().line_hl_group("StructDiffCurrent").build();
            let _ = self.panel_buf.set_extmark(ns, row, 0, &opts);
        }
    }

    pub fn item_at_cursor(&self) -> Option<Item> {
        let (line, _) = self.sidebar.get_cursor().ok()?;
        self.line_items.get(line.checked_sub(1)?).copied()
    }

    pub fn focus_current(&mut self) {
        // Only move the cursor in the panel, never in a file that ended up
        // in the sidebar window.
        if let Some(row) = self.current_row()
            && self.sidebar.is_valid()
            && self.sidebar_shows_panel()
        {
            let _ = self.sidebar.set_cursor(row + 1, 2);
        }
    }

    pub fn toggle_fold(&mut self, gi: usize, cfg: &Config) {
        let name = self.model.groups[gi].name.clone();
        if !self.collapsed.remove(&name) {
            self.collapsed.insert(name);
        }
        self.render_panel(cfg);
        if let Some(row) = self.line_items.iter().position(|it| *it == Item::Group(gi)) {
            let _ = self.sidebar.set_cursor(row + 1, 0);
        }
    }

    // Narrative split ------------------------------------------------------

    fn render_narrative(&mut self) {
        let Some(buf) = self.narrative_buf.as_mut().filter(|b| b.is_valid()) else { return };
        let lines = narrative::render(self.model.narrative.as_ref(), &self.model.groups, self.model.state, &self.model.spec);
        ui::set_lines(buf, &lines);
    }

    pub fn narrative_open(&self) -> bool {
        self.narrative_win.as_ref().is_some_and(|w| ui::same_tab(w, &self.tab))
    }

    /// Open the narrative split. Returns the narrative buffer when it was just
    /// created (and so needs keymaps).
    pub fn open_narrative(&mut self, cfg: &Config) -> Result<Option<Buffer>, api::Error> {
        if self.narrative_open() {
            return Ok(None);
        }
        let mut created = None;
        if !self.narrative_buf.as_ref().is_some_and(Buffer::is_valid) {
            let mut buf = api::create_buf(false, true)?;
            buf.set_name("structdiff://narrative")?;
            ui::buf_opt(&buf, "bufhidden", "hide");
            ui::buf_opt(&buf, "filetype", "markdown");
            created = Some(buf.clone());
            self.narrative_buf = Some(buf);
        }
        self.render_narrative();
        let cmd = format!("botright {}split", cfg.narrative_height);
        let mut win = ui::in_win(&self.right, move || {
            api::command(&cmd)?;
            Ok(Window::current())
        })?;
        win.set_buf(self.narrative_buf.as_ref().expect("created above"))?;
        ui::win_opt(&win, "wrap", true);
        ui::win_opt(&win, "linebreak", true);
        ui::win_opt(&win, "winfixheight", true);
        ui::win_opt(&win, "conceallevel", 2i64);
        self.narrative_win = Some(win);
        Ok(created)
    }

    /// Close the split if open, else open it (see [`View::open_narrative`]).
    pub fn toggle_narrative(&mut self, cfg: &Config) -> Result<Option<Buffer>, api::Error> {
        match self.narrative_win.take() {
            Some(win) if ui::same_tab(&win, &self.tab) => win.close(true).map(|()| None),
            _ => self.open_narrative(cfg),
        }
    }

    // Lifecycle ------------------------------------------------------------

    /// Free everything except the tab itself. The caller must have taken
    /// the real buffer ([`View::take_real_buf`]) to unmap it.
    pub fn cleanup(mut self) {
        if let Some(mut t) = self.watcher.take() {
            let _ = t.stop();
        }
        for buf in [Some(self.panel_buf.clone()), self.narrative_buf.take()].into_iter().flatten() {
            if buf.is_valid() {
                let _ = buf.delete(&api::opts::BufDeleteOpts::builder().force(true).build());
            }
        }
    }
}
