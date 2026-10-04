//! The narrative contract. The diff-narrative skill writes
//! `<root>/.structdiff/narrative*.json`; the viewer writes
//! `<root>/.structdiff/groups.json` so the skill uses the same range and
//! grouping. Neither side calls the other.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::git::{Range, Repo};
use crate::group::{Group, Grouping};

pub const VERSION: u32 = 1;

pub fn dir(root: &Path) -> PathBuf {
    root.join(".structdiff")
}

/// The scratch directory, created if needed, for writing into. Refuses a
/// `.structdiff` symlink, which a branch could commit to point writes
/// outside the repo.
fn writable_dir(root: &Path) -> std::io::Result<PathBuf> {
    let d = dir(root);
    if std::fs::symlink_metadata(&d).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(std::io::Error::other(format!("{} is a symlink; refusing to write through it", d.display())));
    }
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// `narrative.json` for the working tree, otherwise `narrative-<spec>.json`
/// with every character outside `[A-Za-z0-9._-]` replaced by `_`.
pub fn filename(spec: &str) -> String {
    if spec.is_empty() {
        return "narrative.json".to_owned();
    }
    let safe: String =
        spec.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    format!("narrative-{safe}.json")
}

pub fn path(root: &Path, spec: &str) -> PathBuf {
    dir(root).join(filename(spec))
}

/// Keep `.structdiff/` out of `git status` without touching .gitignore.
pub fn ensure_excluded(exclude_file: &Path) -> std::io::Result<()> {
    const LINE: &str = "/.structdiff/";
    if let Ok(text) = std::fs::read_to_string(exclude_file)
        && text.lines().any(|l| l == LINE)
    {
        return Ok(());
    }
    if let Some(parent) = exclude_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut fd = std::fs::OpenOptions::new().create(true).append(true).open(exclude_file)?;
    write!(fd, "\n# structdiff.nvim scratch data\n{LINE}\n")
}

#[derive(Serialize)]
struct ExportFile<'a> {
    path: &'a str,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_path: Option<&'a str>,
}

#[derive(Serialize)]
struct ExportGroup<'a> {
    name: &'a str,
    files: Vec<ExportFile<'a>>,
}

#[derive(Serialize)]
struct ExportRange<'a> {
    spec: &'a str,
    base: &'a str,
    target: Option<&'a str>,
}

#[derive(Serialize)]
struct Export<'a> {
    version: u32,
    fingerprint: &'a str,
    range: ExportRange<'a>,
    output: String,
    groups: Vec<ExportGroup<'a>>,
    /// The definitions that produced `groups`, so a later
    /// `structdiff export` outside Neovim groups the same way.
    grouping: &'a Grouping,
}

/// Write `<root>/.structdiff/groups.json` for the skill, first making sure
/// `.structdiff/` is excluded from git. Returns the file's path.
pub fn export_groups(
    repo: &Repo,
    range: &Range,
    groups: &[Group],
    fingerprint: &str,
    grouping: &Grouping,
) -> std::io::Result<PathBuf> {
    // Best effort: in a read-only .git (a sandboxed agent, say) the export
    // still works; .structdiff/ just isn't excluded from git.
    let _ = ensure_excluded(&repo.exclude);
    let root = &repo.root;
    let export = Export {
        version: VERSION,
        fingerprint,
        range: ExportRange { spec: &range.spec, base: &range.base, target: range.target.as_deref() },
        output: format!(".structdiff/{}", filename(&range.spec)),
        groups: groups
            .iter()
            .map(|g| ExportGroup {
                name: &g.name,
                files: g
                    .files
                    .iter()
                    .map(|f| ExportFile { path: &f.path, status: f.status.to_string(), old_path: f.old_path.as_deref() })
                    .collect(),
            })
            .collect(),
        grouping,
    };
    let path = writable_dir(root)?.join("groups.json");
    std::fs::write(&path, serde_json::to_vec(&export)?)?;
    Ok(path)
}

/// The grouping recorded by the last export, if any. Lets `structdiff
/// export` reuse the groups configured in Neovim.
pub fn exported_grouping(root: &Path) -> Option<Grouping> {
    #[derive(serde::Deserialize)]
    struct Previous {
        grouping: Grouping,
    }
    let text = std::fs::read_to_string(dir(root).join("groups.json")).ok()?;
    serde_json::from_str::<Previous>(&text).ok().map(|p| p.grouping)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Narrative {
    pub fingerprint: Option<String>,
    pub overall: String,
    pub groups: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
    pub order: Vec<String>,
}

fn str_map(v: Option<&Value>) -> BTreeMap<String, String> {
    v.and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned()))).collect())
        .unwrap_or_default()
}

impl Narrative {
    /// Keep what has the right shape, drop the rest: a sloppy file degrades
    /// instead of failing.
    pub fn from_value(data: &Value) -> Result<Self, &'static str> {
        let obj = data.as_object().ok_or("not a JSON object")?;
        Ok(Self {
            fingerprint: obj.get("fingerprint").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned),
            overall: obj.get("overall").and_then(Value::as_str).unwrap_or_default().to_owned(),
            groups: str_map(obj.get("groups")),
            // A file reason is one line in the sidebar.
            files: str_map(obj.get("files")).into_iter().map(|(p, r)| (p, r.split_whitespace().collect::<Vec<_>>().join(" "))).collect(),
            order: obj
                .get("order")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
        })
    }

    /// Parse an agent's reply: a JSON object, possibly wrapped in a code fence
    /// or surrounded by stray text.
    pub fn parse_reply(text: &str) -> Result<Self, &'static str> {
        let start = text.find('{').ok_or("no JSON object in the reply")?;
        let end = text.rfind('}').ok_or("no JSON object in the reply")?;
        if end < start {
            return Err("no JSON object in the reply");
        }
        let value: Value = serde_json::from_str(&text[start..=end]).map_err(|_| "the reply's JSON is invalid")?;
        Self::from_value(&value)
    }

    /// The narrative-file JSON for this narrative.
    pub fn to_json(&self) -> String {
        let value = serde_json::json!({
            "version": VERSION,
            "fingerprint": self.fingerprint,
            "overall": self.overall,
            "groups": self.groups,
            "files": self.files,
            "order": self.order,
        });
        serde_json::to_string_pretty(&value).expect("plain JSON values serialize")
    }

    /// Write this narrative as `spec`'s narrative file, creating
    /// `.structdiff/` (and excluding it from git, best effort) if needed.
    pub fn save(&self, repo: &Repo, spec: &str) -> Result<PathBuf> {
        let _ = ensure_excluded(&repo.exclude);
        let path = path(&repo.root, spec);
        writable_dir(&repo.root).map_err(|source| Error::Io { path: dir(&repo.root), source })?;
        std::fs::write(&path, self.to_json()).map_err(|source| Error::Io { path: path.clone(), source })?;
        Ok(path)
    }

    /// Ok(None) when there is no narrative for this range yet.
    pub fn load(root: &Path, spec: &str) -> Result<Option<Self>> {
        let path = path(root, spec);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(Error::Io { path, source }),
        };
        let invalid = |reason| Error::InvalidNarrative { path: path.clone(), reason };
        let value: Value = serde_json::from_str(&text).map_err(|_| invalid("invalid JSON"))?;
        Self::from_value(&value).map(Some).map_err(invalid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    None,
    Fresh,
    Stale,
    Unverified,
}

pub fn state(narrative: Option<&Narrative>, fingerprint: &str) -> State {
    match narrative {
        None => State::None,
        Some(n) => match &n.fingerprint {
            None => State::Unverified,
            Some(fp) if fp == fingerprint => State::Fresh,
            Some(_) => State::Stale,
        },
    }
}

/// Markdown for the narrative split.
pub fn render(narrative: Option<&Narrative>, groups: &[Group], state: State, spec: &str) -> Vec<String> {
    let mut lines = vec![if spec.is_empty() { "# Change narrative".to_owned() } else { format!("# Change narrative: {spec}") }];
    match state {
        State::Stale => {
            lines.push(String::new());
            lines.push("> **Stale:** the diff changed since this was written. Run `:StructDiffGenerate`.".into());
        }
        State::Unverified => {
            lines.push(String::new());
            lines.push("> Written without a fingerprint; it may not match the current diff.".into());
        }
        _ => {}
    }
    let Some(n) = narrative else {
        lines.push(String::new());
        let cmd = format!("/diff-narrative {spec}");
        lines.push(format!("No narrative yet. Run `:StructDiffGenerate` or `{}` in Claude Code.", cmd.trim()));
        return lines;
    };
    if !n.overall.is_empty() {
        lines.push(String::new());
        lines.extend(n.overall.split('\n').map(str::to_owned));
    }
    for g in groups {
        lines.push(String::new());
        lines.push(format!("## {}", one_line(&g.name)));
        if let Some(why) = n.groups.get(&g.name) {
            lines.push(String::new());
            lines.extend(why.split('\n').map(str::to_owned));
        }
        lines.push(String::new());
        for f in &g.files {
            lines.push(match n.files.get(&f.path) {
                Some(reason) => format!("- `{}` — {reason}", one_line(&f.path)),
                None => format!("- `{}`", one_line(&f.path)),
            });
        }
    }
    lines
}

/// `text` made safe to show on one line: newlines and carriage returns
/// (which git allows in file names) are shown escaped. Neovim rejects a
/// buffer line containing a newline.
pub fn one_line(text: &str) -> String {
    text.replace('\n', "\\n").replace('\r', "\\r")
}

/// Greedy word wrap by character count.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            out.push(std::mem::take(&mut line));
            line.push_str(word);
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}
