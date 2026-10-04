//! `structdiff`: the command-line side of structdiff.nvim, for the
//! diff-narrative skill. It computes exactly what the Neovim view would, so a
//! narrative written from a plain Claude Code session matches the viewer's
//! fingerprint.

use std::process::ExitCode;

use structdiff_core::{Model, Repo, narrative};

const USAGE: &str = "\
usage: structdiff export [RANGE]

Write .structdiff/groups.json for RANGE in the git repo containing the
current directory, and print its path. RANGE is empty (HEAD vs working
tree), a revision (REV vs working tree), A..B, or A...B (what B adds on top
of A).

Groups come from the repo's .structdiff.json if present, otherwise from the
grouping recorded by the last export (e.g. by the Neovim plugin), otherwise
the defaults.";

fn export(spec: &str) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let repo = Repo::discover(&cwd).map_err(|e| format!("not a git repository: {e}"))?;
    let grouping = narrative::exported_grouping(&repo.root).unwrap_or_default();
    let (model, warnings) = Model::load(repo, spec, &grouping)?;
    for w in warnings {
        eprintln!("structdiff: warning: {w}");
    }
    let path = model.export(&grouping)?;
    println!("{}", path.display());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["export"] => export(""),
        ["export", spec] => export(spec),
        ["-h" | "--help" | "help"] => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
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
