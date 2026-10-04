use structdiff_core::narrative::{self, Narrative, State};
use structdiff_core::{ChangedFile, Error, Group};

#[test]
fn load_absent_invalid_and_normalized() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(matches!(Narrative::load(root, ""), Ok(None)));
    std::fs::create_dir_all(root.join(".structdiff")).unwrap();
    std::fs::write(narrative::path(root, ""), "{not json").unwrap();
    let err = Narrative::load(root, "").unwrap_err();
    assert!(matches!(err, Error::InvalidNarrative { reason: "invalid JSON", .. }), "{err:?}");
    assert!(err.to_string().starts_with("invalid JSON in "), "{err}");
    std::fs::write(
        narrative::path(root, ""),
        r#"{"version":1,"fingerprint":"abc","overall":"story","groups":{"Source":"why","Bad":3},
            "files":{"a.lua":"reason"},"order":["a.lua",7],"extra":true}"#,
    )
    .unwrap();
    let n = Narrative::load(root, "").unwrap().unwrap();
    assert_eq!(n.fingerprint.as_deref(), Some("abc"));
    assert_eq!(n.overall, "story");
    assert_eq!(n.groups.len(), 1);
    assert_eq!(n.files["a.lua"], "reason");
    assert_eq!(n.order, ["a.lua"]);
    std::fs::write(narrative::path(root, ""), "[1,2]").unwrap();
    assert!(matches!(Narrative::load(root, ""), Err(Error::InvalidNarrative { reason: "not a JSON object", .. })));
}

#[test]
fn state() {
    let with = |fp: Option<&str>| Narrative { fingerprint: fp.map(str::to_owned), ..Default::default() };
    assert_eq!(narrative::state(None, "x"), State::None);
    assert_eq!(narrative::state(Some(&with(None)), "x"), State::Unverified);
    assert_eq!(narrative::state(Some(&with(Some("x"))), "x"), State::Fresh);
    assert_eq!(narrative::state(Some(&with(Some("y"))), "x"), State::Stale);
}

#[test]
fn ensure_excluded_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let ex = dir.path().join("info/exclude");
    narrative::ensure_excluded(&ex).unwrap();
    narrative::ensure_excluded(&ex).unwrap();
    assert_eq!(std::fs::read_to_string(&ex).unwrap().matches("/.structdiff/").count(), 1);
}

#[test]
fn filename_per_range() {
    assert_eq!(narrative::filename(""), "narrative.json");
    assert_eq!(narrative::filename("main...HEAD"), "narrative-main...HEAD.json");
    assert_eq!(narrative::filename("origin/main..HEAD~1"), "narrative-origin_main..HEAD_1.json");
}

#[test]
fn render_includes_overall_group_why_and_file_reasons() {
    let n = Narrative {
        overall: "Big picture.".into(),
        groups: [("Source".to_owned(), "Because.".to_owned())].into(),
        files: [("a.lua".to_owned(), "adds x".to_owned())].into(),
        ..Default::default()
    };
    let groups = [Group { name: "Source".into(), files: vec![ChangedFile::new('M', "a.lua"), ChangedFile::new('M', "b.lua")] }];
    let text = narrative::render(Some(&n), &groups, State::Stale, "main...HEAD").join("\n");
    assert!(text.starts_with("# Change narrative: main...HEAD"));
    assert!(text.contains("Stale"));
    assert!(text.contains("Big picture."));
    assert!(text.contains("## Source\n\nBecause."));
    assert!(text.contains("- `a.lua` — adds x"));
    assert!(text.ends_with("- `b.lua`"));
    let none = narrative::render(None, &groups, State::None, "").join("\n");
    assert!(none.contains("Ask your agent to run `/diff-narrative`."), "{none}");
}

#[test]
fn wrap() {
    assert_eq!(narrative::wrap("one two three four", 9), ["one two", "three", "four"]);
    assert_eq!(narrative::wrap("  ", 9), Vec::<String>::new());
    assert_eq!(narrative::wrap("supercalifragilistic word", 5), ["supercalifragilistic", "word"]);
}

#[test]
fn equivalent_range_spellings_share_a_narrative() {
    assert_eq!(narrative::filename("main..."), narrative::filename("main...HEAD"));
    assert_eq!(narrative::filename("..main"), "narrative-HEAD..main.json");
    assert_eq!(narrative::filename("main.."), "narrative-main..HEAD.json");
    assert_eq!(narrative::filename(" main "), "narrative-main.json");
    assert_eq!(narrative::canonical_spec("a...b"), "a...b");
}
