use std::collections::BTreeMap;

use structdiff_core::ChangedFile;
use structdiff_core::group::{self, Compiled, Group, GroupDef};

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
fn repo_config_replaces_groups() {
    let dir = tempfile::tempdir().unwrap();
    let defaults = group::default_groups();
    assert_eq!(group::groups_for(dir.path(), &defaults), (defaults.clone(), None));
    std::fs::write(dir.path().join(".structdiff.json"), r#"{"groups":[{"name":"Migrations","patterns":["^db/"]}]}"#).unwrap();
    assert_eq!(group::groups_for(dir.path(), &defaults).0, vec![GroupDef::new("Migrations", &["^db/"])]);
    std::fs::write(dir.path().join(".structdiff.json"), "{oops").unwrap();
    let (groups, warning) = group::groups_for(dir.path(), &defaults);
    assert_eq!(groups, defaults);
    assert!(warning.unwrap().starts_with("ignoring invalid"));
}
