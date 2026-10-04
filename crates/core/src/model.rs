//! The loaded change set: range, files, groups, narrative and display order.
//! Everything the viewer shows, without any UI. Loading only reads: nothing
//! is written to the repo until [`Model::export`].

use std::path::PathBuf;

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
}

impl Model {
    /// Resolve `spec`, scan the change set and load its narrative. Returns
    /// non-fatal warnings alongside the model.
    pub fn load(repo: Repo, spec: &str, grouping: &Grouping) -> Result<(Self, Vec<String>), String> {
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
        };
        let warnings = model.rescan(grouping)?;
        Ok((model, warnings))
    }

    /// Re-resolve the range (branches move) and rescan.
    pub fn rescan(&mut self, grouping: &Grouping) -> Result<Vec<String>, String> {
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
                warnings.push(e);
                None
            }
        };
        self.state = narrative::state(self.narrative.as_ref(), &self.fingerprint);
        let (grouping, warning) = grouping.for_repo(&self.repo.root);
        warnings.extend(warning);
        let (compiled, bad) = Compiled::new(&grouping.groups);
        warnings.extend(bad);
        self.groups = group::assign(&self.files, &compiled, &grouping.other);
        if let Some(n) = &self.narrative {
            group::sort_by_order(&mut self.groups, &n.order);
        }
        warnings
    }

    /// Write `.structdiff/groups.json` for the diff-narrative skill (and
    /// exclude `.structdiff/` from git). The only write to the repo.
    pub fn export(&self, grouping: &Grouping) -> Result<PathBuf, String> {
        let (grouping, _) = grouping.for_repo(&self.repo.root);
        narrative::export_groups(&self.repo, &self.range, &self.groups, &self.fingerprint, &grouping)
            .map_err(|e| format!("cannot write {}: {e}", narrative::dir(&self.repo.root).join("groups.json").display()))
    }

    /// Display order of all files: (group index, file index). `]f` walks it.
    pub fn flat(&self) -> Vec<(usize, usize)> {
        self.groups
            .iter()
            .enumerate()
            .flat_map(|(gi, g)| (0..g.files.len()).map(move |fi| (gi, fi)))
            .collect()
    }

    pub fn file(&self, (gi, fi): (usize, usize)) -> &ChangedFile {
        &self.groups[gi].files[fi]
    }

    pub fn index_of(&self, path: &str) -> Option<usize> {
        self.flat().iter().position(|&pos| self.file(pos).path == path)
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn narrative_filename(&self) -> String {
        narrative::filename(&self.spec)
    }
}
