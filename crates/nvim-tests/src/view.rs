use nvim_oxi::api::{self, opts::OptionOpts};
use structdiff_core::{State, narrative};

use crate::helpers::*;

fn group_names() -> Vec<String> {
    structdiff::with_view(|v| v.model().groups.iter().map(|g| g.name.clone()).collect()).unwrap()
}

fn diff_on(win: &api::Window) -> bool {
    api::get_option_value::<bool>("diff", &OptionOpts::builder().win(win.clone()).build()).unwrap()
}

#[nvim_oxi::test]
fn view_groups_files_and_navigates_across_groups() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    // display order: Source, Tests, Docs (match order puts Tests first)
    assert_eq!(group_names(), ["Source", "Tests", "Docs"]);
    let p = panel();
    assert_eq!(p[0], " StructDiff  4 files");
    assert_eq!(p[1], " HEAD → working tree");
    assert!(p.contains(&"▾ Source (2)".into()), "{p:?}");
    assert!(p.contains(&"  ? backoff.lua  lua".into()), "{p:?}");
    assert_eq!(current().as_deref(), Some("lua/backoff.lua"));
    let (left, right, sidebar) = structdiff::with_view(|v| (v.left().clone(), v.right().clone(), v.sidebar().clone())).unwrap();
    assert!(diff_on(&left) && diff_on(&right));
    assert!(name(&right).ends_with("/lua/backoff.lua"));
    assert_eq!(win_lines(&left), [""]); // untracked: empty left side

    structdiff::goto_file(1);
    assert_eq!(current().as_deref(), Some("lua/core.lua"));
    assert_eq!(name(&left), "structdiff://HEAD/lua/core.lua");

    structdiff::goto_group(1);
    assert_eq!(current().as_deref(), Some("tests/core_spec.lua"));
    structdiff::goto_group(1);
    assert_eq!(current().as_deref(), Some("README.md")); // deleted: empty right side
    assert_eq!(win_lines(&right), [""]);
    assert_eq!(win_lines(&left), ["# x"]);
    structdiff::goto_file(1); // past the end: stays put
    assert_eq!(current().as_deref(), Some("README.md"));
    structdiff::goto_group(-1);
    assert_eq!(current().as_deref(), Some("tests/core_spec.lua"));

    // the sidebar cursor follows the current file
    let (line, _) = sidebar.get_cursor().unwrap();
    assert_eq!(panel()[line - 1], "  M core_spec.lua  tests");
    structdiff::close();
}

#[nvim_oxi::test]
fn narrative_feeds_reasons_order_split_and_staleness() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    let fp = structdiff::with_view(|v| v.model().fingerprint.clone()).unwrap();
    std::fs::create_dir_all(narrative::dir(&r.root)).unwrap();
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
    assert_eq!(structdiff::with_view(|v| v.model().state), Some(State::Fresh));
    assert_eq!(group_names(), ["Source", "Tests", "Docs"]);
    let virt = virt_texts();
    assert!(virt.contains(&"turns retry on".into()), "{virt:?}");
    assert!(virt.contains(&"Retry needs a backoff helper.".into()), "{virt:?}");
    assert_eq!(panel()[2], " narrative up to date");

    structdiff::toggle_narrative();
    let (open, nlines) = structdiff::with_view(|v| (v.narrative_open(), lines(v.narrative_buf().unwrap()))).unwrap();
    assert!(open);
    assert!(nlines.contains(&"Add retry.".into()), "{nlines:?}");
    structdiff::toggle_narrative();
    assert_eq!(structdiff::with_view(|v| v.narrative_open()), Some(false));

    write(&r.root, "lua/util.lua", "return 3\n");
    refresh_wait();
    assert_eq!(structdiff::with_view(|v| v.model().state), Some(State::Stale));
    assert!(panel()[2].contains("stale"), "{}", panel()[2]);
    structdiff::close();
}

#[nvim_oxi::test]
fn watcher_reloads_when_the_narrative_file_is_written() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    let fp = structdiff::with_view(|v| v.model().fingerprint.clone()).unwrap();
    std::fs::create_dir_all(narrative::dir(&r.root)).unwrap();
    std::fs::write(narrative::path(&r.root, ""), serde_json::json!({"fingerprint": fp, "overall": "x"}).to_string()).unwrap();
    assert!(wait_until(3000, || structdiff::with_view(|v| v.model().state) == Some(State::Fresh)));
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
    open_wait(None);
    structdiff::generate(None);
    assert_eq!(structdiff::with_view(|v| v.generating()), Some(true));
    assert_eq!(panel()[2], " generating narrative…");
    assert!(wait_until(5000, || {
        structdiff::with_view(|v| !v.generating()).unwrap_or(false) && !structdiff::busy()
    }));
    let overall = structdiff::with_view(|v| v.model().narrative.as_ref().map(|n| n.overall.clone())).flatten();
    assert_eq!(overall.as_deref(), Some("generated for []"));
    assert_eq!(structdiff::with_view(|v| v.narrative_open()), Some(true));
    // generating is what exports groups.json for the skill
    assert!(r.root.join(".structdiff/groups.json").exists());
    structdiff::close();
}

#[nvim_oxi::test]
fn branch_range_uses_read_only_revisions_and_its_own_narrative() {
    let r = branch_repo();
    start(&r.root);
    open_wait(Some("main...HEAD"));
    assert_eq!(panel()[1], " main...HEAD");
    let flat: Vec<String> = structdiff::with_view(|v| v.flat().iter().map(|&p| v.model().file(p).path.clone()).collect()).unwrap();
    assert_eq!(flat, ["a.lua", "new.lua", "gone.md"]); // Source before Docs
    let (left, right, real) = structdiff::with_view(|v| (v.left().clone(), v.right().clone(), v.real_buf().is_some())).unwrap();
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
    open_wait(None);
    assert_eq!(panel()[1], " HEAD → working tree");
    structdiff::close();
}

#[nvim_oxi::test]
fn close_removes_the_tab_and_real_buffer_keymaps() {
    let r = sample_repo();
    start(&r.root);
    let tabs = structdiff::ui::tab_count();
    open_wait(None);
    assert_eq!(structdiff::ui::tab_count(), tabs + 1);
    // starts on lua/backoff.lua, a real working-tree buffer
    let buf = structdiff::with_view(|v| v.real_buf().cloned()).flatten().expect("real buffer");
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
    open_wait(None);
    let left = structdiff::with_view(|v| v.left().clone()).unwrap();
    left.close(true).unwrap();
    structdiff::goto_file(1);
    let (left, sidebar) = structdiff::with_view(|v| (v.left().clone(), v.sidebar().clone())).unwrap();
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
    assert!(wait_until(5000, || !structdiff::busy()));
    assert_eq!(panel()[1], " main...");
    api::command("StructDiffClose").unwrap();
    assert!(structdiff::with_view(|_| ()).is_none());
}

#[nvim_oxi::test]
fn unknown_revision_opens_nothing() {
    let r = sample_repo();
    start(&r.root);
    let tabs = structdiff::ui::tab_count();
    open_wait(Some("nope...HEAD"));
    assert!(structdiff::with_view(|_| ()).is_none());
    assert_eq!(structdiff::ui::tab_count(), tabs);
}

#[nvim_oxi::test]
fn user_closing_the_tab_cleans_up() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    api::command("tabclose").unwrap();
    // The poller checks for a closed tab every few ticks, not every tick.
    assert!(wait_until(3000, || structdiff::with_view(|_| ()).is_none()));
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

#[nvim_oxi::test]
fn open_does_not_block_and_reports_progress() {
    let r = sample_repo();
    start(&r.root);
    structdiff::open(None);
    // Returned before git finished: no view yet, but busy.
    assert!(structdiff::busy());
    assert!(wait_until(5000, || !structdiff::busy()));
    assert_eq!(current().as_deref(), Some("lua/backoff.lua"));
    structdiff::close();
}

#[nvim_oxi::test]
fn overlapping_refreshes_apply_only_the_latest() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    write(&r.root, "lua/util.lua", "return 3\n");
    structdiff::refresh();
    structdiff::refresh();
    assert!(wait_until(5000, || !structdiff::busy()));
    assert_eq!(structdiff::with_view(|v| v.model().file_count()), Some(5));
    structdiff::close();
}

#[nvim_oxi::test]
fn version_guard_is_an_exact_allowlist_with_an_explicit_opt_out() {
    assert_eq!(structdiff::check_neovim(structdiff::TESTED_NEOVIM), Ok(()));
    // Same minor, different patch: rejected, since nvim-oxi broke within 0.12.
    let err = structdiff::check_neovim(&[(0, 12, 4)]).unwrap_err();
    assert!(err.contains("built and checked for Neovim 0.12.4"), "{err}");
    assert!(err.contains("structdiff_allow_untested_nvim"), "{err}");
    api::set_var("structdiff_allow_untested_nvim", true).unwrap();
    assert_eq!(structdiff::check_neovim(&[(0, 12, 4)]), Ok(()));
}

#[nvim_oxi::test]
fn viewing_writes_nothing_to_the_repo() {
    let r = sample_repo();
    start(&r.root);
    let exclude = std::fs::read_to_string(r.root.join(".git/info/exclude")).unwrap_or_default();
    open_wait(None);
    refresh_wait();
    structdiff::goto_file(1);
    structdiff::close();
    assert!(!r.root.join(".structdiff").exists());
    assert_eq!(std::fs::read_to_string(r.root.join(".git/info/exclude")).unwrap_or_default(), exclude);
}

#[nvim_oxi::test]
fn toggling_reasons_updates_the_one_config_the_view_renders_from() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    let fp = structdiff::with_view(|v| v.model().fingerprint.clone()).unwrap();
    std::fs::create_dir_all(narrative::dir(&r.root)).unwrap();
    std::fs::write(
        narrative::path(&r.root, ""),
        serde_json::json!({"fingerprint": fp, "files": {"lua/core.lua": "turns retry on"}}).to_string(),
    )
    .unwrap();
    structdiff::reload_narrative();
    assert!(virt_texts().contains(&"turns retry on".into()));
    structdiff::toggle_reasons();
    assert!(!structdiff::config().show_reasons);
    assert!(virt_texts().is_empty(), "{:?}", virt_texts());
    structdiff::toggle_reasons();
    assert!(virt_texts().contains(&"turns retry on".into()));
    structdiff::close();
}

#[nvim_oxi::test]
fn reentrant_calls_are_dropped_with_a_warning_not_silently() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    capture_notifications();
    let inner = structdiff::with_view(|_| structdiff::with_view(|_| 42));
    assert_eq!(inner, Some(None));
    let msgs = notifications();
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("re-entrant call from crates/nvim-tests/src/view.rs:"), "{}", msgs[0]);
    structdiff::close();
}

fn panel_tick() -> i64 {
    let buf = structdiff::with_view(|v| v.panel_buf().clone()).unwrap();
    api::call_function("getbufvar", (buf, "changedtick")).unwrap()
}

/// 1-based sidebar line holding the current-file highlight.
fn current_mark_line() -> Option<usize> {
    use nvim_oxi::api::opts::GetExtmarksOpts;
    use nvim_oxi::api::types::ExtmarkPosition;
    let ns = api::create_namespace("structdiff-current");
    structdiff::with_view(|v| {
        v.panel_buf()
            .get_extmarks(ns, ExtmarkPosition::ByTuple((0, 0)), ExtmarkPosition::ByTuple((usize::MAX >> 33, 0)), &GetExtmarksOpts::default())
            .unwrap()
            .map(|(_, row, _, _)| row + 1)
            .next()
    })
    .unwrap()
}

#[nvim_oxi::test]
fn moving_between_files_only_moves_the_current_mark() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None); // on Source's backoff.lua; core.lua is next
    let tick = panel_tick();
    let before = current_mark_line().unwrap();
    structdiff::goto_file(1);
    assert_eq!(panel_tick(), tick, "the sidebar text was rewritten");
    let after = current_mark_line().unwrap();
    assert_eq!(after, before + 1);
    assert_eq!(panel()[after - 1], "  M core.lua  lua");
    structdiff::close();
}

#[nvim_oxi::test]
fn highlight_links_survive_a_colorscheme_clear() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    api::command("highlight clear").unwrap();
    structdiff::goto_file(1);
    let out: String = api::call_function("execute", ("highlight StructDiffTitle",)).unwrap();
    assert!(out.contains("links to Title"), "{out}");
    structdiff::close();
}

#[nvim_oxi::test]
fn moving_into_a_folded_group_unfolds_it() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    structdiff::goto_file(1); // last file of Source; Tests is next
    let tests = panel().iter().position(|l| l.starts_with("▾ Tests")).unwrap();
    structdiff::with_view(|v| v.sidebar().clone()).unwrap().set_cursor(tests + 1, 0).unwrap();
    structdiff::fold_at_cursor();
    assert!(panel().contains(&"▸ Tests (1)".to_owned()), "{:?}", panel());
    structdiff::goto_file(1);
    assert_eq!(current().as_deref(), Some("tests/core_spec.lua"));
    assert!(panel().contains(&"▾ Tests (1)".to_owned()), "{:?}", panel());
    assert!(panel().contains(&"  M core_spec.lua  tests".to_owned()));
    structdiff::close();
}

/// Configure generate_cmd as a shell script run from the repo root.
fn generate_with(script: &str, timeout_secs: u64) {
    let opts = api::call_function::<_, nvim_oxi::Object>(
        "luaeval",
        (
            "{ generate_cmd = { 'sh', '-c', _A[1] }, generate_timeout = _A[2] }",
            nvim_oxi::Array::from_iter([nvim_oxi::Object::from(script), nvim_oxi::Object::from(timeout_secs as i64)]),
        ),
    )
    .unwrap();
    structdiff::setup(opts);
}

fn process_alive(pid: &str) -> bool {
    std::process::Command::new("kill").args(["-0", pid.trim()]).status().unwrap().success()
}

fn generating() -> bool {
    structdiff::with_view(|v| v.generating()).unwrap_or(false) || structdiff::busy()
}

// The command backgrounds a grandchild `sleep` and records its pid, so the
// test can check the whole process group was stopped.
const SLOW: &str = "sleep 30 & echo $! > .structdiff/sleep.pid; wait";

#[nvim_oxi::test]
fn cancel_stops_the_command_and_its_children() {
    let r = sample_repo();
    start(&r.root);
    generate_with(SLOW, 0);
    open_wait(None);
    capture_notifications();
    structdiff::generate(None);
    let pid_file = r.root.join(".structdiff/sleep.pid");
    assert!(wait_until(5000, || std::fs::read_to_string(&pid_file).is_ok_and(|p| !p.trim().is_empty())));
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    assert!(process_alive(&pid));
    structdiff::cancel_generate();
    assert!(wait_until(5000, || !generating()), "generation did not stop");
    assert!(notifications().iter().any(|m| m.contains("narrative generation cancelled")), "{:?}", notifications());
    assert!(!process_alive(&pid), "the command's child process survived");
    structdiff::close();
}

#[nvim_oxi::test]
fn generation_times_out() {
    let r = sample_repo();
    start(&r.root);
    generate_with(SLOW, 1);
    open_wait(None);
    capture_notifications();
    structdiff::generate(None);
    assert!(wait_until(6000, || !generating()), "generation did not time out");
    assert!(notifications().iter().any(|m| m.contains("timed out after 1s")), "{:?}", notifications());
    let pid = std::fs::read_to_string(r.root.join(".structdiff/sleep.pid")).unwrap();
    assert!(!process_alive(&pid));
    structdiff::close();
}

#[nvim_oxi::test]
fn cancel_without_a_generation_says_so() {
    let r = sample_repo();
    start(&r.root);
    capture_notifications();
    structdiff::cancel_generate();
    assert_eq!(notifications(), ["structdiff: no narrative generation is running"]);
}

#[nvim_oxi::test]
fn background_jobs_are_all_handled() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    for _ in 0..20 {
        structdiff::refresh();
    }
    assert!(wait_until(10000, || !structdiff::busy()));
    assert_eq!(structdiff::background_jobs(), 0);
    structdiff::close();
}

#[nvim_oxi::test]
fn generator_is_chosen_from_setup() {
    let lua = |expr: &str| api::call_function::<_, nvim_oxi::Object>("luaeval", (expr,)).unwrap();
    let cmd = |spec: &str| structdiff::config().generate_cmd_for(spec, "PROMPT");
    assert_eq!(cmd("")[0], "claude"); // default
    structdiff::setup(lua("{ generator = 'codex' }"));
    assert_eq!(cmd("main...")[..4], ["codex", "exec", "--sandbox", "read-only"]);
    assert_eq!(cmd("main...").last().unwrap(), "PROMPT");
    structdiff::setup(lua("{ generator = 'copilot' }"));
    assert_eq!(cmd("")[..3], ["copilot", "-p", "PROMPT"]);
    // generate_cmd overrides the generator entirely
    structdiff::setup(lua("{ generator = 'codex', generate_cmd = { 'my-agent', '{range}', '{output}' } }"));
    assert_eq!(cmd("main.."), ["my-agent", "main..", ".structdiff/narrative-main...json"]);
    // an unknown generator is reported and leaves the config alone
    capture_notifications();
    structdiff::setup(lua("{ generator = 'gpt' }"));
    assert_eq!(cmd("main..")[0], "my-agent");
    assert!(notifications().iter().any(|m| m.contains("invalid setup options")), "{:?}", notifications());
}

#[nvim_oxi::test]
fn an_agents_json_reply_becomes_a_fresh_narrative() {
    let r = sample_repo();
    start(&r.root);
    // Stand-in agent: answers with JSON on stdout, including a path that
    // isn't in the change set and a forged fingerprint.
    let reply = r#"Sure: {"fingerprint":"forged","overall":"Adds retry.","files":{"lua/core.lua":"turns retry on","nope.txt":"x"},"order":["lua/core.lua"]}"#;
    let opts = api::call_function::<_, nvim_oxi::Object>(
        "luaeval",
        ("{ generate_cmd = { 'printf', '%s', _A } }", reply),
    )
    .unwrap();
    structdiff::setup(opts);
    open_wait(None);
    structdiff::generate(None);
    assert!(wait_until(5000, || structdiff::with_view(|v| !v.generating()).unwrap_or(false) && !structdiff::busy()));
    let (state, files) = structdiff::with_view(|v| {
        let n = v.model().narrative.clone().unwrap();
        (v.model().state, n.files.keys().cloned().collect::<Vec<_>>())
    })
    .unwrap();
    assert_eq!(state, State::Fresh);
    assert_eq!(files, ["lua/core.lua"]);
    structdiff::close();
}

#[nvim_oxi::test]
fn a_reply_that_is_not_a_narrative_is_reported() {
    let r = sample_repo();
    start(&r.root);
    let opts = api::call_function::<_, nvim_oxi::Object>("luaeval", ("{ generate_cmd = { 'echo', 'I cannot help with that' } }",)).unwrap();
    structdiff::setup(opts);
    open_wait(None);
    capture_notifications();
    structdiff::generate(None);
    assert!(wait_until(5000, || structdiff::with_view(|v| !v.generating()).unwrap_or(false) && !structdiff::busy()));
    assert!(
        notifications().iter().any(|m| m.contains("the agent's reply wasn't a narrative") && m.contains("I cannot help")),
        "{:?}",
        notifications()
    );
    assert!(!narrative::path(&r.root, "").exists());
    structdiff::close();
}

#[nvim_oxi::test]
fn an_ex_error_while_showing_a_file_is_reported_not_fatal() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None); // on lua/backoff.lua, a real working-tree buffer
    // Unsaved edit in the real file: re-running `:edit` on it raises E37.
    let mut buf = structdiff::with_view(|v| v.right().get_buf().unwrap()).unwrap();
    buf.set_lines(0..0, false, ["-- unsaved"]).unwrap();
    capture_notifications();
    refresh_wait();
    assert!(notifications().iter().any(|m| m.contains("E37")), "{:?}", notifications());
    // Neovim is still alive, the view is intact and the edit is kept.
    assert_eq!(current().as_deref(), Some("lua/backoff.lua"));
    assert_eq!(lines(&buf)[0], "-- unsaved");
    structdiff::close();
}

#[nvim_oxi::test]
fn a_failing_user_autocmd_does_not_take_neovim_down() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    api::command("autocmd BufReadPost core.lua lua error('user autocmd failed')").unwrap();
    capture_notifications();
    structdiff::goto_file(1); // loads lua/core.lua with :edit
    assert_eq!(current().as_deref(), Some("lua/core.lua"));
    assert!(notifications().iter().any(|m| m.contains("user autocmd failed")), "{:?}", notifications());
    structdiff::close();
}

#[nvim_oxi::test]
fn lua_functions_tolerate_missing_arguments() {
    let r = sample_repo();
    start(&r.root);
    open_wait(None);
    // goto_file() with no delta used to abort Neovim; it now means +1.
    let _: nvim_oxi::Object = api::call_function("luaeval", ("require('structdiff') ~= nil",)).unwrap_or(nvim_oxi::Object::nil());
    let module = structdiff::init().unwrap();
    let goto: nvim_oxi::Function<(), ()> = nvim_oxi::conversion::FromObject::from_object(module.get("goto_file").unwrap().clone()).unwrap();
    goto.call(()).unwrap();
    assert_eq!(current().as_deref(), Some("lua/core.lua"));
    structdiff::close();
}
