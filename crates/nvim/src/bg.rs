//! Run blocking work (git, the narrative command) off Neovim's main thread.

use std::convert::Infallible;
use std::sync::mpsc;

use nvim_oxi::libuv::AsyncHandle;

/// Run `work` on a new thread, then `done` with its result on Neovim's main
/// loop (where the API may be used).
pub fn spawn<T, W, D>(work: W, done: D) -> Result<(), String>
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
    D: FnOnce(T) + 'static,
{
    let (tx, rx) = mpsc::channel::<T>();
    let mut done = Some(done);
    // The callback runs on the main thread, but in libuv's fast context, so
    // the real work is scheduled.
    let handle = AsyncHandle::new(move || {
        if let (Ok(value), Some(done)) = (rx.try_recv(), done.take()) {
            nvim_oxi::schedule(move |_| done(value));
        }
        Ok::<_, Infallible>(())
    })
    .map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = tx.send(work());
        let _ = handle.send();
    });
    Ok(())
}
