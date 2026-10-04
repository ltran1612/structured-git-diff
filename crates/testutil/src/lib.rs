//! Throwaway git repositories for structdiff's tests. Shared by the core, CLI
//! and in-Neovim test suites. Deliberately independent of structdiff-core,
//! so depending on it never creates a cycle.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A git repository in a temporary directory, deleted on drop.
pub struct TempRepo {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
}

/// Run git in `dir` with a fixed identity and default branch; panics on failure.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false"])
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

/// Working-tree changes against HEAD across groups: Source edits plus an
/// untracked file, a test edit, and a deleted doc.
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
    remove(&r.root, "README.md");
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
