mod common;

use common::*;
use structdiff_core::git::{self, ChangedFile, Repo};

#[test]
fn parse_name_status_handles_renames_and_type_changes() {
    let out = "M\0a.lua\0R087\0old.lua\0new.lua\0D\0gone.txt\0T\0link\0";
    assert_eq!(
        git::parse_name_status(out),
        vec![
            ChangedFile::new('M', "a.lua"),
            ChangedFile { status: 'R', path: "new.lua".into(), old_path: Some("old.lua".into()) },
            ChangedFile::new('D', "gone.txt"),
            ChangedFile::new('M', "link"),
        ]
    );
}

#[test]
fn changed_files_covers_staged_unstaged_untracked_deleted_renamed() {
    let r = repo(&[("a.lua", "a\n"), ("b.lua", "b\n"), ("old name.lua", "x\ny\nz\n")]);
    write(&r.root, "a.lua", "a2\n");
    write(&r.root, "staged.lua", "s\n");
    git(&r.root, &["add", "staged.lua"]);
    remove(&r.root, "b.lua");
    git(&r.root, &["mv", "old name.lua", "new name.lua"]);
    write(&r.root, "untracked.md", "u\n");
    let repo = Repo::discover(&r.root).unwrap();
    let range = repo.resolve_range("").unwrap();
    assert_eq!(range.base, "HEAD");
    assert_eq!(range.target, None);
    let files = repo.changed_files(&range).unwrap();
    assert_eq!(summary(&files), ["M a.lua", "D b.lua", "R new name.lua", "A staged.lua", "? untracked.md"]);
    assert_eq!(files[2].old_path.as_deref(), Some("old name.lua"));
}

#[test]
fn fingerprint_changes_with_tracked_and_untracked_edits() {
    let r = repo(&[("a.lua", "a\n")]);
    let repo = Repo::discover(&r.root).unwrap();
    let range = repo.resolve_range("").unwrap();
    let fp = || repo.fingerprint(&range, &repo.changed_files(&range).unwrap());
    write(&r.root, "a.lua", "a2\n");
    write(&r.root, "new.txt", "1\n");
    let fp1 = fp();
    assert_eq!(fp1, fp());
    assert_eq!(fp1.len(), 64);
    write(&r.root, "new.txt", "2\n");
    let fp2 = fp();
    assert_ne!(fp1, fp2, "untracked edit should change fingerprint");
    write(&r.root, "a.lua", "a3\n");
    assert_ne!(fp2, fp(), "tracked edit should change fingerprint");
}

#[test]
fn repo_without_commits_diffs_against_the_empty_tree() {
    let r = empty_repo();
    write(&r.root, "x.lua", "x\n");
    git(&r.root, &["add", "x.lua"]);
    let repo = Repo::discover(&r.root).unwrap();
    let range = repo.resolve_range("").unwrap();
    assert_eq!(range.base, git::EMPTY_TREE);
    assert_eq!(range.left_label, "empty");
    assert_eq!(summary(&repo.changed_files(&range).unwrap()), ["A x.lua"]);
}

#[test]
fn three_dots_uses_the_merge_base_and_ignores_uncommitted_changes() {
    let r = branch_repo();
    let repo = Repo::discover(&r.root).unwrap();
    let range = repo.resolve_range("main...HEAD").unwrap();
    assert_eq!(range.base, git(&r.root, &["merge-base", "main", "HEAD"]).trim());
    assert_eq!(range.target.as_deref(), Some(git(&r.root, &["rev-parse", "HEAD"]).trim()));
    assert_eq!(range.left_label, "merge-base(main)");
    assert_eq!(range.right_label, "HEAD");
    assert_eq!(summary(&repo.changed_files(&range).unwrap()), ["M a.lua", "D gone.md", "A new.lua"]);
    assert_eq!(repo.content(range.target.as_ref().unwrap(), "a.lua").unwrap(), b"a feature\n");
    // an empty right side means HEAD
    assert_eq!(repo.resolve_range("main...").unwrap().base, range.base);
}

#[test]
fn two_dots_compares_the_two_tips_directly() {
    let r = branch_repo();
    let repo = Repo::discover(&r.root).unwrap();
    let range = repo.resolve_range("main..feature").unwrap();
    assert_eq!(range.base, git(&r.root, &["rev-parse", "main"]).trim());
    // main's own commit shows up as a change too, unlike with "..."
    assert_eq!(
        summary(&repo.changed_files(&range).unwrap()),
        ["M a.lua", "D gone.md", "A new.lua", "M shared.lua"]
    );
}

#[test]
fn single_rev_compares_it_to_the_working_tree() {
    let r = branch_repo();
    let repo = Repo::discover(&r.root).unwrap();
    let range = repo.resolve_range("main").unwrap();
    assert_eq!(range.target, None);
    assert_eq!(range.right_label, "worktree");
    assert_eq!(range.describe(), "main → working tree");
    assert_eq!(
        summary(&repo.changed_files(&range).unwrap()),
        ["M a.lua", "D gone.md", "A new.lua", "? scratch.txt", "M shared.lua"]
    );
}

#[test]
fn unknown_revisions_are_reported() {
    let r = branch_repo();
    let repo = Repo::discover(&r.root).unwrap();
    assert_eq!(repo.resolve_range("nope").unwrap_err(), "unknown revision: nope");
    assert_eq!(repo.resolve_range("main...nope").unwrap_err(), "unknown revision: nope");
}

#[test]
fn not_a_repo_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Repo::discover(dir.path()).is_err());
}

#[test]
fn refs_lists_head_and_branches() {
    let r = branch_repo();
    let refs = Repo::discover(&r.root).unwrap().refs();
    assert_eq!(refs[0], "HEAD");
    assert!(refs.contains(&"main".into()) && refs.contains(&"feature".into()), "{refs:?}");
}

#[test]
fn split_completion_keeps_the_range_prefix() {
    assert_eq!(git::split_completion("ma"), ("", "ma"));
    assert_eq!(git::split_completion("main..fe"), ("main..", "fe"));
    assert_eq!(git::split_completion("main...fe"), ("main...", "fe"));
    assert_eq!(git::split_completion("main..."), ("main...", ""));
}
