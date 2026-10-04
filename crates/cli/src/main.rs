//! `structdiff`: the command-line side of structdiff.nvim, for agents and
//! skills. It computes exactly what the Neovim view would: `export` so a
//! narrative written outside Neovim matches the viewer's fingerprint, and
//! `groups` so a grouping can be checked against the real regex engine.

use std::process::ExitCode;

use structdiff_core::group;
use structdiff_core::{ChangedFile, Error, Grouping, Model, Repo, narrative};

const USAGE: &str = "\
usage: structdiff export [RANGE]
       structdiff groups [--config FILE] [--all]

export: write .structdiff/groups.json for RANGE in the git repo containing
the current directory, and print its path. RANGE is empty (HEAD vs working
tree), a revision (REV vs working tree), A..B, or A...B (what B adds on top
of A).

groups: show how every file in the repo (tracked, plus untracked that git
doesn't ignore) would be grouped. Problems come first: an unreadable or
invalid .structdiff.json, invalid patterns, unknown keys, display_order
names with no group, duplicate or fallback-named groups. Then groups that
match nothing, then each group in display order with sample files (the
fallback group lists up to 50). --config tries a draft .structdiff.json
without installing it; --all lists every file. Exits 1 if there is any
problem.

Groups come from the repo's .structdiff.json if present, otherwise from the
grouping recorded by the last export (e.g. by the Neovim plugin), otherwise
the defaults.";

/// How many files to show per group without --all.
const SAMPLES: usize = 5;

fn export(spec: &str) -> Result<(), Error> {
    let cwd = std::env::current_dir().map_err(|source| Error::Io { path: ".".into(), source })?;
    let repo = Repo::discover(&cwd)?;
    let grouping = narrative::exported_grouping(&repo.root).unwrap_or_default();
    let (model, warnings) = Model::load(repo, spec, &grouping)?;
    for w in warnings {
        eprintln!("structdiff: warning: {w}");
    }
    let path = model.export(&grouping)?;
    emit(&format!("{}\n", path.display()));
    Ok(())
}

/// How many files to show for the fallback group without --all.
const OTHER_SHOWN: usize = 50;

/// Write to stdout, ending quietly if the reader went away (`| head`).
fn emit(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes()).and_then(|()| out.flush());
}

/// `structdiff groups`. Returns false when the grouping has a problem the
/// user must fix: an unreadable or invalid config, an invalid pattern, or a
/// mistake reported by `Grouping::problems`.
fn groups(config: Option<&str>, all: bool) -> Result<bool, Error> {
    use std::fmt::Write as _;
    let cwd = std::env::current_dir().map_err(|source| Error::Io { path: ".".into(), source })?;
    let repo = Repo::discover(&cwd)?;
    let base = narrative::exported_grouping(&repo.root).unwrap_or_default();
    let mut problems: Vec<String> = Vec::new();
    let (grouping, source): (Grouping, String) = match config {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|source| Error::Io { path: path.into(), source })?;
            match base.with_repo_config(&text) {
                Ok(g) => (g, format!("{path} (draft)")),
                Err(e) => {
                    eprintln!("structdiff: invalid config {path}: {e}");
                    return Ok(false);
                }
            }
        }
        None => {
            let (g, warning) = base.for_repo(&repo.root);
            match warning {
                Some(w) => {
                    problems.push(w);
                    (g, "the built-in defaults (.structdiff.json was ignored, see above)".to_owned())
                }
                None if repo.root.join(".structdiff.json").exists() => (g, ".structdiff.json".to_owned()),
                None if narrative::exported_grouping(&repo.root).is_some() => {
                    (g, "the last export (.structdiff/groups.json)".to_owned())
                }
                None => (g, "the built-in defaults".to_owned()),
            }
        }
    };

    let files: Vec<ChangedFile> = repo.all_files()?.iter().map(|p| ChangedFile::new('M', p)).collect();
    let (grouped, invalid) = group::arrange(&files, &grouping);
    problems.extend(invalid);
    problems.extend(grouping.problems());
    let empty: Vec<&str> = grouping
        .groups
        .iter()
        .map(|d| d.name.as_str())
        .filter(|name| !grouped.iter().any(|g| g.name == *name))
        .collect();

    // Problems first, so they survive output truncation.
    let mut out = format!("grouping from {source}; {} files\n", files.len());
    for p in &problems {
        let _ = writeln!(out, "problem: {p}");
    }
    if !empty.is_empty() {
        let _ = writeln!(out, "match nothing in this repo: {}", empty.join(", "));
    }
    for g in &grouped {
        let _ = writeln!(out, "\n{} ({})", g.name, g.files.len());
        let limit = if all { usize::MAX } else if g.name == grouping.other { OTHER_SHOWN } else { SAMPLES };
        for f in g.files.iter().take(limit) {
            let _ = writeln!(out, "  {}", f.path);
        }
        if g.files.len() > limit {
            let _ = writeln!(out, "  … {} more (--all lists them)", g.files.len() - limit);
        }
    }
    emit(&out);
    Ok(problems.is_empty())
}

enum Command {
    Export(String),
    Groups { config: Option<String>, all: bool },
    Help,
}

fn parse(args: &[String]) -> Option<Command> {
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["export"] => Some(Command::Export(String::new())),
        ["export", spec] => Some(Command::Export(spec.to_owned())),
        ["-h" | "--help" | "help"] => Some(Command::Help),
        ["groups", ref rest @ ..] => {
            let (mut config, mut all) = (None, false);
            let mut it = rest.iter();
            while let Some(arg) = it.next() {
                match *arg {
                    "--all" if !all => all = true,
                    "--config" if config.is_none() => match it.next() {
                        Some(path) if !path.starts_with("--") => config = Some((*path).to_owned()),
                        _ => return None,
                    },
                    _ => return None,
                }
            }
            Some(Command::Groups { config, all })
        }
        _ => None,
    }
}

fn main() -> ExitCode {
    let Some(args) = std::env::args_os().skip(1).map(|a| a.into_string().ok()).collect::<Option<Vec<String>>>() else {
        eprintln!("structdiff: arguments must be valid UTF-8\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let result = match parse(&args) {
        Some(Command::Export(spec)) => export(&spec),
        Some(Command::Groups { config, all }) => match groups(config.as_deref(), all) {
            Ok(true) => return ExitCode::SUCCESS,
            Ok(false) => return ExitCode::FAILURE,
            Err(e) => Err(e),
        },
        Some(Command::Help) => {
            emit(&format!("{USAGE}\n"));
            return ExitCode::SUCCESS;
        }
        None => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("structdiff: {e}");
            ExitCode::FAILURE
        }
    }
}
