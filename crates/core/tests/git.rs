use structdiff_testutil::*;
use structdiff_core::Error;
use structdiff_core::git::{self, ChangedFile, Repo};

#[test]
fn parse_raw_handles_renames_type_changes_and_unknown_hashes() {
    let z = "0".repeat(40);
    let a = "a".repeat(40);
    let out = format!(
        ":100644 100644 {a} {z} M\0a.lua\0:100644 100644 {a} {a} R087\0old.lua\0new.lua\0\
         :100644 000000 {a} {z} D\0gone.txt\0:120000 100644 {a} {a} T\0link\0"
    );
    let entries = git::parse_raw(&out);
    let files: Vec<ChangedFile> = entries.iter().map(|e| e.file.clone()).collect();
    assert_eq!(
        files,
        vec![
            ChangedFile::new('M', "a.lua"),
            ChangedFile { status: 'R', path: "new.lua".into(), old_path: Some("old.lua".into()) },
            ChangedFile::new('D', "gone.txt"),
            ChangedFile::new('M', "link"),
        ]
    );
    assert_eq!(entries[0].dst_sha, z);
    assert_eq!(entries[1].dst_sha, a);
}

#[test]
fn structdiff_scratch_files_are_never_changes() {
    let r = repo(&[("a.lua", "a\n")]);
    write(&r.root, ".structdiff/narrative.json", "{}"); // not yet excluded
    write(&r.root, "b.lua", "b\n");
    let repo = Repo::discover(&r.root).unwrap();
    let range = repo.resolve_range("").unwrap();
    assert_eq!(summary(&repo.scan(&range).unwrap().files), ["? b.lua"]);
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
    let files = repo.scan(&range).unwrap().files;
    assert_eq!(summary(&files), ["M a.lua", "D b.lua", "R new name.lua", "A staged.lua", "? untracked.md"]);
    assert_eq!(files[2].old_path.as_deref(), Some("old name.lua"));
}

#[test]
fn fingerprint_changes_with_tracked_and_untracked_edits() {
    let r = repo(&[("a.lua", "a\n")]);
    let repo = Repo::discover(&r.root).unwrap();
    let range = repo.resolve_range("").unwrap();
    let fp = || repo.scan(&range).unwrap().fingerprint;
    write(&r.root, "a.lua", "a2\n");
    write(&r.root, "new.txt", "1\n");
    let fp1 = fp();
    assert_eq!(fp1, fp());
    assert_eq!(fp1.len(), 64);
    write(&r.root, "new.txt", "2\n");
    let fp2 = fp();
    assert_ne!(fp1, fp2, "untracked edit should change fingerprint");
    write(&r.root, "a.lua", "a3\n");
    let fp3 = fp();
    assert_ne!(fp2, fp3, "tracked edit should change fingerprint");
    // staging an edit doesn't change the change set against HEAD
    git(&r.root, &["add", "a.lua"]);
    assert_eq!(fp3, fp(), "staging alone should not change fingerprint");
    // but editing a staged file again does
    write(&r.root, "a.lua", "a4\n");
    assert_ne!(fp3, fp(), "edit after staging should change fingerprint");
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
    assert_eq!(summary(&repo.scan(&range).unwrap().files), ["A x.lua"]);
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
    assert_eq!(summary(&repo.scan(&range).unwrap().files), ["M a.lua", "D gone.md", "A new.lua"]);
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
        summary(&repo.scan(&range).unwrap().files),
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
        summary(&repo.scan(&range).unwrap().files),
        ["M a.lua", "D gone.md", "A new.lua", "? scratch.txt", "M shared.lua"]
    );
}

#[test]
fn unknown_revisions_are_reported() {
    let r = branch_repo();
    let repo = Repo::discover(&r.root).unwrap();
    assert!(matches!(repo.resolve_range("nope"), Err(Error::UnknownRevision(r)) if r == "nope"));
    assert!(matches!(repo.resolve_range("main...nope"), Err(Error::UnknownRevision(r)) if r == "nope"));
    assert_eq!(repo.resolve_range("nope").unwrap_err().to_string(), "unknown revision: nope");
}

#[test]
fn not_a_repo_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = Repo::discover(dir.path()).unwrap_err();
    assert!(matches!(err, Error::NotARepo(_)), "{err:?}");
    assert!(err.to_string().starts_with("not a git repository: "), "{err}");
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

#[test]
fn unrelated_histories_have_no_merge_base() {
    let r = repo(&[("a", "a\n")]);
    git(&r.root, &["checkout", "-q", "--orphan", "island"]);
    write(&r.root, "b", "b\n");
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "island"]);
    let repo = Repo::discover(&r.root).unwrap();
    let err = repo.resolve_range("main...island").unwrap_err();
    assert!(matches!(&err, Error::NoMergeBase { a, b } if a == "main" && b == "island"), "{err:?}");
    // two-dot ranges don't need a merge base
    assert!(repo.resolve_range("main..island").is_ok());
}

fn summary(files: &[structdiff_core::ChangedFile]) -> Vec<String> {
    files.iter().map(|f| format!("{} {}", f.status, f.path)).collect()
}
