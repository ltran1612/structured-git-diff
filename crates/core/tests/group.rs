use std::collections::BTreeMap;

use structdiff_core::ChangedFile;
use structdiff_core::group::{self, Compiled, Group, GroupDef, Grouping};

fn files(paths: &[&str]) -> Vec<ChangedFile> {
    paths.iter().map(|p| ChangedFile::new('M', p)).collect()
}

fn names(groups: &[Group]) -> BTreeMap<String, Vec<String>> {
    groups.iter().map(|g| (g.name.clone(), g.files.iter().map(|f| f.path.clone()).collect())).collect()
}

fn order(groups: &[Group]) -> Vec<&str> {
    groups.iter().map(|g| g.name.as_str()).collect()
}

#[test]
fn default_groups_classify_common_paths() {
    let (compiled, warnings) = Compiled::new(&group::default_groups());
    assert!(warnings.is_empty(), "{warnings:?}");
    let g = group::assign(
        &files(&[
            "lua/foo.lua", "tests/foo_spec.lua", "src/x_test.go", "README.md", "package.json",
            ".github/workflows/ci.yml", "assets/logo.png", "docs/guide.txt", "requirements-dev.txt", "Cargo.lock",
        ]),
        &compiled,
        "Other",
    );
    let expect: BTreeMap<String, Vec<String>> = [
        ("Source", vec!["lua/foo.lua"]),
        ("Tests", vec!["tests/foo_spec.lua", "src/x_test.go"]),
        ("Docs", vec!["README.md", "docs/guide.txt"]),
        ("Config/Build", vec!["package.json", "requirements-dev.txt", "Cargo.lock"]),
        ("CI", vec![".github/workflows/ci.yml"]),
        ("Other", vec!["assets/logo.png"]),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.into_iter().map(str::to_owned).collect()))
    .collect();
    assert_eq!(names(&g), expect);
    assert_eq!(order(&g), ["Tests", "CI", "Config/Build", "Docs", "Source", "Other"]);
}

#[test]
fn first_matching_group_wins_and_empty_groups_are_dropped() {
    let defs = [GroupDef::new("API", &["^api/"]), GroupDef::new("Lua", &[r"\.lua$"]), GroupDef::new("Empty", &["^nothing/"])];
    let (compiled, _) = Compiled::new(&defs);
    let g = group::assign(&files(&["api/a.lua", "b.lua", "c.txt"]), &compiled, "Rest");
    assert_eq!(order(&g), ["API", "Lua", "Rest"]);
    assert_eq!(g[0].files[0].path, "api/a.lua");
}

#[test]
fn invalid_patterns_are_skipped_and_reported() {
    let (compiled, warnings) = Compiled::new(&[GroupDef::new("Bad", &["("])]);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("bad pattern in group \"Bad\": ("), "{}", warnings[0]);
    assert_eq!(order(&group::assign(&files(&["a.lua"]), &compiled, "Other")), ["Other"]);
}

#[test]
fn sort_by_order_is_stable_for_unlisted_files() {
    let mut g = vec![Group { name: "S".into(), files: files(&["a", "b", "c", "d"]) }];
    group::sort_by_order(&mut g, &["c".into(), "a".into()]);
    assert_eq!(names(&g)["S"], ["c", "a", "b", "d"]);
}

#[test]
fn sort_by_order_puts_the_group_holding_the_first_file_first() {
    let mut g: Vec<Group> = ["Tests", "Docs", "Source", "Other"]
        .iter()
        .map(|n| Group { name: (*n).into(), files: files(&[&n.to_lowercase()]) })
        .collect();
    group::sort_by_order(&mut g, &["source".into(), "tests".into()]);
    assert_eq!(order(&g), ["Source", "Tests", "Docs", "Other"]);
}

#[test]
fn repo_config_replaces_groups_but_keeps_the_fallback_name() {
    let dir = tempfile::tempdir().unwrap();
    let configured = Grouping { other: "Misc".into(), ..Grouping::default() };
    assert_eq!(configured.for_repo(dir.path()), (configured.clone(), None));
    std::fs::write(dir.path().join(".structdiff.json"), r#"{"groups":[{"name":"Migrations","patterns":["^db/"]}]}"#).unwrap();
    let (repo, _) = configured.for_repo(dir.path());
    assert_eq!(repo.groups, vec![GroupDef::new("Migrations", &["^db/"])]);
    assert_eq!(repo.other, "Misc");
    std::fs::write(dir.path().join(".structdiff.json"), "{oops").unwrap();
    let (fallback, warning) = configured.for_repo(dir.path());
    assert_eq!(fallback, configured);
    assert!(warning.unwrap().starts_with("ignoring invalid"));
}

#[test]
fn display_order_is_separate_from_match_order() {
    let mut g: Vec<Group> = ["Tests", "CI", "Docs", "Source", "Other", "Assets"]
        .iter()
        .map(|n| Group { name: (*n).into(), files: files(&[&n.to_lowercase()]) })
        .collect();
    // Assets is unlisted: after the listed groups, before the fallback.
    group::sort_display(&mut g, &["Source".into(), "Tests".into(), "Docs".into(), "CI".into()], "Other");
    assert_eq!(order(&g), ["Source", "Tests", "Docs", "CI", "Assets", "Other"]);
    // Naming the fallback places it explicitly.
    group::sort_display(&mut g, &["Other".into(), "Source".into()], "Other");
    assert_eq!(order(&g), ["Other", "Source", "Tests", "Docs", "CI", "Assets"]);
}

#[test]
fn repo_config_can_set_the_display_order() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".structdiff.json"),
        r#"{"groups":[{"name":"A","patterns":["a"]},{"name":"B","patterns":["b"]}],"display_order":["B","A"]}"#,
    )
    .unwrap();
    let (g, _) = Grouping::default().for_repo(dir.path());
    assert_eq!(g.display, ["B", "A"]);
    // Without display_order, the configured one is kept, minus names this
    // config doesn't define.
    std::fs::write(dir.path().join(".structdiff.json"), r#"{"groups":[{"name":"Tests","patterns":["t"]},{"name":"A","patterns":["a"]}]}"#).unwrap();
    assert_eq!(Grouping::default().for_repo(dir.path()).0.display, ["Tests"]);
}

#[test]
fn groups_json_without_display_order_still_parses() {
    let g: Grouping = serde_json::from_str(r#"{"groups":[],"other":"Other"}"#).unwrap();
    assert_eq!(g.display, group::default_display_order());
}

#[test]
fn with_repo_config_replaces_groups_and_optionally_display() {
    let base = Grouping { other: "Misc".into(), ..Grouping::default() };
    let g = base.with_repo_config(r#"{"groups":[{"name":"A","patterns":["a"]}],"display_order":["A"]}"#).unwrap();
    assert_eq!(g.groups.len(), 1);
    assert_eq!(g.display, ["A"]);
    assert_eq!(g.other, "Misc");
    // without display_order, inherited names this config doesn't define drop out
    let g = base.with_repo_config(r#"{"groups":[]}"#).unwrap();
    assert!(g.display.is_empty());
    assert!(base.with_repo_config("{").is_err());
}

#[test]
fn problems_and_strict_parsing() {
    let base = Grouping::default();
    assert!(base.problems().is_empty(), "{:?}", base.problems());
    assert!(base.with_repo_config(r#"{"groups":[{"name":"A","pattern":["a"]}]}"#).is_err());
    assert!(base.with_repo_config(r#"{"groups":[],"other":"X"}"#).is_err());
    // an inherited display order only keeps names this config defines
    let g = base.with_repo_config(r#"{"groups":[{"name":"Tests","patterns":["t"]},{"name":"Mine","patterns":["m"]}]}"#).unwrap();
    assert_eq!(g.display, ["Tests"]);
    assert!(g.problems().is_empty());
}

#[test]
fn an_unreadable_repo_config_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".structdiff.json")).unwrap(); // a directory, not a file
    let (_, warning) = Grouping::default().for_repo(dir.path());
    assert!(warning.unwrap().contains("cannot read it"));
}

#[test]
fn sort_by_order_is_fast_on_big_change_sets() {
    let paths: Vec<String> = (0..10_000).map(|i| format!("src/file_{i:05}.rs")).collect();
    let files: Vec<ChangedFile> = paths.iter().map(|p| ChangedFile::new('M', p)).collect();
    let mut g = vec![Group { name: "Source".into(), files }];
    let order: Vec<String> = paths.iter().rev().cloned().collect();
    let started = std::time::Instant::now();
    group::sort_by_order(&mut g, &order);
    assert!(started.elapsed() < std::time::Duration::from_millis(500), "{:?}", started.elapsed());
    assert_eq!(g[0].files[0].path, "src/file_09999.rs");
}
