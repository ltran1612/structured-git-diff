//! User options, deserialized from the table passed to `setup()`.

use nvim_oxi::Object;
use serde::Deserialize;
use structdiff_core::{GroupDef, Generator, Grouping, generator, narrative};
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
    /// Agent CLI that :StructDiffGenerate runs when no narrative was written
    /// by the agent that made the change: "claude", "codex" or "copilot".
    pub generator: Generator,
    /// Full override of the generator's command. Run from the repo root;
    /// "{prompt}" becomes the narrative instructions, "{range}" the range
    /// spec ("" for the working tree), "{output}" the narrative file's path
    /// relative to the root. Empty means use `generator`'s command.
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
            generator: Generator::default(),
            generate_cmd: Vec::new(),
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

    /// True when `generate_cmd` overrides the generator.
    pub fn custom_generate_cmd(&self) -> bool {
        !self.generate_cmd.is_empty()
    }

    /// The command to run for `spec` with `prompt`: `generate_cmd` if set,
    /// otherwise the `generator`'s, with placeholders filled in.
    pub fn generate_cmd_for(&self, spec: &str, prompt: &str) -> Vec<String> {
        let template = if self.custom_generate_cmd() { self.generate_cmd.clone() } else { self.generator.template() };
        let output = format!(".structdiff/{}", narrative::filename(spec));
        generator::expand(&template, spec, &output, prompt)
    }
}
