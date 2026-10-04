//! Run blocking work (git, the narrative command) off Neovim's main thread.
//!
//! All jobs share one libuv async handle. nvim-oxi never closes an
//! `AsyncHandle`, so creating one per job would leak a handle each time.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use nvim_oxi::libuv::AsyncHandle;

type Value = Box<dyn Any + Send>;
type Done = Box<dyn FnOnce(Value)>;

/// Results that finished on worker threads, waiting for the main loop.
static FINISHED: Mutex<Vec<(u64, Value)>> = Mutex::new(Vec::new());
static NEXT_JOB: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Main thread only: what to run with each job's result.
    static WAITING: RefCell<HashMap<u64, Done>> = RefCell::new(HashMap::new());
    static WAKER: RefCell<Option<AsyncHandle>> = const { RefCell::new(None) };
}

/// The shared handle, created on first use. Its callback runs on the main
/// thread, but in libuv's fast context, so it only schedules the real work.
fn waker() -> Result<AsyncHandle, String> {
    WAKER.with(|w| {
        if let Some(h) = w.borrow().as_ref() {
            return Ok(h.clone());
        }
        let handle = AsyncHandle::new(|| {
            let finished = std::mem::take(&mut *FINISHED.lock().unwrap_or_else(|e| e.into_inner()));
            for (job, value) in finished {
                if let Some(done) = WAITING.with(|m| m.borrow_mut().remove(&job)) {
                    nvim_oxi::schedule(move |_| done(value));
                }
            }
            Ok::<_, Infallible>(())
        })
        .map_err(|e| e.to_string())?;
        *w.borrow_mut() = Some(handle.clone());
        Ok(handle)
    })
}

/// Run `work` on a new thread, then `done` with its result on Neovim's main
/// loop (where the API may be used).
pub fn spawn<T, W, D>(work: W, done: D) -> Result<(), String>
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(T) + 'static,
{
    let waker = waker()?;
    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    let done: Done = Box::new(move |value| {
        if let Ok(value) = value.downcast::<T>() {
            done(*value);
        }
    });
    WAITING.with(|m| m.borrow_mut().insert(job, done));
    std::thread::spawn(move || {
        let value: Value = Box::new(work());
        FINISHED.lock().unwrap_or_else(|e| e.into_inner()).push((job, value));
        let _ = waker.send();
    });
    Ok(())
}

/// Number of jobs whose results haven't been handled yet (for tests).
pub fn pending() -> usize {
    WAITING.with(|m| m.borrow().len())
}
