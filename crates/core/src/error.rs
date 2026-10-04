//! Errors from the core library. Each failure callers may want to handle
//! differently gets its own variant; `Display` gives the user-facing message.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    /// The `git` executable could not be started.
    GitUnavailable(std::io::Error),
    /// A git command exited with an error.
    Git { args: String, stderr: String },
    /// The directory is not inside a git work tree (git's message).
    NotARepo(String),
    /// A revision in the range doesn't name a commit.
    UnknownRevision(String),
    /// `A...B` was given but A and B share no history.
    NoMergeBase { a: String, b: String },
    /// A narrative file exists but can't be used.
    InvalidNarrative { path: PathBuf, reason: &'static str },
    /// Reading or writing a file failed.
    Io { path: PathBuf, source: std::io::Error },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::GitUnavailable(e) => write!(f, "cannot run git: {e}"),
            Error::Git { args, stderr } => write!(f, "git {args} failed: {stderr}"),
            Error::NotARepo(msg) => write!(f, "not a git repository: {msg}"),
            Error::UnknownRevision(rev) => write!(f, "unknown revision: {rev}"),
            Error::NoMergeBase { a, b } => write!(f, "no merge base between {a} and {b}"),
            Error::InvalidNarrative { path, reason } => write!(f, "{reason} in {}", path.display()),
            Error::Io { path, source } => write!(f, "cannot access {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::GitUnavailable(e) | Error::Io { source: e, .. } => Some(e),
            _ => None,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
