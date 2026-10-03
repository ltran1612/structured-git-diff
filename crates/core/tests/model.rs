mod common;

use common::*;
use structdiff_core::{Model, Repo, State, group, narrative};

fn load(root: &std::path::Path, spec: &str) -> Model {
    let (m, warnings) = Model::load(Repo::discover(root).unwrap(), spec, &group::default_groups(), "Other").unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    m
}

#[test]
fn load_groups_exports_and_excludes_its_own_files() {
    let r = repo(&[("lua/core.lua", "x\n"), ("README.md", "# x\n")]);
    write(&r.root, "lua/core.lua", "y\n");
    write(&r.root, "lua/backoff.lua", "z\n");
    let m = load(&r.root, "");
    let exported: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(r.root.join(".structdiff/groups.json")).unwrap()).unwrap();
    assert_eq!(exported["fingerprint"], m.fingerprint.as_str());
    assert_eq!(exported["range"]["target"], serde_json::Value::Null);
    assert_eq!(exported["output"], ".structdiff/narrative.json");
    assert_eq!(exported["groups"][0]["name"], "Source");
    // .structdiff/ never shows up as a change, even after a rescan
    let m = load(&r.root, "");
    assert_eq!(summary(&m.files), ["? lua/backoff.lua", "M lua/core.lua"]);
    assert_eq!(m.state, State::None);
}

#[test]
fn narrative_order_regroups_and_staleness_tracks_the_diff() {
    let r = repo(&[("lua/core.lua", "x\n"), ("tests/core_spec.lua", "t\n")]);
    write(&r.root, "lua/core.lua", "y\n");
    write(&r.root, "tests/core_spec.lua", "t2\n");
    let mut m = load(&r.root, "");
    assert_eq!(m.groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Tests", "Source"]);
    std::fs::write(
        narrative::path(&r.root, ""),
        serde_json::json!({"version":1,"fingerprint":m.fingerprint,"order":["lua/core.lua","tests/core_spec.lua"]}).to_string(),
    )
    .unwrap();
    m.reload_narrative();
    assert_eq!(m.state, State::Fresh);
    assert_eq!(m.groups[0].name, "Source");
    assert_eq!(m.index_of("tests/core_spec.lua"), Some(1));
    write(&r.root, "lua/core.lua", "changed again\n");
    m.rescan().unwrap();
    assert_eq!(m.state, State::Stale);
}

#[test]
fn branch_range_has_its_own_narrative_and_ignores_worktree_edits() {
    let r = branch_repo();
    let mut m = load(&r.root, "main...HEAD");
    assert_eq!(m.narrative_filename(), "narrative-main...HEAD.json");
    std::fs::write(narrative::path(&r.root, ""), r#"{"overall":"worktree story"}"#).unwrap();
    m.reload_narrative();
    assert_eq!(m.state, State::None);
    std::fs::write(
        narrative::path(&r.root, "main...HEAD"),
        serde_json::json!({"fingerprint": m.fingerprint, "overall": "branch story"}).to_string(),
    )
    .unwrap();
    m.reload_narrative();
    assert_eq!(m.state, State::Fresh);
    write(&r.root, "a.lua", "edited again\n");
    m.rescan().unwrap();
    assert_eq!(m.state, State::Fresh);
}

#[test]
fn unknown_range_fails_to_load() {
    let r = repo(&[("a", "a\n")]);
    let err = Model::load(Repo::discover(&r.root).unwrap(), "nope...HEAD", &[], "Other").err().unwrap();
    assert_eq!(err, "unknown revision: nope");
}
