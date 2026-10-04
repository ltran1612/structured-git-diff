use std::path::Path;
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn structdiff(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_structdiff")).current_dir(dir).args(args).output().unwrap()
}

#[test]
fn export_writes_groups_json_matching_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("a.lua"), "a\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);
    std::fs::write(root.join("a.lua"), "a2\n").unwrap();

    let out = structdiff(&root, &["export"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let path = String::from_utf8(out.stdout).unwrap();
    assert_eq!(path.trim(), root.join(".structdiff/groups.json").to_str().unwrap());

    let exported: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path.trim()).unwrap()).unwrap();
    let repo = structdiff_core::Repo::discover(&root).unwrap();
    let (model, _) = structdiff_core::Model::load(repo, "", &structdiff_core::Grouping::default()).unwrap();
    assert_eq!(exported["fingerprint"], model.fingerprint.as_str());
    assert_eq!(exported["groups"][0]["files"][0]["path"], "a.lua");
}

#[test]
fn export_reuses_the_grouping_from_the_last_export() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-q"]);
    std::fs::write(root.join("a.lua"), "a\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);
    std::fs::write(root.join("a.lua"), "a2\n").unwrap();
    // As if the Neovim plugin exported with a custom grouping.
    let custom = structdiff_core::Grouping {
        groups: vec![structdiff_core::GroupDef::new("Lua", &[r"\.lua$"])],
        other: "Rest".into(),
    };
    let repo = structdiff_core::Repo::discover(&root).unwrap();
    let (model, _) = structdiff_core::Model::load(repo, "", &custom).unwrap();
    model.export(&custom).unwrap();

    assert!(structdiff(&root, &["export"]).status.success());
    let exported: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".structdiff/groups.json")).unwrap()).unwrap();
    assert_eq!(exported["groups"][0]["name"], "Lua");
}

#[test]
fn errors_and_usage() {
    let dir = tempfile::tempdir().unwrap();
    let out = structdiff(dir.path(), &["export"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a git repository"));
    assert_eq!(structdiff(dir.path(), &["bogus"]).status.code(), Some(2));
    assert!(structdiff(dir.path(), &["--help"]).status.success());
}
