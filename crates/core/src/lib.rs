//! structdiff's editor-independent core: git access, grouping, and the
//! narrative file contract. No Neovim dependency, so it tests with plain
//! `cargo test`.

pub mod error;
pub mod git;
pub mod group;
pub mod model;
pub mod narrative;

pub use error::Error;
pub use git::{ChangedFile, Range, Repo};
pub use group::{Group, GroupDef, Grouping};
pub use model::Model;
pub use narrative::{Narrative, State};
