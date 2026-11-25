//! The worker: a state machine over messages and chosen times, the thread loop that drives it
//! with the wall clock, and the thread that turns `sync`'s changes into messages.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use airboss_core::{CoreSnapshot, TaskId};
use airboss_services::{
    Changes, Clock, ErrorCode, NeedsItem, NeedsKey, NeedsKind, NeedsList, ServiceError, SyncService,
};

use crate::compute::{Entry, compute_full};
use crate::{AttentionConfig, Hidden, HiddenRow, HiddenStore, Inputs, log};

/// No single wait is longer than this, so a deadline passed during sleep is caught within
/// 15 minutes even without `woke()`.
pub const MAX_WAIT: Duration = Duration::from_secs(15 * 60);

/// A hidden row whose key hasn't been live this long is deleted.
const PRUNE_AFTER: Duration = Duration::from_secs(30 * 86_400);

/// Pruning runs at most this often.
const PRUNE_EVERY: Duration = Duration::from_secs(86_400);

/// Where a `Hide` or `Restore` sends its answer.
pub type Reply = Sender<Result<(), ServiceError>>;

/// What the worker is asked to do.
#[derive(Debug)]
pub enum AttentionMsg {
    /// `core` changed; this snapshot came with the change.
    Store(Arc<CoreSnapshot>),
    /// `sync`'s changes moved: read its snapshot.
    Sync,
    /// Dismiss (`None`) or snooze until a time; one write, then a recompute, then the reply.
    Hide(NeedsKey, Option<SystemTime>, Reply),
    /// Undo a dismiss or snooze.
    Restore(NeedsKey, Reply),
    /// The "Check again" button.
    Recheck,
    /// The Mac woke.
    Woke,
    /// A wait ended with nothing received.
    Tick,
    Shutdown,
}

/// What the loop does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Wait for a message, sending `Tick` if none comes by then (at most `MAX_WAIT` away).
    Wait(SystemTime),
    Stop,
}

/// What the service hands out without asking the worker: the list and its change signal.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    published: Mutex<Arc<Published>>,
    changes: Changes,
}

/// One published list, and for each item every task it's about.
#[derive(Debug, Default)]
struct Published {
    list: Arc<NeedsList>,
    tasks: Vec<Vec<TaskId>>,
}

impl Shared {
    fn current(&self) -> MutexGuard<'_, Arc<Published>> {
        self.published
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub fn needs_me(&self) -> Arc<NeedsList> {
        self.current().list.clone()
    }

    /// Items about `task`, in list order, including one shown under a lower task id.
    pub fn for_task(&self, task: TaskId) -> Vec<NeedsItem> {
        let published = self.current().clone();
        published
            .list
            .items
            .iter()
            .zip(&published.tasks)
            .filter(|(_, tasks)| tasks.contains(&task))
            .map(|(item, _)| item.clone())
            .collect()
    }

    pub fn changes(&self) -> Changes {
        self.changes.clone()
    }
}

/// The worker's state machine: inputs, hidden rows, its own connection and the last list.
/// Tests drive it with [`handle`](AttentionWorker::handle) and chosen times.
pub struct AttentionWorker {
    cfg: AttentionConfig,
    inputs: Inputs,
    sync: Arc<dyn SyncService>,
    store: HiddenStore,
    rows: Vec<HiddenRow>,
    /// `rows` as `compute` takes them.
    hidden: Vec<Hidden>,
    shared: Arc<Shared>,
    /// Every live key from the last recompute, with its kind and task.
    live: HashMap<NeedsKey, (NeedsKind, TaskId)>,
    /// When the first unhandled `Store` or `Sync` arrived.
    dirty_at: Option<SystemTime>,
    next_wake: Option<SystemTime>,
    recomputes: u64,
    last_prune: Option<SystemTime>,
    /// When each hidden key was last seen live, since this worker started.
    last_live: HashMap<NeedsKey, SystemTime>,
}

impl std::fmt::Debug for AttentionWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttentionWorker")
            .field("hidden", &self.rows.len())
            .field("live", &self.live.len())
            .field("recomputes", &self.recomputes)
            .finish_non_exhaustive()
    }
}

impl AttentionWorker {
    /// Loads the hidden rows and computes the first list at `now`. `core` is the snapshot
    /// read at start; `sync` is read now and after each of its changes.
    pub fn new(
        cfg: AttentionConfig,
        core: Arc<CoreSnapshot>,
        sync: Arc<dyn SyncService>,
        store: HiddenStore,
        now: SystemTime,
    ) -> AttentionWorker {
        // A table that can't be read hides nothing: showing too much beats hiding a question.
        let rows = store.load().unwrap_or_default();
        let last_live = rows.iter().map(|r| (r.key.clone(), now)).collect();
        let mut worker = AttentionWorker {
            cfg,
            inputs: Inputs {
                core,
                sync: sync.snapshot(),
            },
            sync,
            store,
            rows,
            hidden: Vec::new(),
            shared: Arc::new(Shared::default()),
            live: HashMap::new(),
            dirty_at: None,
            next_wake: None,
            recomputes: 0,
            last_prune: None,
            last_live,
        };
        worker.rebuild_hidden();
        worker.recompute(now);
        worker
    }

    /// Handles one message at `now`, and says when to wake next.
    pub fn handle(&mut self, msg: AttentionMsg, now: SystemTime) -> Step {
        match msg {
            AttentionMsg::Store(snapshot) => {
                self.inputs.core = snapshot;
                self.changed(now);
            }
            AttentionMsg::Sync => {
                self.inputs.sync = self.sync.snapshot();
                self.changed(now);
            }
            AttentionMsg::Hide(key, until, reply) => {
                let answer = self.hide(key, until, now);
                // The caller may have given up waiting; nothing to do then.
                let _ = reply.send(answer);
            }
            AttentionMsg::Restore(key, reply) => {
                let answer = self.restore(&key, now);
                let _ = reply.send(answer);
            }
            AttentionMsg::Recheck | AttentionMsg::Woke | AttentionMsg::Tick => self.recompute(now),
            AttentionMsg::Shutdown => return Step::Stop,
        }
        self.step(now)
    }

    /// The current list; no I/O.
    pub fn needs_me(&self) -> Arc<NeedsList> {
        self.shared.needs_me()
    }

    /// The items about `task`, in list order.
    pub fn for_task(&self, task: TaskId) -> Vec<NeedsItem> {
        self.shared.for_task(task)
    }

    /// Bumped each time the visible list changes.
    pub fn changes(&self) -> Changes {
        self.shared.changes()
    }

    /// How many times `compute` has run, for tests of settling.
    pub fn recomputes(&self) -> u64 {
        self.recomputes
    }

    pub(crate) fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    /// A `Store` or `Sync` arrived: recompute once the settle window from the first one
    /// has passed. A steady stream can't postpone it.
    fn changed(&mut self, now: SystemTime) {
        let first = *self.dirty_at.get_or_insert(now);
        if now >= first + self.cfg.settle || now < first {
            self.recompute(now);
        }
    }

    /// When to wake next, as of `now`: a pending settle window, the next deadline, or the
    /// 15-minute cap, whichever is first.
    fn step(&self, now: SystemTime) -> Step {
        let mut at = now + MAX_WAIT;
        if let Some(first) = self.dirty_at {
            at = at.min(first + self.cfg.settle);
        }
        if let Some(wake) = self.next_wake {
            at = at.min(wake);
        }
        Step::Wait(at)
    }

    fn hide(
        &mut self,
        key: NeedsKey,
        until: Option<SystemTime>,
        now: SystemTime,
    ) -> Result<(), ServiceError> {
        let Some(&(kind, task)) = self.live.get(&key) else {
            return Err(ServiceError::new(
                ErrorCode::NotFound,
                "That item isn't in Needs me any more.",
            ));
        };
        let row = HiddenRow {
            key,
            kind,
            task,
            until,
        };
        self.store.hide(&row)?;
        self.rows.retain(|r| r.key != row.key);
        self.last_live.insert(row.key.clone(), now);
        self.rows.push(row);
        self.rebuild_hidden();
        self.recompute(now);
        Ok(())
    }

    fn restore(&mut self, key: &NeedsKey, now: SystemTime) -> Result<(), ServiceError> {
        if !self.store.restore(key)? {
            return Err(ServiceError::new(
                ErrorCode::NotFound,
                "That item isn't hidden any more.",
            ));
        }
        self.rows.retain(|r| r.key != *key);
        self.last_live.remove(key);
        self.rebuild_hidden();
        self.recompute(now);
        Ok(())
    }

    fn rebuild_hidden(&mut self) {
        self.hidden = self
            .rows
            .iter()
            .map(|r| Hidden {
                key: r.key.clone(),
                until: r.until,
            })
            .collect();
    }

    fn recompute(&mut self, now: SystemTime) {
        let full = compute_full(&self.inputs, &self.hidden, now, &self.cfg);
        self.recomputes += 1;
        self.dirty_at = None;
        self.next_wake = full.next_wake;
        self.live = full
            .live
            .into_iter()
            .map(|l| (l.key, (l.kind, l.task)))
            .collect();
        self.prune(now);
        self.publish(full.entries);
    }

    /// Deletes rows whose key hasn't been live for 30 days, at most once a day.
    fn prune(&mut self, now: SystemTime) {
        for row in &self.rows {
            if self.live.contains_key(&row.key) {
                match self.last_live.get_mut(&row.key) {
                    Some(seen) => *seen = now,
                    None => {
                        self.last_live.insert(row.key.clone(), now);
                    }
                }
            }
        }
        if self
            .last_prune
            .is_some_and(|last| now >= last && now < last + PRUNE_EVERY)
        {
            return;
        }
        self.last_prune = Some(now);
        let gone: Vec<NeedsKey> = self
            .rows
            .iter()
            .filter(|r| !self.live.contains_key(&r.key))
            .filter(|r| {
                self.last_live
                    .get(&r.key)
                    .is_none_or(|&seen| seen + PRUNE_AFTER <= now)
            })
            .map(|r| r.key.clone())
            .collect();
        if gone.is_empty() || self.store.prune(&gone).is_err() {
            return;
        }
        self.rows.retain(|r| !gone.contains(&r.key));
        for key in &gone {
            self.last_live.remove(key);
        }
        self.rebuild_hidden();
    }

    /// Publishes the new list; `version` and `changes()` move only if an item changed.
    fn publish(&mut self, entries: Vec<Entry>) {
        let (items, tasks): (Vec<NeedsItem>, Vec<Vec<TaskId>>) =
            entries.into_iter().map(|e| (e.item, e.tasks)).unzip();
        let checked_at = self.inputs.sync.checked_at.clone();
        let changed = {
            let mut current = self.shared.current();
            let changed = current.list.items != items;
            if changed || current.list.checked_at != checked_at || current.tasks != tasks {
                let list = NeedsList {
                    count: u32::try_from(items.len()).unwrap_or(u32::MAX),
                    items,
                    checked_at,
                    version: current.list.version + u64::from(changed),
                };
                *current = Arc::new(Published {
                    list: Arc::new(list),
                    tasks,
                });
            }
            changed
        };
        if changed {
            self.shared.changes.bump();
        }
    }
}

/// The worker thread: waits for a message or the next deadline, by the wall clock, and
/// never longer than [`MAX_WAIT`]. A panic in `handle` is logged and the previous list stays.
pub(crate) fn run(mut worker: AttentionWorker, rx: Receiver<AttentionMsg>, clock: Arc<dyn Clock>) {
    // The first list's deadline counts too: a task may turn quiet minutes after launch.
    let mut wake_at = match worker.step(clock.now()) {
        Step::Wait(at) => at,
        Step::Stop => return,
    };
    loop {
        let wait = wake_at
            .duration_since(clock.now())
            .unwrap_or(Duration::ZERO)
            .min(MAX_WAIT);
        let msg = match rx.recv_timeout(wait) {
            Ok(msg) => msg,
            Err(RecvTimeoutError::Timeout) => AttentionMsg::Tick,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let now = clock.now();
        match catch_unwind(AssertUnwindSafe(|| worker.handle(msg, now))) {
            Ok(Step::Wait(at)) => wake_at = at,
            Ok(Step::Stop) => return,
            Err(_) => {
                log("a recompute panicked; keeping the previous list");
                wake_at = now + MAX_WAIT;
            }
        }
    }
}

/// The `Changes` waiter: sends `Sync` each time `sync`'s count moves past what it saw.
pub(crate) fn watch_sync(
    sync: Arc<dyn SyncService>,
    mut seen: u64,
    tx: Sender<AttentionMsg>,
    stop: Arc<AtomicBool>,
) {
    let changes = sync.changes();
    loop {
        let count = changes.wait_past(seen, MAX_WAIT);
        if stop.load(Ordering::Acquire) {
            return;
        }
        if count > seen {
            seen = count;
            if tx.send(AttentionMsg::Sync).is_err() {
                return;
            }
        }
    }
}
