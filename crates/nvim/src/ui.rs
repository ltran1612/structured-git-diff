//! Small Neovim helpers shared by the view.

use nvim_oxi::api::{self, Buffer, Window, opts::OptionOpts, types::Mode};
use nvim_oxi::{Array, Object};

pub const INFO: i64 = 2;
pub const WARN: i64 = 3;
pub const ERROR: i64 = 4;

/// `vim.notify("structdiff: " .. msg, level)`, so noice/snacks pick it up.
pub fn notify(msg: &str, level: i64) {
    let args = Array::from_iter([Object::from(format!("structdiff: {msg}")), Object::from(level)]);
    let _ = api::call_function::<_, Object>("luaeval", ("vim.notify(_A[1], _A[2])", args));
}

pub fn win_opt<V: nvim_oxi::conversion::ToObject>(win: &Window, name: &str, value: V) {
    let _ = api::set_option_value(name, value, &OptionOpts::builder().win(win.clone()).build());
}

pub fn buf_opt<V: nvim_oxi::conversion::ToObject>(buf: &Buffer, name: &str, value: V) {
    let _ = api::set_option_value(name, value, &OptionOpts::builder().buf(buf.clone()).build());
}

pub fn buf_name(buf: &Buffer) -> String {
    buf.get_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn find_buf(name: &str) -> Option<Buffer> {
    api::list_bufs().find(|b| buf_name(b) == name)
}

pub fn filetype_for(path: &str) -> Option<String> {
    api::call_function::<_, String>("luaeval", ("vim.filetype.match({ filename = _A }) or ''", path))
        .ok()
        .filter(|ft| !ft.is_empty())
}

/// Replace a buffer's lines, toggling 'modifiable' around the write.
pub fn set_lines(buf: &mut Buffer, lines: &[String]) {
    buf_opt(buf, "modifiable", true);
    let _ = buf.set_lines(.., false, lines.iter().map(String::as_str));
    buf_opt(buf, "modifiable", false);
    buf_opt(buf, "modified", false);
}

/// A read-only scratch buffer named `name`, reused if it already exists.
/// Wiped once no window shows it.
pub fn scratch(name: &str, lines: &[String], path: Option<&str>) -> Buffer {
    let mut buf = find_buf(name).unwrap_or_else(|| {
        let mut b = api::create_buf(false, true).expect("create scratch buffer");
        let _ = b.set_name(name);
        buf_opt(&b, "bufhidden", "wipe");
        b
    });
    set_lines(&mut buf, lines);
    if let Some(ft) = path.and_then(filetype_for) {
        let current: String =
            api::get_option_value("filetype", &OptionOpts::builder().buf(buf.clone()).build()).unwrap_or_default();
        if current != ft {
            buf_opt(&buf, "filetype", ft);
        }
    }
    buf
}

pub fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8000)].contains(&0)
}

/// Decode file bytes into display lines the way Neovim would read the
/// same file, so revision panes diff cleanly against real buffers:
/// - text that isn't valid UTF-8 is read as Latin-1 (the fallback in
///   Neovim's default 'fileencodings'), which keeps distinct bytes distinct;
/// - when every line ends in CRLF, the CRs are dropped, as Neovim does for
///   a 'fileformat' dos file.
pub fn to_lines(bytes: &[u8]) -> Vec<String> {
    if is_binary(bytes) {
        return vec!["[binary file]".into()];
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    };
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if !lines.is_empty() && lines.iter().all(|l| l.ends_with('\r')) {
        for l in &mut lines {
            l.pop();
        }
    }
    lines
}

pub fn map(buf: &mut Buffer, lhs: &str, desc: &str, f: impl Fn() + 'static) {
    let desc = format!("structdiff: {desc}");
    let opts = api::opts::SetKeymapOpts::builder()
        .callback(move |_| f())
        .desc(desc.as_str())
        .nowait(true)
        .silent(true)
        .build();
    let _ = buf.set_keymap(Mode::Normal, lhs, "", &opts);
}

/// A buffer's own Normal-mode maps on some keys, saved before structdiff
/// put its maps there, and the keys it used.
#[derive(Default)]
pub struct SavedMaps {
    keys: Vec<String>,
    saved: Vec<api::types::KeymapInfos>,
}

/// Save `buf`'s buffer-local Normal-mode maps on `keys`.
pub fn save_maps(buf: &Buffer, keys: &[&str]) -> SavedMaps {
    let saved = buf
        .get_keymap(Mode::Normal)
        .map(|maps| maps.filter(|m| keys.contains(&m.lhs.as_str())).collect())
        .unwrap_or_default();
    SavedMaps { keys: keys.iter().map(|k| (*k).to_owned()).collect(), saved }
}

/// Remove structdiff's maps on the saved keys and put the buffer's own back
/// (their descriptions aren't recoverable from Neovim's API).
pub fn restore_maps(buf: &mut Buffer, maps: SavedMaps) {
    for lhs in &maps.keys {
        unmap(buf, lhs);
    }
    for m in maps.saved {
        let mut opts = api::opts::SetKeymapOpts::builder();
        opts.noremap(m.noremap).silent(m.silent).expr(m.expr).nowait(m.nowait);
        if let Some(cb) = m.callback {
            opts.callback(cb);
        }
        let _ = buf.set_keymap(Mode::Normal, &m.lhs, m.rhs.as_deref().unwrap_or(""), &opts.build());
    }
}

pub fn unmap(buf: &mut Buffer, lhs: &str) {
    let _ = buf.del_keymap(Mode::Normal, lhs);
}

/// Number of tab pages. Uses Vimscript because nvim-oxi's
/// `api::list_tabpages()` omits the `Arena*` that Neovim 0.12's
/// `nvim_list_tabpages` takes, which corrupts the heap (see compat notes in
/// lib.rs).
pub fn tab_count() -> i64 {
    api::call_function("tabpagenr", ("$",)).unwrap_or(1)
}

pub fn same_tab(win: &Window, tab: &api::TabPage) -> bool {
    win.is_valid() && win.get_tabpage().is_ok_and(|t| &t == tab)
}

/// Run `f` with `win` as the current window, without moving the user's
/// cursor there.
///
/// Errors inside `f` come back as `Err`, never through nvim-oxi:
/// `Window::call` turns an `Err` returned by its closure into a Lua error
/// raised from an `extern "C"` trampoline, which can't unwind and aborts
/// Neovim. That's why clippy.toml bans `Window::call` everywhere else.
#[allow(clippy::disallowed_methods)]
pub fn in_win<T: 'static>(win: &Window, f: impl FnOnce() -> Result<T, api::Error> + 'static) -> Result<T, api::Error> {
    use std::cell::RefCell;
    use std::rc::Rc;
    let slot: Rc<RefCell<Option<Result<T, api::Error>>>> = Rc::new(RefCell::new(None));
    let inner = slot.clone();
    win.call::<_, _, ()>(move |_| {
        *inner.borrow_mut() = Some(f());
        Ok::<_, std::convert::Infallible>(())
    })?;
    // The closure always runs when nvim_win_call succeeds. If it somehow
    // didn't, report that rather than panicking (a panic here would abort
    // Neovim too).
    slot.borrow_mut().take().unwrap_or_else(|| Err(api::Error::Other("window call did not run".into())))
}

/// Run an Ex command inside `win` without moving the cursor there.
pub fn win_cmd(win: &Window, cmd: &str) -> Result<(), api::Error> {
    let cmd = cmd.to_owned();
    in_win(win, move || api::command(&cmd))
}
