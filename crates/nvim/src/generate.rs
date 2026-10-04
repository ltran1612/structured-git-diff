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

/// The one narrative generation that may run at a time.
struct Job {
    spec: String,
    /// Set to ask it to stop.
    cancel: Arc<AtomicBool>,
}

thread_local! {
    static JOB: RefCell<Option<Job>> = const { RefCell::new(None) };
}

/// The fallback for changes nobody narrated: run the configured agent CLI
/// (`generator`, or `generate_cmd`) with the diff-narrative instructions in
/// the background, and reload when it finishes. With `spec`, (re)opens the
/// view on that range.
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
    let running = JOB.with(|j| j.borrow().as_ref().map(|job| job.cancel.clone()));
    match running {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            ui::notify("cancelling narrative generation…", ui::INFO);
        }
        None => ui::notify("no narrative generation is running", ui::INFO),
    }
}

fn start() {
    if let Some(spec) = JOB.with(|j| j.borrow().as_ref().map(|job| job.spec.clone())) {
        let what = if spec.is_empty() { "the working tree".to_owned() } else { spec };
        return ui::notify(&format!("already generating a narrative for {what} (:StructDiffCancel stops it)"), ui::INFO);
    }
    let cfg = state::config();
    let started = with_view(|v| {
        v.set_generating(true);
        v.redraw(&cfg);
        Some((v.id(), v.model().clone()))
    })
    .flatten();
    let Some((view_id, mut model)) = started else { return };
    let grouping = cfg.grouping();
    let custom = cfg.custom_generate_cmd();
    let command_cfg = cfg.clone();
    let timeout = (cfg.generate_timeout > 0).then(|| Duration::from_secs(cfg.generate_timeout));
    let cancel = Arc::new(AtomicBool::new(false));
    JOB.with(|j| *j.borrow_mut() = Some(Job { spec: model.spec.clone(), cancel: cancel.clone() }));
    ui::notify("generating narrative…", ui::INFO);
    let spawned = bg::spawn(
        move || {
            model.rescan(&grouping).map_err(|e| e.to_string())?;
            if custom {
                // Custom commands may follow the skill, which reads groups.json.
                model.export(&grouping).map_err(|e| e.to_string())?;
            }
            let prompt = structdiff_core::generator::prompt(&model);
            let cmd = command_cfg.generate_cmd_for(&model.spec, &prompt);
            let file = structdiff_core::narrative::path(&model.repo.root, &model.spec);
            let before = modified(&file);
            let reply = run(&cmd, &model.repo.root, &cancel, timeout)?;
            // The agent only answers; validating the reply and writing the
            // file is ours. A custom command may write the file itself.
            match model.adopt_reply(&reply) {
                Ok(narrative) => narrative.save(&model.repo, &model.spec).map(|_| ()).map_err(|e| e.to_string()),
                Err(_) if modified(&file) != before => Ok(()),
                Err(reason) => {
                    let snippet: String = reply.trim().chars().take(200).collect();
                    Err(format!("the agent's reply wasn't a narrative ({reason}): {snippet}"))
                }
            }
        },
        move |res| finished(view_id, res),
    );
    if let Err(e) = spawned {
        JOB.with(|j| j.borrow_mut().take());
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

/// Send `sig` to the whole process group `spawn_child` created.
#[cfg(unix)]
fn signal_group(pgid: u32, sig: libc::c_int) -> bool {
    // SAFETY: plain syscall; a negative pid addresses the process group.
    unsafe { libc::kill(-(pgid as libc::pid_t), sig) == 0 }
}

/// Wait until `done()` or `limit` passes; true if done.
fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Stop every process in the command's group: SIGTERM, then SIGKILL for
/// anything still alive two seconds later (including members that ignore
/// SIGTERM). Reaps the leader.
fn stop(child: &mut Child) {
    let pgid = child.id();
    #[cfg(unix)]
    {
        signal_group(pgid, libc::SIGTERM);
        let all_gone = wait_for(Duration::from_secs(2), || {
            let _ = child.try_wait();
            !signal_group(pgid, 0)
        });
        if !all_gone {
            signal_group(pgid, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = (pgid, child.kill());
    let _ = child.wait();
}

/// After the command exits, stop anything it left running in its group
/// (say, `sleep 30 &`). Such a process also holds the output pipes open.
fn stop_leftovers(pgid: u32) {
    #[cfg(unix)]
    if signal_group(pgid, 0) {
        signal_group(pgid, libc::SIGTERM);
        if !wait_for(Duration::from_secs(1), || !signal_group(pgid, 0)) {
            signal_group(pgid, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = pgid;
}

/// Read a pipe on its own thread into a shared buffer, so a chatty command
/// can't fill the pipe and block, and the reader can be abandoned (keeping
/// what it read) if something outside the group still holds the pipe open.
struct Drain {
    data: Arc<std::sync::Mutex<Vec<u8>>>,
    done: std::sync::mpsc::Receiver<()>,
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> Drain {
    let data = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (tx, done) = std::sync::mpsc::channel();
    let sink = data.clone();
    std::thread::spawn(move || {
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(&chunk[..n]);
            }
        }
        let _ = tx.send(());
    });
    Drain { data, done }
}

impl Drain {
    /// Everything read, waiting at most `grace` for the pipe to close.
    fn finish(self, grace: Duration) -> Vec<u8> {
        let _ = self.done.recv_timeout(grace);
        std::mem::take(&mut *self.data.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Run the command; its stdout on success.
fn run(cmd: &[String], root: &Path, cancel: &AtomicBool, timeout: Option<Duration>) -> Result<String, String> {
    let mut child = spawn_child(cmd, root).map_err(|e| format!("cannot run {}: {e}", cmd[0]))?;
    let pgid = child.id();
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
    stop_leftovers(pgid);
    let grace = Duration::from_secs(2);
    let (out, err) = (stdout.finish(grace), stderr.finish(grace));
    if status.success() {
        return Ok(String::from_utf8_lossy(&out).into_owned());
    }
    let err = String::from_utf8_lossy(if err.is_empty() { &out } else { &err });
    let tail: String = err.trim().chars().rev().take(500).collect::<Vec<_>>().into_iter().rev().collect();
    Err(format!("narrative generation failed: {tail}"))
}

fn finished(view_id: u64, res: Result<(), String>) {
    JOB.with(|j| j.borrow_mut().take());
    match &res {
        Ok(()) => ui::notify("narrative ready", ui::INFO),
        Err(e) => ui::notify(e, ui::ERROR),
    }
    // Only the view that asked for it is updated. If it has been closed or
    // replaced, the narrative file was still written; a view of the same
    // range picks it up through its watcher.
    let same_view = with_view(|v| {
        let ours = v.id() == view_id;
        if ours {
            v.set_generating(false);
        }
        ours
    });
    if same_view != Some(true) {
        return;
    }
    refresh_then(Some(Box::new(|| {
        let cfg = state::config();
        with_view(|v| {
            if v.model().narrative.is_some() {
                actions::open_narrative(v, &cfg);
            }
        });
    })));
}
