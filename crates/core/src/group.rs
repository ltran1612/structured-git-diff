//! Regex grouping of changed files and narrative reading order.

use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::git::ChangedFile;

/// A group definition. Patterns use Rust `regex` syntax and are matched
/// against the repo-relative path (unanchored; use ^ and $).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupDef {
    pub name: String,
    #[serde(default)]
    pub patterns: Vec<String>,
}

impl GroupDef {
    pub fn new(name: &str, patterns: &[&str]) -> Self {
        Self { name: name.to_owned(), patterns: patterns.iter().map(|p| (*p).to_owned()).collect() }
    }
}

/// Tried in order; the first match wins.
pub fn default_groups() -> Vec<GroupDef> {
    vec![
        GroupDef::new(
            "Tests",
            &[r"(^|/)(tests?|spec|__tests__)/", r"_test\.\w+$", r"\.(test|spec)\.\w+$", r"(^|/)test_[^/]+\.py$"],
        ),
        GroupDef::new("CI", &[r"^\.github/", r"^\.gitlab-ci", r"^\.circleci/"]),
        GroupDef::new(
            "Config/Build",
            &[
                r"(^|/)(Makefile|CMakeLists\.txt|Dockerfile|Justfile|flake\.nix)$",
                r"(^|/)requirements[^/]*\.txt$",
                r"\.(lock|json|ya?ml|toml|ini|cfg|conf)$",
                r"(^|/)\.[^/]+rc$",
            ],
        ),
        GroupDef::new("Docs", &[r"(^|/)docs?/", r"\.(md|rst|txt|adoc)$", r"(^|/)(README|CHANGELOG|LICENSE)[^/]*$"]),
        GroupDef::new(
            "Source",
            &[r"\.(lua|py|go|rs|js|jsx|mjs|ts|tsx|c|h|cc|cpp|hpp|java|kt|rb|sh|bash|vim|zig|cs|swift|php|ex|exs|hs|ml|scala|sql|html|css|scss|vue|svelte)$"],
        ),
    ]
}

/// The order groups are shown in by default, independent of the order they
/// are matched in (Tests must match before Source, but reads better after).
pub fn default_display_order() -> Vec<String> {
    ["Source", "Tests", "Docs", "Config/Build", "CI"].map(String::from).to_vec()
}

/// How files are grouped: definitions in match-priority order, the fallback
/// group name, and the order groups are displayed in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grouping {
    pub groups: Vec<GroupDef>,
    pub other: String,
    /// Group names in display order. Groups not listed follow in match
    /// order, then the fallback group. A narrative's reading order still
    /// takes precedence.
    #[serde(default = "default_display_order")]
    pub display: Vec<String>,
}

impl Default for Grouping {
    fn default() -> Self {
        Self { groups: default_groups(), other: "Other".to_owned(), display: default_display_order() }
    }
}

impl Grouping {
    /// The grouping for a repo: `<root>/.structdiff.json` `{"groups": [...]}`
    /// replaces `self.groups` when present. Returns a warning when that file
    /// is invalid.
    pub fn for_repo(&self, root: &Path) -> (Grouping, Option<String>) {
        #[derive(Deserialize)]
        struct RepoConfig {
            groups: Vec<GroupDef>,
            display_order: Option<Vec<String>>,
        }
        let path = root.join(".structdiff.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (self.clone(), None);
        };
        match serde_json::from_str::<RepoConfig>(&text) {
            Ok(cfg) => (
                Grouping {
                    groups: cfg.groups,
                    other: self.other.clone(),
                    display: cfg.display_order.unwrap_or_else(|| self.display.clone()),
                },
                None,
            ),
            Err(e) => (self.clone(), Some(format!("ignoring invalid {}: {e}", path.display()))),
        }
    }
}

pub struct Compiled {
    groups: Vec<(String, Vec<Regex>)>,
}

impl Compiled {
    /// Compile definitions. Invalid patterns are skipped and reported.
    pub fn new(defs: &[GroupDef]) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let groups = defs
            .iter()
            .map(|g| {
                let regexes = g
                    .patterns
                    .iter()
                    .filter_map(|p| {
                        Regex::new(p)
                            .map_err(|e| warnings.push(format!("bad pattern in group {:?}: {p} ({e})", g.name)))
                            .ok()
                    })
                    .collect();
                (g.name.clone(), regexes)
            })
            .collect();
        (Self { groups }, warnings)
    }

    pub fn matching(&self, path: &str) -> Option<&str> {
        self.groups.iter().find(|(_, res)| res.iter().any(|re| re.is_match(path))).map(|(n, _)| n.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub files: Vec<ChangedFile>,
}

/// Split files into non-empty groups, in definition order, fallback last.
pub fn assign(files: &[ChangedFile], compiled: &Compiled, other_name: &str) -> Vec<Group> {
    let mut groups: Vec<Group> =
        compiled.groups.iter().map(|(name, _)| Group { name: name.clone(), files: Vec::new() }).collect();
    let mut other = Group { name: other_name.to_owned(), files: Vec::new() };
    for f in files {
        match compiled.matching(&f.path).and_then(|n| groups.iter_mut().find(|g| g.name == n)) {
            Some(g) => g.files.push(f.clone()),
            None => other.files.push(f.clone()),
        }
    }
    groups.push(other);
    groups.retain(|g| !g.files.is_empty());
    groups
}

/// Put groups in display order: names in `display` first, in that order;
/// unlisted groups after them in their current (match) order. The fallback
/// group stays last unless `display` names it.
pub fn sort_display(groups: &mut [Group], display: &[String], other: &str) {
    let rank = |name: &str| match display.iter().position(|d| d == name) {
        Some(i) => (0, i),
        None if name == other => (2, 0),
        None => (1, 0),
    };
    groups.sort_by_key(|g| rank(&g.name)); // stable: unlisted keep match order
}

/// Stable-sort each group's files by their position in `order` (unlisted
/// files keep their relative order after the listed ones), then sort the
/// groups by their earliest file so the story's root cause comes first.
pub fn sort_by_order(groups: &mut [Group], order: &[String]) {
    if order.is_empty() {
        return;
    }
    let rank = |path: &str| order.iter().position(|p| p == path).unwrap_or(usize::MAX);
    for g in groups.iter_mut() {
        g.files.sort_by_key(|f| rank(&f.path)); // sort_by_key is stable
    }
    groups.sort_by_key(|g| g.files.first().map_or(usize::MAX, |f| rank(&f.path)));
}
