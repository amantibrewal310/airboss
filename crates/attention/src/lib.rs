//! `airboss-attention`: what's waiting on me.
//!
//! It turns the latest `core` snapshot and `sync` snapshot into the Needs me list: the list at
//! the top of the main window, the callouts in a task's panel and the menu-bar count. It
//! changes no task. Everything it shows follows from one pure function, [`compute`]. See
//! `SPEC-attention.md` at the repo root.

mod compute;
mod db;
mod hidden;
mod kinds;
mod quiet;
mod stopped;
mod worker;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use airboss_core::{CoreSnapshot, EventId, TaskId};
use airboss_services::time::from_ts;
use airboss_services::{
    AttentionService, Changes, Clock, Connection, ErrorCode, NeedsItem, NeedsKey, NeedsList,
    Service, ServiceError, SharedStore, StoreChange, SyncService, Timestamp,
};

use crate::worker::Shared;

pub use compute::{Computed, Hidden, Inputs, compute};
pub use hidden::{HiddenRow, HiddenStore};
pub use worker::{AttentionMsg, AttentionWorker, MAX_WAIT, Reply, Step};

/// Thresholds and limits. The defaults are the EM's (25 Nov 2026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionConfig {
    /// Work with no activity this long is Quiet: 5 days.
    pub quiet_after: Duration,
    /// How long "Snooze" hides an item when no end is given: 3 days.
    pub snooze_for: Duration,
    /// How long a task that became ready stays in Ready: 7 days.
    pub ready_window: Duration,
    /// How long a `doing` task's agents must have been stopped, with nothing else changing,
    /// before it shows as Agent stopped: 10 minutes (EM, 26 Nov 2026).
    pub stopped_after: Duration,
    /// Items the menu-bar dropdown shows: 6.
    pub menu_items: usize,
    /// Changes this close together make one recompute: 150 ms.
    pub settle: Duration,
}

const DAY: Duration = Duration::from_secs(86_400);

impl Default for AttentionConfig {
    fn default() -> AttentionConfig {
        AttentionConfig {
            quiet_after: DAY * 5,
            snooze_for: DAY * 3,
            ready_window: DAY * 7,
            stopped_after: Duration::from_secs(10 * 60),
            menu_items: 6,
            settle: Duration::from_millis(150),
        }
    }
}

/// How long `dismiss`, `snooze` and `restore` wait for the worker. A recompute takes
/// milliseconds, so this only matters if the worker is wedged.
const ASK_TIMEOUT: Duration = Duration::from_secs(5);

/// How long `shutdown` waits for the two threads.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

/// The attention service: implements [`AttentionService`].
///
/// Two threads: the worker, which owns every input, the hidden rows and its own database
/// connection, and a waiter on `sync`'s [`Changes`]. Every trait method either queues a
/// message and returns, or reads the published list.
pub struct Attention {
    tx: Sender<AttentionMsg>,
    shared: Arc<Shared>,
    cfg: AttentionConfig,
    clock: Arc<dyn Clock>,
    sync: Arc<dyn SyncService>,
    stop: Arc<AtomicBool>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    /// Why the hidden-items table didn't open, shown as a connection row.
    problem: Option<String>,
}

/// The reason on the "Needs me · Didn't start" row.
const DIDNT_START: &str =
    "Its table in the database couldn't be opened, so dismissed items come back and Dismiss fails.";

impl std::fmt::Debug for Attention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attention")
            .field("cfg", &self.cfg)
            .field("stopped", &self.stop.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

/// Starts attention in the app process.
///
/// Reads `core` once, now (`snapshot()` and `path()`), and never again: every later change
/// arrives through [`Service::store_changed`]. Opens `attention_hidden` in the same file, on
/// its own connection. If the snapshot can't be read, it starts empty and waits for the first
/// change; if the table can't be opened, it hides nothing and dismissals fail with `Storage`.
/// It fails only if a thread can't be started.
pub fn start(
    cfg: AttentionConfig,
    store: SharedStore,
    sync: Arc<dyn SyncService>,
    clock: Arc<dyn Clock>,
) -> Result<Arc<Attention>, ServiceError> {
    let (snapshot, path) = {
        let store = store.lock().unwrap_or_else(PoisonError::into_inner);
        (store.snapshot(), store.path())
    };
    // Attention keeps no handle to the store: nothing after this can read `core`.
    drop(store);
    let core = snapshot.unwrap_or_else(|e| {
        log(&format!(
            "couldn't read core at start ({e}); waiting for the first change"
        ));
        empty_snapshot()
    });
    let (hidden, problem) = match match &path {
        Some(path) => HiddenStore::open(path),
        None => HiddenStore::in_memory(),
    } {
        Ok(hidden) => (hidden, None),
        Err(_) => (HiddenStore::unavailable(), Some(DIDNT_START.to_string())),
    };

    // Read before the worker takes its first sync snapshot, so no change falls between.
    let seen = sync.changes().current();
    let worker = AttentionWorker::new(
        cfg.clone(),
        Arc::new(core),
        sync.clone(),
        hidden,
        clock.now(),
    );
    let shared = worker.shared();
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let worker_clock = clock.clone();
    let worker_thread = thread::Builder::new()
        .name("attention".into())
        .spawn(move || worker::run(worker, rx, worker_clock))
        .map_err(|e| no_thread(&e))?;
    let (waiter_sync, waiter_tx, waiter_stop) = (sync.clone(), tx.clone(), stop.clone());
    let waiter = thread::Builder::new()
        .name("attention-sync".into())
        .spawn(move || worker::watch_sync(waiter_sync, seen, waiter_tx, waiter_stop));
    let waiter = match waiter {
        Ok(handle) => handle,
        Err(e) => {
            let _ = tx.send(AttentionMsg::Shutdown);
            let _ = worker_thread.join();
            return Err(no_thread(&e));
        }
    };
    Ok(Arc::new(Attention {
        tx,
        shared,
        cfg,
        clock,
        sync,
        stop,
        threads: Mutex::new(vec![worker_thread, waiter]),
        problem,
    }))
}

impl Attention {
    /// The first items for the menu-bar dropdown (`menu_items`, 6 by default).
    pub fn menu(&self) -> Vec<NeedsItem> {
        let list = self.shared.needs_me();
        list.items
            .iter()
            .take(self.cfg.menu_items)
            .cloned()
            .collect()
    }

    fn send(&self, msg: AttentionMsg) {
        // After shutdown there's no one to tell, and nothing to do.
        let _ = self.tx.send(msg);
    }

    /// Sends a `Hide` or `Restore` and waits for the worker's answer.
    fn ask(&self, msg: impl FnOnce(Reply) -> AttentionMsg) -> Result<(), ServiceError> {
        let (reply, answer) = mpsc::channel();
        if self.stop.load(Ordering::Acquire) || self.tx.send(msg(reply)).is_err() {
            return Err(shutting_down());
        }
        match answer.recv_timeout(ASK_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(ServiceError::new(
                ErrorCode::Internal,
                "Needs me is busy; try again in a moment.",
            )),
            // The worker dropped the reply: it stopped, or `handle` panicked.
            Err(RecvTimeoutError::Disconnected) if self.stop.load(Ordering::Acquire) => {
                Err(shutting_down())
            }
            Err(RecvTimeoutError::Disconnected) => Err(ServiceError::new(
                ErrorCode::Internal,
                "Something went wrong in Needs me; try again.",
            )),
        }
    }
}

impl Service for Attention {
    /// Attention depends on nothing outside the app; it has a row only when its own table
    /// didn't open.
    fn connections(&self) -> Vec<Connection> {
        let row = |why: &String| Connection {
            name: "Needs me".into(),
            ok: false,
            state: "Didn't start".into(),
            detail: why.clone(),
            fix: Some("Quit and reopen Airboss. If it happens again, see app.log.".into()),
        };
        self.problem.iter().map(row).collect()
    }

    fn recheck(&self) {
        self.send(AttentionMsg::Recheck);
    }

    fn changes(&self) -> Changes {
        self.shared.changes()
    }

    /// Queues the snapshot that came with the change; the events aren't needed.
    fn store_changed(&self, c: &StoreChange) {
        self.send(AttentionMsg::Store(c.snapshot.clone()));
    }

    fn woke(&self) {
        self.send(AttentionMsg::Woke);
    }

    /// Stops both threads, waiting at most a second.
    fn shutdown(&self) {
        if self.stop.swap(true, Ordering::AcqRel) {
            return;
        }
        self.send(AttentionMsg::Shutdown);
        // The waiter sleeps in `wait_past` for up to 15 minutes; a bump wakes it to see the
        // stop flag. Other listeners only re-read an unchanged snapshot.
        self.sync.changes().bump();
        let handles =
            std::mem::take(&mut *self.threads.lock().unwrap_or_else(PoisonError::into_inner));
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while handles.iter().any(|h| !h.is_finished()) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        for handle in handles {
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                log("a thread didn't stop within a second; leaving it");
            }
        }
    }
}

impl AttentionService for Attention {
    fn needs_me(&self) -> Arc<NeedsList> {
        self.shared.needs_me()
    }

    fn for_task(&self, task: TaskId) -> Vec<NeedsItem> {
        self.shared.for_task(task)
    }

    fn dismiss(&self, k: &NeedsKey) -> Result<(), ServiceError> {
        self.ask(|reply| AttentionMsg::Hide(k.clone(), None, reply))
    }

    /// `None` snoozes for `snooze_for` (3 days) from now.
    fn snooze(&self, k: &NeedsKey, until: Option<Timestamp>) -> Result<(), ServiceError> {
        let until: SystemTime = match until {
            None => self.clock.now() + self.cfg.snooze_for,
            Some(ts) => from_ts(&ts).ok_or_else(|| {
                ServiceError::new(ErrorCode::Internal, "That snooze time can't be read.")
            })?,
        };
        self.ask(|reply| AttentionMsg::Hide(k.clone(), Some(until), reply))
    }

    fn restore(&self, k: &NeedsKey) -> Result<(), ServiceError> {
        self.ask(|reply| AttentionMsg::Restore(k.clone(), reply))
    }
}

impl Drop for Attention {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn empty_snapshot() -> CoreSnapshot {
    CoreSnapshot {
        tree: Vec::new(),
        edges: Vec::new(),
        ready: Vec::new(),
        latest_event_id: EventId(0),
    }
}

fn no_thread(e: &std::io::Error) -> ServiceError {
    log(&format!("couldn't start a thread: {e}"));
    ServiceError::new(ErrorCode::Internal, "Needs me couldn't start.")
}

fn shutting_down() -> ServiceError {
    ServiceError::new(ErrorCode::ShuttingDown, "Airboss is shutting down.")
}

/// One line through the services' log, which the app sends to its log file.
pub(crate) fn log(message: &str) {
    airboss_services::log::line("airboss-attention", message);
}
