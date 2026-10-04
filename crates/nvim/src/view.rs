//! The open view: one tab with | sidebar | base | target |, plus an optional
//! narrative split along the bottom.

use std::collections::HashSet;

use nvim_oxi::api::{self, Buffer, TabPage, Window, opts::SetExtmarkOpts};
use nvim_oxi::libuv::TimerHandle;
use structdiff_core::narrative::{self, State};
use structdiff_core::{ChangedFile, Model};

use crate::config::Config;
use crate::ui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Header,
    Blank,
    Group(usize),
    File(usize, usize),
}

enum Mark {
    Hl { row: usize, start: usize, end: usize, group: &'static str },
    Line { row: usize, group: &'static str },
    Virt { row: usize, lines: Vec<String>, group: &'static str },
}

pub struct View {
    pub model: Model,
    pub cfg: Config,
    pub current: Option<String>,
    pub collapsed: HashSet<String>,
    pub tab: TabPage,
    pub sidebar: Window,
    pub left: Window,
    pub right: Window,
    pub narrative_win: Option<Window>,
    pub panel_buf: Buffer,
    pub narrative_buf: Option<Buffer>,
    pub line_items: Vec<Item>,
    pub real_buf: Option<Buffer>,
    pub generating: bool,
    pub watcher: Option<TimerHandle>,
    /// Bumped per background rescan; only the latest result is applied.
    pub scan_gen: u64,
    pub pending_scans: u32,
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
    pub fn new(model: Model, cfg: Config) -> Result<Self, api::Error> {
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
            model,
            cfg,
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
            generating: false,
            watcher: None,
            scan_gen: 0,
            pending_scans: 0,
        };
        view.split_windows()?;
        Ok(view)
    }

    fn split_windows(&mut self) -> Result<(), api::Error> {
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
        self.sidebar.set_width(self.cfg.sidebar_width)?;
        api::command("wincmd =")
    }

    /// Rebuild the windows if the user closed some of them.
    pub fn ensure_layout(&mut self) -> Result<(), api::Error> {
        if [&self.sidebar, &self.left, &self.right].iter().all(|w| ui::same_tab(w, &self.tab)) {
            return Ok(());
        }
        api::set_current_tabpage(&self.tab)?;
        let _ = api::command("silent! only");
        self.split_windows()
    }

    pub fn flat(&self) -> Vec<(usize, usize)> {
        self.model.flat()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current.as_deref().and_then(|p| self.model.index_of(p))
    }

    // Diff panes -----------------------------------------------------------

    fn rev_lines(&self, rev: &str, path: &str) -> Vec<String> {
        self.model.repo.content(rev, path).map(|b| ui::to_lines(&b)).unwrap_or_default()
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
        self.right.call::<_, _, ()>(|_| {
            api::command("normal! gg")?;
            let hl: i64 = api::call_function("diff_hlID", (1, 1))?;
            if hl == 0 {
                api::command("silent! normal! ]c")?;
            }
            Ok::<_, api::Error>(())
        })?;
        Ok((right, real))
    }

    pub fn diffoff(&self) {
        if ui::same_tab(&self.left, &self.tab) {
            let _ = ui::win_cmd(&self.left, "diffoff!");
        }
    }

    /// Show the idx-th file of the display order.
    pub fn show(&mut self, idx: usize) -> Result<(), api::Error> {
        let flat = self.flat();
        let Some(&pos) = flat.get(idx) else { return Ok(()) };
        let file = self.model.file(pos).clone();
        self.current = Some(file.path.clone());
        self.collapsed.remove(&self.model.groups[pos.0].name);
        self.ensure_layout()?;

        self.unmap_real();
        let (mut right, real) = self.show_diff(&file)?;
        if real {
            crate::keys::map_nav(&mut right, &self.cfg);
            self.real_buf = Some(right);
        } else {
            crate::keys::map_view(&mut right, &self.cfg);
        }
        let mut left = self.left.get_buf()?;
        crate::keys::map_view(&mut left, &self.cfg);
        self.redraw();
        Ok(())
    }

    /// Real files only carry navigation keys while shown, so q / R / gn keep
    /// their normal meaning when editing.
    pub fn unmap_real(&mut self) {
        if let Some(mut buf) = self.real_buf.take()
            && buf.is_valid()
        {
            crate::keys::unmap_nav(&mut buf, &self.cfg);
        }
    }

    // Sidebar --------------------------------------------------------------

    pub fn redraw(&mut self) {
        crate::set_highlights();
        self.render_panel();
        self.focus_current();
        self.render_narrative();
    }

    fn render_panel(&mut self) {
        let width = if self.sidebar.is_valid() {
            self.sidebar.get_width().unwrap_or(self.cfg.sidebar_width)
        } else {
            self.cfg.sidebar_width
        } as usize;
        let reasons = if self.cfg.show_reasons { self.model.narrative.as_ref() } else { None };
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
                if self.current.as_deref() == Some(f.path.as_str()) {
                    marks.push(Mark::Line { row, group: "StructDiffCurrent" });
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
                Mark::Line { row, group } => (row, (0, SetExtmarkOpts::builder().line_hl_group(group).build())),
                Mark::Virt { row, lines, group } => {
                    let virt = lines.into_iter().map(|l| [(l, group)]);
                    (row, (0, SetExtmarkOpts::builder().virt_lines(virt).build()))
                }
            };
            let _ = self.panel_buf.set_extmark(ns, row, opts.0, &opts.1);
        }
        self.line_items = items;
    }

    pub fn item_at_cursor(&self) -> Option<Item> {
        let (line, _) = self.sidebar.get_cursor().ok()?;
        self.line_items.get(line.checked_sub(1)?).copied()
    }

    pub fn focus_current(&mut self) {
        let Some(cur) = self.current.clone() else { return };
        let row = self.line_items.iter().position(|it| match *it {
            Item::File(gi, fi) => self.model.groups[gi].files[fi].path == cur,
            _ => false,
        });
        if let Some(row) = row
            && self.sidebar.is_valid()
        {
            let _ = self.sidebar.set_cursor(row + 1, 2);
        }
    }

    pub fn toggle_fold(&mut self, gi: usize) {
        let name = self.model.groups[gi].name.clone();
        if !self.collapsed.remove(&name) {
            self.collapsed.insert(name);
        }
        self.render_panel();
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

    pub fn open_narrative(&mut self) -> Result<(), api::Error> {
        if self.narrative_open() {
            return Ok(());
        }
        if !self.narrative_buf.as_ref().is_some_and(Buffer::is_valid) {
            let mut buf = api::create_buf(false, true)?;
            buf.set_name("structdiff://narrative")?;
            ui::buf_opt(&buf, "bufhidden", "hide");
            ui::buf_opt(&buf, "filetype", "markdown");
            crate::keys::map_narrative(&mut buf, &self.cfg);
            self.narrative_buf = Some(buf);
        }
        self.render_narrative();
        let cmd = format!("botright {}split", self.cfg.narrative_height);
        let mut win = self.right.call::<_, _, Window>(move |_| {
            api::command(&cmd)?;
            Ok::<_, api::Error>(Window::current())
        })?;
        win.set_buf(self.narrative_buf.as_ref().expect("created above"))?;
        ui::win_opt(&win, "wrap", true);
        ui::win_opt(&win, "linebreak", true);
        ui::win_opt(&win, "winfixheight", true);
        ui::win_opt(&win, "conceallevel", 2i64);
        self.narrative_win = Some(win);
        Ok(())
    }

    pub fn toggle_narrative(&mut self) -> Result<(), api::Error> {
        match self.narrative_win.take() {
            Some(win) if ui::same_tab(&win, &self.tab) => win.close(true),
            _ => self.open_narrative(),
        }
    }

    // Lifecycle ------------------------------------------------------------

    /// Free everything except the tab itself.
    pub fn cleanup(mut self) {
        if let Some(mut t) = self.watcher.take() {
            let _ = t.stop();
        }
        self.unmap_real();
        for buf in [Some(self.panel_buf.clone()), self.narrative_buf.take()].into_iter().flatten() {
            if buf.is_valid() {
                let _ = buf.delete(&api::opts::BufDeleteOpts::builder().force(true).build());
            }
        }
    }
}
