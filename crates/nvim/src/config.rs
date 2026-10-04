//! User options, deserialized from the table passed to `setup()`.

use nvim_oxi::Object;
use serde::Deserialize;
use structdiff_core::{GroupDef, Grouping};
use structdiff_core::group::{default_display_order, default_groups};

/// One key or several: `"]g"` or `{ "za", "<Tab>" }`.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Keys {
    One(String),
    Many(Vec<String>),
}

impl Keys {
    pub fn list(&self) -> Vec<&str> {
        match self {
            Keys::One(k) => vec![k.as_str()],
            Keys::Many(ks) => ks.iter().map(String::as_str).collect(),
        }
    }
}

fn one(k: &str) -> Keys {
    Keys::One(k.to_owned())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Keymaps {
    pub next_group: Keys,
    pub prev_group: Keys,
    pub next_file: Keys,
    pub prev_file: Keys,
    pub select: Keys,
    pub toggle_fold: Keys,
    pub toggle_narrative: Keys,
    pub toggle_reasons: Keys,
    pub refresh: Keys,
    pub close: Keys,
}

impl Default for Keymaps {
    fn default() -> Self {
        Self {
            next_group: one("]g"),
            prev_group: one("[g"),
            next_file: one("]f"),
            prev_file: one("[f"),
            select: one("<CR>"),
            toggle_fold: Keys::Many(vec!["za".into(), "<Tab>".into()]),
            toggle_narrative: one("gn"),
            toggle_reasons: one("gr"),
            refresh: one("R"),
            close: one("q"),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Rust `regex` patterns against repo-relative paths; first match wins.
    pub groups: Vec<GroupDef>,
    pub other_group: String,
    /// Group names in the order they are shown (match order is `groups`).
    pub display_order: Vec<String>,
    pub sidebar_width: u32,
    pub narrative_height: u32,
    pub show_reasons: bool,
    /// Seconds before :StructDiffGenerate gives up and stops the command;
    /// 0 waits forever.
    pub generate_timeout: u64,
    /// Run by :StructDiffGenerate from the repo root. "{range}" becomes the
    /// range spec ("" for the working tree).
    pub generate_cmd: Vec<String>,
    pub keymaps: Keymaps,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            groups: default_groups(),
            other_group: "Other".into(),
            display_order: default_display_order(),
            sidebar_width: 40,
            narrative_height: 15,
            show_reasons: true,
            generate_timeout: 600,
            generate_cmd: [
                "claude",
                "-p",
                "/diff-narrative {range}",
                "--permission-mode",
                "acceptEdits",
                "--allowedTools",
                "Bash(git:*)",
                "Bash(structdiff export:*)",
                "Read",
                "Write",
            ]
            .map(String::from)
            .to_vec(),
            keymaps: Keymaps::default(),
        }
    }
}

impl Config {
    /// Parse `setup()` options. nil and `{}` mean defaults; lists such as
    /// `groups` replace the defaults rather than merging by index.
    pub fn from_object(obj: Object) -> Result<Self, String> {
        let empty = match obj.kind() {
            nvim_oxi::ObjectKind::Nil => true,
            nvim_oxi::ObjectKind::Array => unsafe { obj.as_array_unchecked() }.is_empty(),
            nvim_oxi::ObjectKind::Dictionary => unsafe { obj.as_dictionary_unchecked() }.is_empty(),
            _ => false,
        };
        if empty {
            return Ok(Self::default());
        }
        Self::deserialize(nvim_oxi::serde::Deserializer::new(obj)).map_err(|e| format!("invalid setup options: {e}"))
    }

    pub fn grouping(&self) -> Grouping {
        Grouping { groups: self.groups.clone(), other: self.other_group.clone(), display: self.display_order.clone() }
    }

    /// generate_cmd with "{range}" replaced by `spec`.
    pub fn generate_cmd_for(&self, spec: &str) -> Vec<String> {
        self.generate_cmd
            .iter()
            .map(|a| if a.contains("{range}") { a.replace("{range}", spec).trim().to_owned() } else { a.clone() })
            .collect()
    }
}
