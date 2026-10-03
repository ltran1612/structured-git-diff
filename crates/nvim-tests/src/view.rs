use nvim_oxi::api::{self, opts::OptionOpts};
use structdiff_core::{State, narrative};

use crate::helpers::*;

fn group_names() -> Vec<String> {
    structdiff::with_view(|v| v.model.groups.iter().map(|g| g.name.clone()).collect()).unwrap()
}

fn diff_on(win: &api::Window) -> bool {
    api::get_option_value::<bool>("diff", &OptionOpts::builder().win(win.clone()).build()).unwrap()
}

#[nvim_oxi::test]
fn view_groups_files_and_navigates_across_groups() {
    let r = sample_repo();
    start(&r.root);
    structdiff::open(None);
    assert_eq!(group_names(), ["Tests", "Docs", "Source"]);
    let p = panel();
    assert_eq!(p[0], " StructDiff  4 files");
    assert_eq!(p[1], " HEAD → working tree");
    assert!(p.contains(&"▾ Source (2)".into()), "{p:?}");
    assert!(p.contains(&"  ? backoff.lua  lua".into()), "{p:?}");
    assert_eq!(current().as_deref(), Some("tests/core_spec.lua"));
    let (left, right, sidebar) = structdiff::with_view(|v| (v.left.clone(), v.right.clone(), v.sidebar.clone())).unwrap();
    assert!(diff_on(&left) && diff_on(&right));
    assert!(name(&right).ends_with("/tests/core_spec.lua"));

    structdiff::goto_group(1);
    assert_eq!(current().as_deref(), Some("README.md")); // deleted: empty right side
    assert_eq!(win_lines(&right), [""]);
    assert_eq!(win_lines(&left), ["# x"]);

    structdiff::goto_file(1);
    assert_eq!(current().as_deref(), Some("lua/backoff.lua")); // untracked: empty left side
    assert_eq!(win_lines(&left), [""]);
    structdiff::goto_file(1);
    assert_eq!(current().as_deref(), Some("lua/core.lua"));
    assert_eq!(name(&left), "structdiff://HEAD/lua/core.lua");
    structdiff::goto_file(1); // past the end: stays put
    assert_eq!(current().as_deref(), Some("lua/core.lua"));
    structdiff::goto_group(-1);
    assert_eq!(current().as_deref(), Some("README.md"));

    // the sidebar cursor follows the current file
    let (line, _) = sidebar.get_cursor().unwrap();
    assert_eq!(panel()[line - 1], "  D README.md");
    structdiff::close();
}

#[nvim_oxi::test]
fn narrative_feeds_reasons_order_split_and_staleness() {
    let r = sample_repo();
    start(&r.root);
    structdiff::open(None);
    let fp = structdiff::with_view(|v| v.model.fingerprint.clone()).unwrap();
    std::fs::write(
        narrative::path(&r.root, ""),
        serde_json::json!({
            "version": 1, "fingerprint": fp, "overall": "Add retry.",
            "groups": {"Source": "Retry needs a backoff helper."},
            "files": {"lua/core.lua": "turns retry on", "lua/backoff.lua": "new helper"},
            "order": ["lua/core.lua", "lua/backoff.lua"]
        })
        .to_string(),
    )
    .unwrap();
    structdiff::reload_narrative();
    assert_eq!(structdiff::with_view(|v| v.model.state), Some(State::Fresh));
    assert_eq!(group_names(), ["Source", "Tests", "Docs"]);
    let virt = virt_texts();
    assert!(virt.contains(&"turns retry on".into()), "{virt:?}");
    assert!(virt.contains(&"Retry needs a backoff helper.".into()), "{virt:?}");
    assert_eq!(panel()[2], " narrative up to date");

    structdiff::toggle_narrative();
    let (open, nlines) = structdiff::with_view(|v| (v.narrative_open(), lines(v.narrative_buf.as_ref().unwrap()))).unwrap();
    assert!(open);
    assert!(nlines.contains(&"Add retry.".into()), "{nlines:?}");
    structdiff::toggle_narrative();
    assert_eq!(structdiff::with_view(|v| v.narrative_open()), Some(false));

    write(&r.root, "lua/util.lua", "return 3\n");
    structdiff::refresh();
    assert_eq!(structdiff::with_view(|v| v.model.state), Some(State::Stale));
    assert!(panel()[2].contains("stale"), "{}", panel()[2]);
    structdiff::close();
}

#[nvim_oxi::test]
fn watcher_reloads_when_the_narrative_file_is_written() {
    let r = sample_repo();
    start(&r.root);
    structdiff::open(None);
    let fp = structdiff::with_view(|v| v.model.fingerprint.clone()).unwrap();
    std::fs::write(narrative::path(&r.root, ""), serde_json::json!({"fingerprint": fp, "overall": "x"}).to_string()).unwrap();
    assert!(wait_until(3000, || structdiff::with_view(|v| v.model.state) == Some(State::Fresh)));
    structdiff::close();
}

#[nvim_oxi::test]
fn generate_runs_the_command_in_the_background_and_reloads() {
    let r = sample_repo();
    start(&r.root);
    // Stand-in for claude: echo the range it was given into the narrative.
    let script = r#"printf '{"overall":"generated for [%s]"}' "$1" > .structdiff/narrative.json"#;
    let opts = api::call_function::<_, nvim_oxi::Object>(
        "luaeval",
        ("{ generate_cmd = { 'sh', '-c', _A, 'sh', '{range}' } }", script),
    )
    .unwrap();
    structdiff::setup(opts);
    structdiff::open(None);
    structdiff::generate(None);
    assert_eq!(structdiff::with_view(|v| v.generating), Some(true));
    assert_eq!(panel()[2], " generating narrative…");
    assert!(wait_until(5000, || structdiff::with_view(|v| !v.generating).unwrap_or(false)));
    let overall = structdiff::with_view(|v| v.model.narrative.as_ref().map(|n| n.overall.clone())).flatten();
    assert_eq!(overall.as_deref(), Some("generated for []"));
    assert_eq!(structdiff::with_view(|v| v.narrative_open()), Some(true));
    structdiff::close();
}

#[nvim_oxi::test]
fn branch_range_uses_read_only_revisions_and_its_own_narrative() {
    let r = branch_repo();
    start(&r.root);
    structdiff::open(Some("main...HEAD".into()));
    assert_eq!(panel()[1], " main...HEAD");
    let flat: Vec<String> = structdiff::with_view(|v| v.flat().iter().map(|&p| v.model.file(p).path.clone()).collect()).unwrap();
    assert_eq!(flat, ["gone.md", "a.lua", "new.lua"]);
    structdiff::goto_file(1);
    let (left, right, real) = structdiff::with_view(|v| (v.left.clone(), v.right.clone(), v.real_buf.is_some())).unwrap();
    assert_eq!(name(&right), "structdiff://HEAD/a.lua");
    assert_eq!(win_lines(&right), ["a feature"]); // committed, not the worktree edit
    assert_eq!(name(&left), "structdiff://merge-base(main)/a.lua");
    assert_eq!(win_lines(&left), ["a"]);
    assert!(!real);
    let modifiable: bool =
        api::get_option_value("modifiable", &OptionOpts::builder().buf(right.get_buf().unwrap()).build()).unwrap();
    assert!(!modifiable);
    let ft: String = api::get_option_value("filetype", &OptionOpts::builder().buf(right.get_buf().unwrap()).build()).unwrap();
    assert_eq!(ft, "lua");

    // switching range replaces the view
    structdiff::open(None);
    assert_eq!(panel()[1], " HEAD → working tree");
    structdiff::close();
}

#[nvim_oxi::test]
fn close_removes_the_tab_and_real_buffer_keymaps() {
    let r = sample_repo();
    start(&r.root);
    let tabs = structdiff::ui::tab_count();
    structdiff::open(None);
    assert_eq!(structdiff::ui::tab_count(), tabs + 1);
    structdiff::goto_group(2); // lua/backoff.lua, a real buffer
    let buf = structdiff::with_view(|v| v.real_buf.clone()).flatten().expect("real buffer");
    let ours = |b: &api::Buffer| {
        b.get_keymap(api::types::Mode::Normal)
            .unwrap()
            .filter(|m| ["]g", "[g", "]f", "[f", "q", "R", "gn", "gr"].contains(&m.lhs.as_str()) && m.callback.is_some())
            .count()
    };
    assert_eq!(ours(&buf), 4); // navigation only; q / R / gn untouched on real files
    structdiff::close();
    assert!(structdiff::with_view(|_| ()).is_none());
    assert_eq!(structdiff::ui::tab_count(), tabs);
    assert_eq!(ours(&buf), 0);
}

#[nvim_oxi::test]
fn layout_is_rebuilt_when_a_diff_window_is_closed() {
    let r = sample_repo();
    start(&r.root);
    structdiff::open(None);
    let left = structdiff::with_view(|v| v.left.clone()).unwrap();
    left.close(true).unwrap();
    structdiff::goto_file(1);
    let (left, sidebar) = structdiff::with_view(|v| (v.left.clone(), v.sidebar.clone())).unwrap();
    assert!(left.is_valid() && sidebar.is_valid());
    assert!(diff_on(&left));
    structdiff::close();
}

#[nvim_oxi::test]
fn commands_and_completion_are_registered() {
    let r = branch_repo();
    start(&r.root);
    let got: Vec<String> = api::call_function("getcompletion", ("StructDiff main...fe", "cmdline")).unwrap();
    assert_eq!(got, ["main...feature"]);
    api::command("StructDiff main...").unwrap();
    assert_eq!(panel()[1], " main...");
    api::command("StructDiffClose").unwrap();
    assert!(structdiff::with_view(|_| ()).is_none());
}

#[nvim_oxi::test]
fn unknown_revision_opens_nothing() {
    let r = sample_repo();
    start(&r.root);
    let tabs = structdiff::ui::tab_count();
    structdiff::open(Some("nope...HEAD".into()));
    assert!(structdiff::with_view(|_| ()).is_none());
    assert_eq!(structdiff::ui::tab_count(), tabs);
}

#[nvim_oxi::test]
fn user_closing_the_tab_cleans_up() {
    let r = sample_repo();
    start(&r.root);
    structdiff::open(None);
    api::command("tabclose").unwrap();
    assert!(wait_until(1000, || structdiff::with_view(|_| ()).is_none()));
}

#[nvim_oxi::test]
fn setup_merges_partial_options_and_rejects_bad_ones() {
    use structdiff::config::Keys;
    let lua = |expr: &str| api::call_function::<_, nvim_oxi::Object>("luaeval", (expr,)).unwrap();
    structdiff::setup(lua("{}"));
    assert_eq!(structdiff::config().sidebar_width, 40);
    structdiff::setup(lua("{ sidebar_width = 30, keymaps = { close = 'x' } }"));
    let cfg = structdiff::config();
    assert_eq!(cfg.sidebar_width, 30);
    assert!(matches!(&cfg.keymaps.close, Keys::One(k) if k == "x"));
    assert!(matches!(&cfg.keymaps.next_group, Keys::One(k) if k == "]g")); // untouched keys keep defaults
    assert_eq!(cfg.groups.len(), 5);
    structdiff::setup(lua("{ groups = { { name = 'Only', patterns = { '.' } } } }"));
    assert_eq!(structdiff::config().groups.len(), 1); // lists replace
    structdiff::setup(lua("{ sidebar_width = 'wide' }"));
    assert_eq!(structdiff::config().groups.len(), 1); // bad options are reported, config unchanged
}
