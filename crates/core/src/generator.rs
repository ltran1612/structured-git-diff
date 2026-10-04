//! The fallback narrative generator: which agent CLI to run when nobody
//! narrated a change, and the exact command for it.
//!
//! Normally the agent that made a change writes its narrative (with the
//! diff-narrative skill) while it still knows why. These commands are for
//! everything else: a fresh agent reconstructs the "why" from the diff. All
//! three get the same instructions, the skill's text, built into the binary
//! so the skill doesn't have to be installed.

use serde::{Deserialize, Serialize};

const SKILL: &str = include_str!("../../../skill/diff-narrative/SKILL.md");

/// An agent CLI that can write narratives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Generator {
    /// Claude Code: `claude -p`.
    #[default]
    Claude,
    /// OpenAI Codex CLI: `codex exec`.
    Codex,
    /// GitHub Copilot CLI: `copilot -p`.
    Copilot,
}

impl Generator {
    /// Command template. `{prompt}`, `{range}` and `{output}` are filled in
    /// by [`expand`]. Each grants only what the task needs: git, `structdiff
    /// export`, reading, and writing the narrative file.
    pub fn template(self) -> Vec<String> {
        let args: &[&str] = match self {
            Generator::Claude => &[
                "claude",
                "-p",
                "{prompt}",
                "--permission-mode",
                "acceptEdits",
                "--allowedTools",
                "Bash(git:*)",
                "Bash(structdiff export:*)",
                "Read",
                "Write",
            ],
            // workspace-write: may write inside the repo (where .structdiff/
            // lives), not elsewhere; commands run sandboxed without prompts.
            Generator::Codex => &["codex", "exec", "--sandbox", "workspace-write", "--ephemeral", "{prompt}"],
            Generator::Copilot => &[
                "copilot",
                "-p",
                "{prompt}",
                "-s",
                "--no-ask-user",
                "--allow-tool=shell(git:*), shell(structdiff:*), write({output})",
            ],
        };
        args.iter().map(|a| (*a).to_owned()).collect()
    }
}

/// The skill's instructions for `spec`, without its YAML front matter.
pub fn prompt(spec: &str) -> String {
    let body = match SKILL.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---\n")) {
        Some((_front_matter, body)) => body,
        None => SKILL,
    };
    body.trim_start().replace("$ARGUMENTS", spec)
}

/// Fill `{prompt}`, `{range}` and `{output}` into each argument. An argument
/// that was only `{range}` and is empty stays empty; others are trimmed so
/// `"/diff-narrative {range}"` becomes `"/diff-narrative"` for the working
/// tree.
pub fn expand(template: &[String], spec: &str, output: &str) -> Vec<String> {
    template
        .iter()
        .map(|arg| {
            let mut out = arg.clone();
            if out.contains("{range}") {
                out = out.replace("{range}", spec).trim().to_owned();
            }
            out = out.replace("{output}", output);
            if out.contains("{prompt}") {
                out = out.replace("{prompt}", &prompt(spec));
            }
            out
        })
        .collect()
}
