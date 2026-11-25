//! Small helpers shared by the app threads.

use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use tauri_plugin_log::log;

/// Runs `f` on its own thread once `delay` has passed since the last [`poke`](Debounced::poke).
/// Dropping it runs `f` once more if a poke is still pending, then stops the thread.
pub struct Debounced {
    tx: Mutex<Sender<()>>,
}

impl Debounced {
    /// Starts the thread; `None` if it couldn't (logged).
    pub fn new(name: &str, delay: Duration, f: impl Fn() + Send + 'static) -> Option<Debounced> {
        let (tx, rx) = mpsc::channel::<()>();
        let spawned = std::thread::Builder::new()
            .name(format!("airboss-{name}"))
            .spawn(move || {
                while rx.recv().is_ok() {
                    loop {
                        match rx.recv_timeout(delay) {
                            Ok(()) => continue,
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => {
                                f();
                                return;
                            }
                        }
                    }
                    f();
                }
            });
        match spawned {
            Ok(_) => Some(Debounced { tx: Mutex::new(tx) }),
            Err(e) => {
                log::error!("couldn't start the {name} thread: {e}");
                None
            }
        }
    }

    /// Asks for `f` to run after the delay, restarting the wait.
    pub fn poke(&self) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(());
        }
    }
}

/// Runs a thread's loop in `catch_unwind`. After one panic it runs again (told it was
/// restarted); after a second it stops and calls `gave_up`.
pub fn spawn_guarded(
    name: &str,
    mut body: impl FnMut(bool) + Send + 'static,
    gave_up: impl FnOnce() + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let label = name.to_string();
    std::thread::Builder::new()
        .name(format!("airboss-{name}"))
        .spawn(move || {
            let mut panics = 0;
            loop {
                let restarted = panics > 0;
                match catch_unwind(AssertUnwindSafe(|| body(restarted))) {
                    Ok(()) => return,
                    Err(_) => {
                        panics += 1;
                        log::error!("the {label} thread panicked ({panics})");
                        if panics >= 2 {
                            gave_up();
                            return;
                        }
                    }
                }
            }
        })
}
