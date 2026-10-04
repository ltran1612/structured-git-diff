use std::path::Path;
use std::process::{Command, Output};

use structdiff_testutil::{repo, write};

fn structdiff(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_structdiff")).current_dir(dir).args(args).output().unwrap()
}

#[test]
fn export_writes_groups_json_matching_the_viewer() {
    let r = repo(&[("a.lua", "a\n")]);
    let root = r.root.clone();
    write(&root, "a.lua", "a2\n");

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
    let r = repo(&[("a.lua", "a\n")]);
    let root = r.root.clone();
    write(&root, "a.lua", "a2\n");
    // As if the Neovim plugin exported with a custom grouping.
    let custom = structdiff_core::Grouping {
        groups: vec![structdiff_core::GroupDef::new("Lua", &[r"\.lua$"])],
        other: "Rest".into(),
        display: Vec::new(),
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

#[test]
fn export_does_not_hang_on_thousands_of_untracked_files() {
    let r = repo(&[("a.lua", "a\n")]);
    for i in 0..5000 {
        write(&r.root, &format!("generated/output/module_{i:05}/artifact.txt"), &format!("{i}\n"));
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_structdiff"))
        .current_dir(&r.root)
        .arg("export")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            panic!("structdiff export hung on 5000 untracked files");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(status.success());
}
