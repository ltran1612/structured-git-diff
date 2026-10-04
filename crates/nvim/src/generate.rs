//! `:StructDiffGenerate`: run the diff-narrative skill in the background and
//! show its narrative when it lands. The command can be cancelled
//! (`:StructDiffCancel`), times out after `generate_timeout` seconds, and is
//! stopped along with its child processes in either case. On Linux it is also
//! stopped if Neovim exits.

use std::cell::RefCell;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::actions::{self, open_then, refresh_then};
use crate::state::{self, with_view};
use crate::{bg, ui};

thread_local! {
    /// Set to ask the running generation to stop.
    static CANCEL: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

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

/// Stop a running generation.
pub fn cancel_generate() {
    match CANCEL.with(|c| c.borrow().clone()) {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            ui::notify("cancelling narrative generation…", ui::INFO);
        }
        None => ui::notify("no narrative generation is running", ui::INFO),
    }
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
    let timeout = (cfg.generate_timeout > 0).then(|| Duration::from_secs(cfg.generate_timeout));
    let cancel = Arc::new(AtomicBool::new(false));
    CANCEL.with(|c| *c.borrow_mut() = Some(cancel.clone()));
    ui::notify("generating narrative…", ui::INFO);
    let spawned = bg::spawn(
        move || {
            // Hand the skill the current change set and these groups. This is
            // the only place the plugin writes to the repo.
            model.rescan(&grouping).map_err(|e| e.to_string())?;
            model.export(&grouping).map_err(|e| e.to_string())?;
            run(&cmd, &model.repo.root, &cancel, timeout)
        },
        finished,
    );
    if let Err(e) = spawned {
        CANCEL.with(|c| c.borrow_mut().take());
        with_view(|v| {
            v.set_generating(false);
            v.redraw(&cfg);
        });
        ui::notify(&e, ui::ERROR);
    }
}

/// Start `cmd` in its own process group (so stopping it also stops whatever
/// it spawned) and, on Linux, have the kernel stop it if this thread, and so
/// Neovim, goes away.
fn spawn_child(cmd: &[String], root: &Path) -> std::io::Result<Child> {
    let mut command = Command::new(&cmd[0]);
    command.args(&cmd[1..]).current_dir(root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        #[cfg(target_os = "linux")]
        // SAFETY: prctl is async-signal-safe, as pre_exec requires.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    command.spawn()
}

/// SIGTERM the child's process group, give it two seconds, then SIGKILL.
fn stop(child: &mut Child) {
    #[cfg(unix)]
    // SAFETY: plain syscall; a negative pid addresses the process group that
    // spawn_child created for this child.
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Drain a pipe on its own thread so a chatty command can't fill it and block.
fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    })
}

fn run(cmd: &[String], root: &Path, cancel: &AtomicBool, timeout: Option<Duration>) -> Result<(), String> {
    let mut child = spawn_child(cmd, root).map_err(|e| format!("cannot run {}: {e}", cmd[0]))?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let started = Instant::now();
    let status: ExitStatus = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if cancel.load(Ordering::Relaxed) {
            stop(&mut child);
            return Err("narrative generation cancelled".into());
        }
        if let Some(limit) = timeout
            && started.elapsed() >= limit
        {
            stop(&mut child);
            return Err(format!("narrative generation timed out after {}s", limit.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let (out, err) = (stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    if status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(if err.is_empty() { &out } else { &err });
    let tail: String = err.trim().chars().rev().take(500).collect::<Vec<_>>().into_iter().rev().collect();
    Err(format!("narrative generation failed: {tail}"))
}

fn finished(res: Result<(), String>) {
    CANCEL.with(|c| c.borrow_mut().take());
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
