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
