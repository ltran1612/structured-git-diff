//! Everything structdiff knows about git: repo discovery, range resolution,
//! changed files, file contents and the change-set fingerprint. All calls go
//! through the `git` binary.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// `git hash-object -t tree /dev/null`: the base when the repo has no commits.
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub root: PathBuf,
    /// Absolute path of `info/exclude` (works for worktrees too).
    pub exclude: PathBuf,
}

/// What to compare. `target == None` means the working tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    pub spec: String,
    pub base: String,
    pub target: Option<String>,
    pub left_label: String,
    pub right_label: String,
}

impl Range {
    /// Human description for headers and messages.
    pub fn describe(&self) -> String {
        match self.target {
            Some(_) => self.spec.clone(),
            None => format!("{} → working tree", self.left_label),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    /// One of M A D R C U, or ? for untracked.
    pub status: char,
    pub path: String,
    pub old_path: Option<String>,
}

impl ChangedFile {
    pub fn new(status: char, path: &str) -> Self {
        Self { status, path: path.to_owned(), old_path: None }
    }
}

fn run_raw(cwd: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(cwd).args(args);
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(Error::GitUnavailable)?;
    // Feed stdin from another thread while this one reads stdout. Writing
    // it all first deadlocks once git's output fills its pipe: git stops
    // reading input until we read, and we never do (`hash-object
    // --stdin-paths` with a few thousand paths did exactly that).
    let writer = stdin.map(|input| {
        let mut pipe = child.stdin.take().expect("piped stdin");
        let input = input.to_vec();
        std::thread::spawn(move || pipe.write_all(&input))
    });
    let out = child.wait_with_output().map_err(Error::GitUnavailable)?;
    if let Some(writer) = writer {
        // git may exit without reading all its input (e.g. on an error);
        // its exit status says what went wrong, not the broken pipe.
        let _ = writer.join();
    }
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(Error::Git { args: args.join(" "), stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned() })
    }
}

fn run(cwd: &Path, args: &[&str]) -> Result<String> {
    run_raw(cwd, args, None).map(|b| String::from_utf8_lossy(&b).into_owned())
}

impl Repo {
    /// The repo containing `dir`, or an error when it is not in a work tree.
    pub fn discover(dir: &Path) -> Result<Self> {
        let toplevel = run(dir, &["rev-parse", "--show-toplevel"]).map_err(|e| match e {
            Error::Git { stderr, .. } => Error::NotARepo(stderr),
            other => other,
        })?;
        let root = PathBuf::from(toplevel.trim());
        let exclude = run(&root, &["rev-parse", "--path-format=absolute", "--git-path", "info/exclude"])?;
        Ok(Self { exclude: PathBuf::from(exclude.trim()), root })
    }

    fn commit(&self, rev: &str) -> Option<String> {
        run(&self.root, &["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")])
            .ok()
            .map(|s| s.trim().to_owned())
    }

    /// Resolve what to compare. `spec` is one of:
    ///   ""        HEAD vs working tree (empty tree if there are no commits)
    ///   "A"       A vs working tree
    ///   "A..B"    A vs B
    ///   "A...B"   merge-base(A, B) vs B, i.e. what B adds on top of A
    /// An empty side of ".." / "..." means HEAD.
    pub fn resolve_range(&self, spec: &str) -> Result<Range> {
        let spec = spec.trim();
        if spec.is_empty() {
            let head = self.commit("HEAD").is_some();
            return Ok(Range {
                spec: String::new(),
                base: if head { "HEAD" } else { EMPTY_TREE }.to_owned(),
                target: None,
                left_label: if head { "HEAD" } else { "empty" }.to_owned(),
                right_label: "worktree".to_owned(),
            });
        }
        let Some(dots_at) = spec.find("..") else {
            let sha = self.commit(spec).ok_or_else(|| Error::UnknownRevision(spec.to_owned()))?;
            return Ok(Range {
                spec: spec.to_owned(),
                base: sha,
                target: None,
                left_label: spec.to_owned(),
                right_label: "worktree".to_owned(),
            });
        };
        let three = spec[dots_at..].starts_with("...");
        let a = &spec[..dots_at];
        let b = &spec[dots_at + if three { 3 } else { 2 }..];
        let a = if a.is_empty() { "HEAD" } else { a };
        let b = if b.is_empty() { "HEAD" } else { b };
        let asha = self.commit(a).ok_or_else(|| Error::UnknownRevision(a.to_owned()))?;
        let bsha = self.commit(b).ok_or_else(|| Error::UnknownRevision(b.to_owned()))?;
        let (base, left_label) = if three {
            let mb = run(&self.root, &["merge-base", &asha, &bsha])
                .map_err(|_| Error::NoMergeBase { a: a.to_owned(), b: b.to_owned() })?;
            (mb.trim().to_owned(), format!("merge-base({a})"))
        } else {
            (asha, a.to_owned())
        };
        Ok(Range { spec: spec.to_owned(), base, target: Some(bsha), left_label, right_label: b.to_owned() })
    }

    fn diff_args<'a>(range: &'a Range, extra: &[&'a str]) -> Vec<&'a str> {
        let mut args = vec!["diff", range.base.as_str()];
        if let Some(t) = &range.target {
            args.push(t);
        }
        args.extend_from_slice(extra);
        // Without "--", a file named like a revision (say, HEAD) makes git
        // refuse the command as ambiguous.
        args.push("--");
        args
    }

    /// Changed files in `range` (sorted by path) and the change-set
    /// fingerprint, from a single `git diff --raw` plus hashing of working-tree
    /// files git didn't hash itself. Against the working tree this includes
    /// staged, unstaged and untracked files.
    pub fn scan(&self, range: &Range) -> Result<Scan> {
        let raw = run_raw(&self.root, &Self::diff_args(range, &["--raw", "-z", "--abbrev=40", "-M", "--no-ext-diff"]), None)?;
        let entries = parse_raw(&String::from_utf8_lossy(&raw));
        let mut files: Vec<ChangedFile> = entries.iter().map(|e| e.file.clone()).collect();

        // Paths whose content the fingerprint must cover but git didn't hash:
        // working-tree files shown with an all-zero blob id, and untracked files.
        let mut to_hash: Vec<String> = Vec::new();
        if range.target.is_none() {
            to_hash.extend(
                entries.iter().filter(|e| e.dst_unknown() && e.file.status != 'D').map(|e| e.file.path.clone()),
            );
            let untracked = run(&self.root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
            for p in untracked.split('\0').filter(|p| !p.is_empty() && !is_internal(p)) {
                files.push(ChangedFile::new('?', p));
                to_hash.push(p.to_owned());
            }
        }
        files.retain(|f| !is_internal(&f.path));
        files.sort_by(|a, b| a.path.cmp(&b.path));

        // Hash a normalized line per change, with every blob id resolved to
        // the real content. Hashing git's raw output directly would make
        // `git add` alone change the fingerprint (the right-hand id goes from
        // all-zeros to the real id).
        // Ids are computed here rather than by `git hash-object`, which
        // fails the whole batch on one path it can't hash (a nested repo, a
        // symlink to a directory, a dangling symlink) and mis-reads names
        // starting with a quote.
        let resolved: std::collections::HashMap<String, String> =
            to_hash.iter().map(|p| (p.clone(), worktree_id(&self.root, p))).collect();
        let mut lines: Vec<String> = entries
            .iter()
            .filter(|e| !is_internal(&e.file.path))
            .map(|e| {
                let dst = if e.dst_unknown() && e.file.status != 'D' {
                    resolved.get(&e.file.path).cloned().unwrap_or_default()
                } else {
                    e.dst_sha.clone()
                };
                let old = e.file.old_path.as_deref().unwrap_or("");
                format!("{} {} {} {} {} {} {}", e.src_mode, e.dst_mode, e.src_sha, dst, e.file.status, old, e.file.path)
            })
            .collect();
        for f in files.iter().filter(|f| f.status == '?') {
            lines.push(format!("? {} {}", resolved.get(&f.path).map_or("", String::as_str), f.path));
        }
        lines.sort();
        let mut hasher = Sha256::new();
        hasher.update(lines.join("\n").as_bytes());
        let fingerprint = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        Ok(Scan { files, fingerprint })
    }

    /// The range's patch text, cut at `limit` bytes (on a line boundary).
    /// Returns the text and whether it was cut.
    pub fn diff_text(&self, range: &Range, limit: usize) -> Result<(String, bool)> {
        let out = run_raw(&self.root, &Self::diff_args(range, &["--no-color", "--no-ext-diff", "-M"]), None)?;
        let text = String::from_utf8_lossy(&out);
        if text.len() <= limit {
            return Ok((text.into_owned(), false));
        }
        let cut = text[..text.floor_char_boundary(limit)].rfind('\n').map_or(0, |i| i + 1);
        Ok((text[..cut].to_owned(), true))
    }

    /// Subjects and bodies of the commits in a committed range (empty for
    /// working-tree ranges).
    pub fn log(&self, range: &Range) -> Result<String> {
        match &range.target {
            Some(target) => run(&self.root, &["log", "--format=%h %s%n%b", &format!("{}..{target}", range.base)]),
            None => Ok(String::new()),
        }
    }

    /// File content at `rev`, or None when it doesn't exist there.
    pub fn content(&self, rev: &str, path: &str) -> Option<Vec<u8>> {
        run_raw(&self.root, &["show", &format!("{rev}:{path}")], None).ok()
    }

    /// HEAD followed by branch, remote and tag names, for completion.
    pub fn refs(&self) -> Vec<String> {
        let out = run(
            &self.root,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes", "refs/tags"],
        )
        .unwrap_or_default();
        std::iter::once("HEAD".to_owned()).chain(out.lines().map(str::to_owned)).collect()
    }
}

/// Git's blob id for `bytes`: SHA-1 over `"blob <len>\0"` and the bytes.
fn blob_id(bytes: &[u8]) -> String {
    use sha1::{Digest as _, Sha1};
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", bytes.len()).as_bytes());
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A content id for a working-tree path, for the fingerprint. For regular
/// files and symlinks it is the id git would store (without clean filters
/// such as autocrlf or LFS), so staging a file doesn't change the
/// fingerprint. Paths git couldn't hash still get a stable id: a directory
/// (a nested repo, or an untracked folder git lists as `dir/`) by its
/// HEAD commit, anything unreadable by kind.
pub fn worktree_id(root: &Path, path: &str) -> String {
    let full = root.join(path.trim_end_matches('/'));
    let Ok(meta) = std::fs::symlink_metadata(&full) else { return "missing".into() };
    if meta.file_type().is_symlink() {
        return match std::fs::read_link(&full) {
            #[cfg(unix)]
            Ok(target) => blob_id(std::os::unix::ffi::OsStrExt::as_bytes(target.as_os_str())),
            #[cfg(not(unix))]
            Ok(target) => blob_id(target.to_string_lossy().as_bytes()),
            Err(_) => "unreadable-link".into(),
        };
    }
    if meta.is_dir() {
        let head = run(&full, &["rev-parse", "HEAD"]).unwrap_or_default();
        return format!("dir:{}", head.trim());
    }
    if meta.is_file() {
        return std::fs::read(&full).map_or_else(|_| "unreadable".into(), |bytes| blob_id(&bytes));
    }
    "special".into()
}

/// The result of [`Repo::scan`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scan {
    pub files: Vec<ChangedFile>,
    pub fingerprint: String,
}

/// structdiff's own scratch directory never counts as a change, even before
/// it has been added to `.git/info/exclude`.
fn is_internal(path: &str) -> bool {
    path == ".structdiff" || path.starts_with(".structdiff/")
}

/// One entry of `git diff --raw -z --abbrev=40` output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    pub file: ChangedFile,
    pub src_mode: String,
    pub dst_mode: String,
    pub src_sha: String,
    /// Blob id of the right-hand side; all zeros when git didn't hash it (a
    /// working-tree file that differs from the index).
    pub dst_sha: String,
}

impl RawEntry {
    fn dst_unknown(&self) -> bool {
        self.dst_sha.bytes().all(|b| b == b'0')
    }
}

/// Parse `git diff --raw -z --abbrev=40` output: for each change, a header
/// `:<mode> <mode> <sha> <sha> <status>` then one path (two for renames and
/// copies), all NUL-separated.
pub fn parse_raw(out: &str) -> Vec<RawEntry> {
    let toks: Vec<&str> = out.split('\0').filter(|t| !t.is_empty()).collect();
    let mut entries = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let header: Vec<&str> = toks[i].trim_start_matches(':').split(' ').collect();
        let [src_mode, dst_mode, src_sha, dst_sha, code] = header[..] else { break };
        let letter = code.chars().next().unwrap_or('M');
        let file = if letter == 'R' || letter == 'C' {
            let (Some(old), Some(new)) = (toks.get(i + 1), toks.get(i + 2)) else { break };
            i += 3;
            ChangedFile { status: letter, old_path: Some((*old).to_owned()), path: (*new).to_owned() }
        } else {
            let Some(path) = toks.get(i + 1) else { break };
            i += 2;
            ChangedFile::new(if letter == 'T' { 'M' } else { letter }, path)
        };
        entries.push(RawEntry {
            file,
            src_mode: src_mode.to_owned(),
            dst_mode: dst_mode.to_owned(),
            src_sha: src_sha.to_owned(),
            dst_sha: dst_sha.to_owned(),
        });
    }
    entries
}

/// Split a range-ish command-line argument into the prefix to keep and the
/// ref being typed: "main...fe" -> ("main...", "fe"), "ma" -> ("", "ma").
pub fn split_completion(arglead: &str) -> (&str, &str) {
    match arglead.find("..") {
        Some(i) => {
            let end = if arglead[i..].starts_with("...") { i + 3 } else { i + 2 };
            arglead.split_at(end)
        }
        None => ("", arglead),
    }
}
