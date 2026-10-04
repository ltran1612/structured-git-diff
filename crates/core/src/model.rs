//! The loaded change set: range, files, groups, narrative and display order.
//! Everything the viewer shows, without any UI.

use crate::git::{ChangedFile, Range, Repo};
use crate::group::{self, Compiled, Group, GroupDef};
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
    defs: Vec<GroupDef>,
    other: String,
}

impl Model {
    /// Resolve `spec`, scan the change set, load its narrative and write
    /// groups.json. Returns non-fatal warnings alongside the model.
    pub fn load(repo: Repo, spec: &str, defs: &[GroupDef], other: &str) -> Result<(Self, Vec<String>), String> {
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
            defs: defs.to_vec(),
            other: other.to_owned(),
        };
        let warnings = model.rescan()?;
        Ok((model, warnings))
    }

    /// Re-resolve the range (branches move), rescan and re-export.
    pub fn rescan(&mut self) -> Result<Vec<String>, String> {
        self.range = self.repo.resolve_range(&self.spec)?;
        let mut warnings = Vec::new();
        if let Err(e) = narrative::ensure_excluded(&self.repo.exclude) {
            warnings.push(format!("cannot update {}: {e}", self.repo.exclude.display()));
        }
        let scan = self.repo.scan(&self.range)?;
        self.files = scan.files;
        self.fingerprint = scan.fingerprint;
        warnings.extend(self.reload_narrative());
        if let Err(e) = narrative::export_groups(&self.repo.root, &self.range, &self.groups, &self.fingerprint) {
            warnings.push(format!("cannot write groups.json: {e}"));
        }
        Ok(warnings)
    }

    /// Reload only the narrative file and regroup by its reading order.
    pub fn reload_narrative(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        self.narrative = match Narrative::load(&self.repo.root, &self.spec) {
            Ok(n) => n,
            Err(e) => {
                warnings.push(e);
                None
            }
        };
        self.state = narrative::state(self.narrative.as_ref(), &self.fingerprint);
        let (defs, warning) = group::groups_for(&self.repo.root, &self.defs);
        warnings.extend(warning);
        let (compiled, bad) = Compiled::new(&defs);
        warnings.extend(bad);
        self.groups = group::assign(&self.files, &compiled, &self.other);
        if let Some(n) = &self.narrative {
            group::sort_by_order(&mut self.groups, &n.order);
        }
        warnings
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
