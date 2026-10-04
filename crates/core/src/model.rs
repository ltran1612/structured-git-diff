//! The loaded change set: range, files, groups, narrative and display order.
//! Everything the viewer shows, without any UI. Loading only reads: nothing
//! is written to the repo until [`Model::export`].

use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::git::{ChangedFile, Range, Repo};
use crate::group::{self, Compiled, Group, Grouping};
use crate::narrative::{self, Narrative, State};

#[derive(Clone)]
pub struct Model {
    pub repo: Repo,
    pub spec: String,
    pub range: Range,
    pub files: Vec<ChangedFile>,
    pub fingerprint: String,
    pub groups: Vec<Group>,
    pub narrative: Option<Narrative>,
    pub state: State,
    /// Display order, (group index, file index) per file, and each path's
    /// position in it. Rebuilt whenever `groups` is.
    order: Vec<(usize, usize)>,
    position: HashMap<String, usize>,
}

impl Model {
    /// Resolve `spec`, scan the change set and load its narrative. Returns
    /// non-fatal warnings alongside the model.
    pub fn load(repo: Repo, spec: &str, grouping: &Grouping) -> Result<(Self, Vec<String>)> {
        let spec = spec.trim().to_owned();
        let range = repo.resolve_range(&spec)?;
        let mut model = Self {
            repo,
            spec,
            range,
            files: Vec::new(),
            fingerprint: String::new(),
            groups: Vec::new(),
            narrative: None,
            state: State::None,
            order: Vec::new(),
            position: HashMap::new(),
        };
        let warnings = model.rescan(grouping)?;
        Ok((model, warnings))
    }

    /// Re-resolve the range (branches move) and rescan.
    pub fn rescan(&mut self, grouping: &Grouping) -> Result<Vec<String>> {
        self.range = self.repo.resolve_range(&self.spec)?;
        let scan = self.repo.scan(&self.range)?;
        self.files = scan.files;
        self.fingerprint = scan.fingerprint;
        Ok(self.reload_narrative(grouping))
    }

    /// Reload only the narrative file and regroup by its reading order.
    pub fn reload_narrative(&mut self, grouping: &Grouping) -> Vec<String> {
        let mut warnings = Vec::new();
        self.narrative = match Narrative::load(&self.repo.root, &self.spec) {
            Ok(n) => n,
            Err(e) => {
                warnings.push(e.to_string());
                None
            }
        };
        self.state = narrative::state(self.narrative.as_ref(), &self.fingerprint);
        let (grouping, warning) = grouping.for_repo(&self.repo.root);
        warnings.extend(warning);
        let (compiled, bad) = Compiled::new(&grouping.groups);
        warnings.extend(bad);
        self.groups = group::assign(&self.files, &compiled, &grouping.other);
        group::sort_display(&mut self.groups, &grouping.display, &grouping.other);
        if let Some(n) = &self.narrative {
            group::sort_by_order(&mut self.groups, &n.order);
        }
        self.index_order();
        warnings
    }

    fn index_order(&mut self) {
        self.order = self
            .groups
            .iter()
            .enumerate()
            .flat_map(|(gi, g)| (0..g.files.len()).map(move |fi| (gi, fi)))
            .collect();
        self.position = self.order.iter().enumerate().map(|(i, &(gi, fi))| (self.groups[gi].files[fi].path.clone(), i)).collect();
    }

    /// Write `.structdiff/groups.json` for the diff-narrative skill (and
    /// exclude `.structdiff/` from git). The only write to the repo.
    ///
    /// `grouping` is the base grouping (the Neovim config), before the
    /// repo's `.structdiff.json` is applied. That base is what gets recorded,
    /// so `structdiff export` and `structdiff groups` start from the same
    /// place as the viewer and apply `.structdiff.json` themselves. Recording
    /// the merged grouping would let a deleted or branch-local
    /// `.structdiff.json` keep controlling them.
    pub fn export(&self, grouping: &Grouping) -> Result<PathBuf> {
        narrative::export_groups(&self.repo, &self.range, &self.groups, &self.fingerprint, grouping)
            .map_err(|source| Error::Io { path: narrative::dir(&self.repo.root).join("groups.json"), source })
    }

    /// Turn a generator agent's reply into this change set's narrative. The
    /// fingerprint is ours, not the agent's, and anything about files or
    /// groups outside this change set is dropped, so a reply can only
    /// describe what is actually here.
    pub fn adopt_reply(&self, reply: &str) -> std::result::Result<Narrative, &'static str> {
        let mut n = Narrative::parse_reply(reply)?;
        n.fingerprint = Some(self.fingerprint.clone());
        let paths: std::collections::HashSet<&str> = self.files.iter().map(|f| f.path.as_str()).collect();
        n.files.retain(|p, _| paths.contains(p.as_str()));
        n.order.retain(|p| paths.contains(p.as_str()));
        n.groups.retain(|name, _| self.groups.iter().any(|g| &g.name == name));
        Ok(n)
    }

    /// Display order of all files: (group index, file index). `]f` walks it.
    pub fn flat(&self) -> &[(usize, usize)] {
        &self.order
    }

    pub fn file(&self, (gi, fi): (usize, usize)) -> &ChangedFile {
        &self.groups[gi].files[fi]
    }

    /// Position of `path` in the display order.
    pub fn index_of(&self, path: &str) -> Option<usize> {
        self.position.get(path).copied()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn narrative_filename(&self) -> String {
        narrative::filename(&self.spec)
    }
}
