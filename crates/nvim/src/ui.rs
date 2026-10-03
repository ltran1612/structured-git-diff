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

pub fn to_lines(bytes: &[u8]) -> Vec<String> {
    if is_binary(bytes) {
        return vec!["[binary file]".into()];
    }
    let text = String::from_utf8_lossy(bytes);
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
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

/// Run an Ex command inside `win` without moving the cursor there.
pub fn win_cmd(win: &Window, cmd: &str) -> Result<(), api::Error> {
    let cmd = cmd.to_owned();
    win.call::<_, _, ()>(move |_| api::command(&cmd))
}
