//! `:StructDiffGenerate`: run the diff-narrative skill in the background and
//! show its narrative when it lands.

use crate::actions::{self, open_then, refresh_then};
use crate::state::{self, with_view};
use crate::{bg, ui};

/// Run the diff-narrative skill (config.generate_cmd) in the background and
/// reload when it finishes. With `spec`, (re)opens the view on that range.
pub fn generate(spec: Option<String>) {
    let spec = spec.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let open_spec = with_view(|v| v.model().spec.clone());
    if open_spec.is_none() || (spec.is_some() && spec != open_spec) {
        return open_then(spec.unwrap_or_default(), Some(Box::new(start)));
    }
    start();
}

fn start() {
    let cfg = state::config();
    let started = with_view(|v| {
        if v.generating() {
            ui::notify("already generating", ui::INFO);
            return None;
        }
        v.set_generating(true);
        v.redraw(&cfg);
        Some((v.model().clone(), cfg.generate_cmd_for(&v.model().spec)))
    })
    .flatten();
    let Some((mut model, cmd)) = started else { return };
    let grouping = cfg.grouping();
    ui::notify("generating narrative…", ui::INFO);
    let spawned = bg::spawn(
        move || {
            // Hand the skill the current change set and these groups. This is
            // the only place the plugin writes to the repo.
            model.rescan(&grouping)?;
            model.export(&grouping)?;
            run(&cmd, &model.repo.root)
        },
        finished,
    );
    if let Err(e) = spawned {
        with_view(|v| {
            v.set_generating(false);
            v.redraw(&cfg);
        });
        ui::notify(&e, ui::ERROR);
    }
}

fn run(cmd: &[String], root: &std::path::Path) -> Result<(), String> {
    let out = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {}: {e}", cmd[0]))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr });
    let tail: String = err.trim().chars().rev().take(500).collect::<Vec<_>>().into_iter().rev().collect();
    Err(format!("narrative generation failed: {tail}"))
}

fn finished(res: Result<(), String>) {
    match &res {
        Ok(()) => ui::notify("narrative ready", ui::INFO),
        Err(e) => ui::notify(e, ui::ERROR),
    }
    with_view(|v| v.set_generating(false));
    refresh_then(Some(Box::new(|| {
        let cfg = state::config();
        with_view(|v| {
            if v.model().narrative.is_some() {
                actions::open_narrative(v, &cfg);
            }
        });
    })));
}
