use std::path::{Path, PathBuf};
use std::process::Command;

use nvim_oxi::api::{self, Buffer, Window};

pub struct TempRepo {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn write(root: &Path, path: &str, content: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

pub fn repo(files: &[(&str, &str)]) -> TempRepo {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    for (p, c) in files {
        write(&root, p, c);
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);
    TempRepo { _dir: dir, root }
}

/// Source + tests + a deleted doc + an untracked file, against HEAD.
pub fn sample_repo() -> TempRepo {
    let r = repo(&[
        ("lua/core.lua", "local M = {}\nreturn M\n"),
        ("lua/util.lua", "return 1\n"),
        ("tests/core_spec.lua", "-- spec\n"),
        ("README.md", "# x\n"),
    ]);
    write(&r.root, "lua/core.lua", "local M = {}\nM.retry = true\nreturn M\n");
    write(&r.root, "lua/backoff.lua", "return 2\n");
    write(&r.root, "tests/core_spec.lua", "-- spec\n-- retry\n");
    std::fs::remove_file(r.root.join("README.md")).unwrap();
    r
}

/// feature branched off main's first commit; main moved on; uncommitted edit.
pub fn branch_repo() -> TempRepo {
    let r = repo(&[("a.lua", "a\n"), ("shared.lua", "s\n"), ("gone.md", "g\n")]);
    let root = &r.root;
    git(root, &["checkout", "-qb", "feature"]);
    write(root, "a.lua", "a feature\n");
    write(root, "new.lua", "n\n");
    std::fs::remove_file(root.join("gone.md")).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "f1"]);
    git(root, &["checkout", "-q", "main"]);
    write(root, "shared.lua", "s main\n");
    git(root, &["commit", "-qam", "m2"]);
    git(root, &["checkout", "-q", "feature"]);
    write(root, "a.lua", "a feature + uncommitted\n");
    r
}

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
    structdiff::with_view(|v| lines(&v.panel_buf)).expect("view open")
}

pub fn current() -> Option<String> {
    structdiff::with_view(|v| v.current.clone()).flatten()
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
        v.panel_buf
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
