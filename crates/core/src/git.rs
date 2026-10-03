//! Everything structdiff knows about git: repo discovery, range resolution,
//! changed files, file contents and the change-set fingerprint. All calls go
//! through the `git` binary.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

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

fn run_raw(cwd: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(cwd).args(args);
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("cannot run git: {e}"))?;
    if let Some(input) = stdin {
        let mut pipe = child.stdin.take().expect("piped stdin");
        pipe.write_all(input).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

fn run(cwd: &Path, args: &[&str]) -> Result<String, String> {
    run_raw(cwd, args, None).map(|b| String::from_utf8_lossy(&b).into_owned())
}

impl Repo {
    /// The repo containing `dir`, or an error when it is not in a work tree.
    pub fn discover(dir: &Path) -> Result<Self, String> {
        let root = PathBuf::from(run(dir, &["rev-parse", "--show-toplevel"])?.trim());
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
    pub fn resolve_range(&self, spec: &str) -> Result<Range, String> {
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
            let sha = self.commit(spec).ok_or_else(|| format!("unknown revision: {spec}"))?;
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
        let asha = self.commit(a).ok_or_else(|| format!("unknown revision: {a}"))?;
        let bsha = self.commit(b).ok_or_else(|| format!("unknown revision: {b}"))?;
        let (base, left_label) = if three {
            let mb = run(&self.root, &["merge-base", &asha, &bsha])
                .map_err(|_| format!("no merge base between {a} and {b}"))?;
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
        args
    }

    /// Changed files in `range`, sorted by path. Against the working tree this
    /// includes staged, unstaged and untracked files.
    pub fn changed_files(&self, range: &Range) -> Result<Vec<ChangedFile>, String> {
        let out = run(&self.root, &Self::diff_args(range, &["--name-status", "-z", "-M", "--no-ext-diff"]))?;
        let mut files = parse_name_status(&out);
        if range.target.is_none() {
            let untracked = run(&self.root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
            files.extend(untracked.split('\0').filter(|p| !p.is_empty()).map(|p| ChangedFile::new('?', p)));
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// sha256 of the range's diff plus the contents of untracked files. Any
    /// change to the change set changes it.
    pub fn fingerprint(&self, range: &Range, files: &[ChangedFile]) -> String {
        let mut hasher = Sha256::new();
        let diff = run_raw(&self.root, &Self::diff_args(range, &["--no-color", "--no-ext-diff", "--binary"]), None)
            .unwrap_or_default();
        hasher.update(&diff);
        let untracked: Vec<&str> = files.iter().filter(|f| f.status == '?').map(|f| f.path.as_str()).collect();
        if !untracked.is_empty() {
            let input = untracked.join("\n") + "\n";
            let hashes = run_raw(&self.root, &["hash-object", "--stdin-paths"], Some(input.as_bytes()))
                .unwrap_or_default();
            let hashes = String::from_utf8_lossy(&hashes);
            for (path, hash) in untracked.iter().zip(hashes.lines().chain(std::iter::repeat(""))) {
                hasher.update(b"\0");
                hasher.update(format!("{path} {hash}").as_bytes());
            }
        }
        hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
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

/// Parse `git diff --name-status -z` output.
pub fn parse_name_status(out: &str) -> Vec<ChangedFile> {
    let toks: Vec<&str> = out.split('\0').filter(|t| !t.is_empty()).collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let letter = toks[i].chars().next().unwrap_or('M');
        if letter == 'R' || letter == 'C' {
            if i + 2 >= toks.len() {
                break;
            }
            files.push(ChangedFile {
                status: letter,
                old_path: Some(toks[i + 1].to_owned()),
                path: toks[i + 2].to_owned(),
            });
            i += 3;
        } else {
            let Some(path) = toks.get(i + 1) else { break };
            files.push(ChangedFile::new(if letter == 'T' { 'M' } else { letter }, path));
            i += 2;
        }
    }
    files
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
