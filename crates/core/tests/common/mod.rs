#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

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
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn write(root: &Path, path: &str, content: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

pub fn remove(root: &Path, path: &str) {
    std::fs::remove_file(root.join(path)).unwrap();
}

/// An empty repo with no commits.
pub fn empty_repo() -> TempRepo {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    TempRepo { _dir: dir, root }
}

/// A repo with one commit containing `files`, on branch main.
pub fn repo(files: &[(&str, &str)]) -> TempRepo {
    let r = empty_repo();
    for (p, c) in files {
        write(&r.root, p, c);
    }
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "init"]);
    r
}

/// main: init -> m2 (touches shared.lua). feature branches off init and adds
/// f1 (edits a.lua, adds new.lua, deletes gone.md). Checked out on feature
/// with an uncommitted edit and an untracked file.
pub fn branch_repo() -> TempRepo {
    let r = repo(&[("a.lua", "a\n"), ("shared.lua", "s\n"), ("gone.md", "g\n")]);
    let root = &r.root;
    git(root, &["checkout", "-qb", "feature"]);
    write(root, "a.lua", "a feature\n");
    write(root, "new.lua", "n\n");
    remove(root, "gone.md");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "f1"]);
    git(root, &["checkout", "-q", "main"]);
    write(root, "shared.lua", "s main\n");
    git(root, &["commit", "-qam", "m2"]);
    git(root, &["checkout", "-q", "feature"]);
    write(root, "a.lua", "a feature + uncommitted\n");
    write(root, "scratch.txt", "x\n");
    r
}

pub fn summary(files: &[structdiff_core::ChangedFile]) -> Vec<String> {
    files.iter().map(|f| format!("{} {}", f.status, f.path)).collect()
}
