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

#[test]
fn groups_reports_grouping_other_and_dead_groups() {
    let r = repo(&[("src/a.rs", "a\n"), ("tests/a_test.rs", "t\n"), ("odd.bin", "x\n")]);
    std::fs::write(
        r.root.join("draft.json"),
        r#"{"groups":[{"name":"Tests","patterns":["^tests/"]},{"name":"Code","patterns":["\\.rs$"]},
             {"name":"Nothing","patterns":["^nope/"]}],"display_order":["Code","Tests"]}"#,
    )
    .unwrap();
    let out = structdiff(&r.root, &["groups", "--config", "draft.json"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    // display order, then the fallback with every file listed
    let code = text.find("Code (1)").unwrap();
    let tests = text.find("Tests (1)").unwrap();
    let other = text.find("Other (2)").unwrap();
    assert!(code < tests && tests < other, "{text}");
    assert!(text.contains("  draft.json") && text.contains("  odd.bin"), "{text}");
    assert!(text.contains("match nothing in this repo: Nothing"), "{text}");
    assert!(text.contains("draft.json (draft)"), "{text}");
}

#[test]
fn groups_exits_1_on_invalid_patterns_and_bad_json() {
    let r = repo(&[("a.rs", "a\n")]);
    std::fs::write(r.root.join("bad.json"), r#"{"groups":[{"name":"X","patterns":["(?=a)"]}]}"#).unwrap();
    let out = structdiff(&r.root, &["groups", "--config", "bad.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("problem: bad pattern in group \"X\""));
    std::fs::write(r.root.join("broken.json"), "{oops").unwrap();
    let out = structdiff(&r.root, &["groups", "--config", "broken.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid config broken.json"));
    for args in [&["groups", "--config"][..], &["groups", "--bogus"], &["groups", "--config", "--all"],
                 &["groups", "--all", "--all"], &["groups", "--config", "a", "--config", "b"]] {
        assert_eq!(structdiff(&r.root, args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn groups_uses_the_repos_structdiff_json_by_default() {
    let r = repo(&[("db/migrate/1.sql", "x\n"), ("a.rs", "a\n")]);
    std::fs::write(r.root.join(".structdiff.json"), r#"{"groups":[{"name":"Migrations","patterns":["^db/migrate/"]}]}"#)
        .unwrap();
    let text = String::from_utf8(structdiff(&r.root, &["groups", "--all"]).stdout).unwrap();
    assert!(text.starts_with("grouping from .structdiff.json"), "{text}");
    assert!(text.contains("Migrations (1)\n  db/migrate/1.sql"), "{text}");
}

#[test]
fn groups_fails_loudly_on_configs_that_would_not_work() {
    let r = repo(&[("a.rs", "a\n"), ("b.md", "b\n")]);
    let check = |json: &str| {
        std::fs::write(r.root.join("draft.json"), json).unwrap();
        let out = structdiff(&r.root, &["groups", "--config", "draft.json"]);
        (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
    };
    // typo'd keys are rejected, not ignored
    let (code, text) = check(r#"{"groups":[{"name":"Code","pattern":["\\.rs$"]}]}"#);
    assert_eq!(code, Some(1), "{text}");
    assert!(text.contains("unknown field `pattern`"), "{text}");
    let (code, _) = check(r#"{"groups":[{"name":"Code","patterns":["x"]}],"displayOrder":["Code"]}"#);
    assert_eq!(code, Some(1));
    // display_order naming a group that doesn't exist
    let (code, text) = check(r#"{"groups":[{"name":"Code","patterns":["x"]}],"display_order":["Sources"]}"#);
    assert_eq!(code, Some(1));
    assert!(text.contains("display_order names \"Sources\""), "{text}");
    // a group named like the fallback, and a duplicate
    let (code, text) = check(r#"{"groups":[{"name":"Other","patterns":["x"]},{"name":"A","patterns":["a"]},{"name":"A","patterns":["b"]}]}"#);
    assert_eq!(code, Some(1));
    assert!(text.contains("fallback group's name") && text.contains("defined twice"), "{text}");
    // a clean draft, without display_order, passes
    let (code, text) = check(r#"{"groups":[{"name":"Code","patterns":["\\.rs$"]}]}"#);
    assert_eq!(code, Some(0), "{text}");
    // a UTF-8 BOM is fine
    let (code, text) = check("\u{feff}{\"groups\":[{\"name\":\"Code\",\"patterns\":[\"x\"]}]}");
    assert_eq!(code, Some(0), "{text}");
}

#[test]
fn groups_reports_a_broken_repo_config_and_exits_1() {
    let r = repo(&[("a.rs", "a\n")]);
    std::fs::write(r.root.join(".structdiff.json"), r#"{"groups":[],}"#).unwrap(); // trailing comma
    let out = structdiff(&r.root, &["groups"]);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(".structdiff.json was ignored"), "{text}");
    assert!(text.lines().nth(1).unwrap().starts_with("problem: ignoring invalid"), "{text}");
}

#[test]
fn groups_lists_problems_before_files_and_caps_other() {
    let r = repo(&[("a.rs", "a\n")]);
    for i in 0..300 {
        write(&r.root, &format!("assets/{i}.bin"), "x");
    }
    std::fs::write(r.root.join("draft.json"), r#"{"groups":[{"name":"Code","patterns":["(?=x)"]}]}"#).unwrap();
    let out = structdiff(&r.root, &["groups", "--config", "draft.json"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.lines().nth(1).unwrap().starts_with("problem: bad pattern"), "{}", &text[..200]);
    assert!(text.contains("… 252 more (--all lists them)"), "{}", &text[text.len() - 300..]);
}

#[test]
fn groups_survives_a_closed_pipe() {
    let r = repo(&[("a.rs", "a\n")]);
    for i in 0..2000 {
        write(&r.root, &format!("f{i}.txt"), "x");
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_structdiff"))
        .current_dir(&r.root)
        .args(["groups", "--all"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take()); // the reader goes away, like `| head`
    let out = child.wait_with_output().unwrap();
    assert_ne!(out.status.code(), Some(101), "panicked: {}", String::from_utf8_lossy(&out.stderr));
}
