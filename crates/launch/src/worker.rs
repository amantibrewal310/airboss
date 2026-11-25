//! The worker: availability checks and settling pending rows, on one small thread.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::SystemTime;

use crate::Inner;

/// What the worker is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMsg {
    /// Run `t3 --version` and update `availability()`.
    CheckAvailability,
    /// Settle pending rows and link unlinked ones now.
    Settle,
    /// The Mac woke: catch up.
    Woke,
    /// A deadline came.
    Tick,
    /// Stop.
    Shutdown,
}

/// What one `handle` did, and when the worker next needs to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub changed: bool,
    /// Always after `now`.
    pub next_wake: SystemTime,
}

/// The worker's state machine. Tests drive it with chosen times.
pub struct LaunchWorker {
    inner: Arc<Inner>,
    rx: Receiver<LaunchMsg>,
}

/// A worker thread that's running.
pub(crate) struct Running {
    pub done: Receiver<()>,
    pub join: JoinHandle<()>,
}

impl LaunchWorker {
    pub(crate) fn new(inner: Arc<Inner>, rx: Receiver<LaunchMsg>) -> LaunchWorker {
        LaunchWorker { inner, rx }
    }

    /// The next queued message, if any, without waiting.
    pub fn try_next(&mut self) -> Option<LaunchMsg> {
        self.rx.try_recv().ok()
    }
}
