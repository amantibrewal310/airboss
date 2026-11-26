//! The worker: availability checks and settling pending rows, on one small thread.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use airboss_services::Availability;

use crate::{Inner, herdr, lock, log, settle, t3cli};

/// The longest the worker ever waits: 4 empty wake-ups an hour.
pub const MAX_WAIT: Duration = Duration::from_secs(15 * 60);

/// How soon an unlinked thread's link is tried again.
const RELINK_AFTER: Duration = Duration::from_secs(60);

/// The longest `t3 --version` may take.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// What the worker is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMsg {
    /// Run `t3 --version`, `hc --version` and `herdr-app --version`, and update
    /// `availability()` and `herdr_availability()`.
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
    /// Always after `now`, and at most 15 minutes after it.
    pub next_wake: SystemTime,
}

/// The worker's state machine. Tests drive it with chosen times; the app's worker thread
/// runs [`LaunchWorker::run`].
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

    /// Does what `msg` asks, at `now`.
    pub fn handle(&mut self, msg: LaunchMsg, now: SystemTime) -> Step {
        let changed = match msg {
            LaunchMsg::CheckAvailability => self.check_availability(),
            LaunchMsg::Settle | LaunchMsg::Woke | LaunchMsg::Tick => {
                match settle::settle_all(&self.inner, now) {
                    Ok(changed) => changed,
                    Err(e) => {
                        log(&format!("couldn't settle pending threads: {e}"));
                        false
                    }
                }
            }
            LaunchMsg::Shutdown => false,
        };
        if changed {
            self.inner.changes.bump();
        }
        Step {
            changed,
            next_wake: self.next_wake(now),
        }
    }

    /// T3: `Missing` without a launcher file, otherwise what `t3 --version` says. Herdr: what
    /// `hc --version` and `herdr-app --version` say.
    fn check_availability(&self) -> bool {
        let t3 = self.check_t3();
        let found = herdr::availability(lock(&self.inner.herdr).as_mut());
        let mut now = lock(&self.inner.herdr_availability);
        let herdr = *now != found;
        *now = found;
        t3 || herdr
    }

    fn check_t3(&self) -> bool {
        let found = if self.inner.cfg.t3_path.exists() {
            let result = lock(&self.inner.t3).run(
                &t3cli::version_argv(),
                None,
                VERSION_TIMEOUT.min(self.inner.cfg.timeout),
            );
            t3cli::read_version(result)
        } else {
            Availability::Missing {
                reason: crate::T3_MISSING.to_string(),
            }
        };
        let mut now = lock(&self.inner.availability);
        let changed = *now != found;
        *now = found;
        changed
    }

    /// The next settle time of any pending row, a minute from now if a thread is
    /// still unlinked, and never more than 15 minutes away.
    fn next_wake(&self, now: SystemTime) -> SystemTime {
        let cap = now + MAX_WAIT;
        let (pending, unlinked) = {
            let db = lock(&self.inner.db);
            match (db.pending(), db.unlinked()) {
                (Ok(p), Ok(u)) => (p, u),
                (Err(e), _) | (_, Err(e)) => {
                    log(&format!("couldn't read pending threads: {e}"));
                    return now + RELINK_AFTER;
                }
            }
        };
        pending
            .iter()
            .filter_map(|row| settle::next_check(row, &self.inner.cfg, now))
            .chain((!unlinked.is_empty()).then_some(now + RELINK_AFTER))
            .chain([cap])
            .min()
            .unwrap_or(cap)
    }

    /// Runs until `Shutdown`, waking at each step's `next_wake` (wall clock). A panic in
    /// `handle` is logged and the worker carries on.
    pub fn run(mut self) {
        let mut next = self.inner.clock.now() + MAX_WAIT;
        loop {
            let wait = next
                .duration_since(self.inner.clock.now())
                .unwrap_or(Duration::ZERO)
                .min(MAX_WAIT);
            let msg = match self.rx.recv_timeout(wait) {
                Ok(LaunchMsg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(msg) => msg,
                Err(RecvTimeoutError::Timeout) => LaunchMsg::Tick,
            };
            let now = self.inner.clock.now();
            next = match catch_unwind(AssertUnwindSafe(|| self.handle(msg, now))) {
                Ok(step) => step.next_wake,
                Err(_) => {
                    log(&format!(
                        "the worker panicked handling {msg:?}; carrying on"
                    ));
                    now + RELINK_AFTER
                }
            };
        }
    }
}
