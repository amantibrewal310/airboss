//! What the worker and service tests share: a `core` store in a temp file, and times.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use airboss_core::{Actor, EventId, LinkKind, NewLink, NewTask, Status, Store, TaskId};
use airboss_services::time::from_ts;
use airboss_services::{SharedStore, StoreChange};
use airboss_sync::fake::TempDir;
use airboss_sync::{StoreSummary, SyncConfig};

/// Every test starts at this moment.
pub const T0: &str = "2026-11-26T12:00:00.000Z";

pub fn t0() -> SystemTime {
    from_ts(T0).unwrap()
}

/// `T0` plus `secs` seconds.
pub fn at(secs: u64) -> SystemTime {
    t0() + Duration::from_secs(secs)
}

pub fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A `core` store in a temp file, as the app has.
pub struct Core {
    pub dir: TempDir,
    pub store: SharedStore,
    cursor: EventId,
}

impl Core {
    pub fn new() -> Core {
        let dir = TempDir::new();
        let store = Store::open(&dir.path().join("airboss.db")).unwrap();
        Core {
            dir,
            store: Arc::new(Mutex::new(store)),
            cursor: EventId(0),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }

    pub fn task(&self, title: &str) -> TaskId {
        self.lock()
            .add_task(NewTask::new(title), &Actor::Me)
            .unwrap()
            .id
    }

    pub fn link(&self, task: TaskId, kind: LinkKind, r: &str) {
        self.lock()
            .link(task, NewLink::new(kind, r), &Actor::Me)
            .unwrap();
    }

    pub fn link_pr(&self, task: TaskId, r: &str) {
        self.lock().link(task, NewLink::pr(r), &Actor::Me).unwrap();
    }

    pub fn status(&self, task: TaskId) -> Status {
        self.lock().task(task).unwrap().status
    }

    pub fn set_status(&self, task: TaskId, to: Status) {
        self.lock().set_status(task, to, None, &Actor::Me).unwrap();
    }

    /// The `linked_at` core stored for this link.
    pub fn linked_at(&self, task: TaskId, r: &str) -> SystemTime {
        let links = self.lock().links(task).unwrap();
        let l = links.iter().find(|l| l.r#ref == r).unwrap();
        from_ts(&l.linked_at).unwrap()
    }

    pub fn notes(&self, task: TaskId) -> Vec<(String, String)> {
        self.lock()
            .history(task)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == airboss_core::EventKind::Note)
            .map(|e| (e.actor.to_string(), e.body.unwrap_or_default()))
            .collect()
    }

    pub fn open(&self) -> HashSet<TaskId> {
        self.change()
            .snapshot
            .tree
            .iter()
            .filter(|n| {
                matches!(
                    n.task.status,
                    Status::Todo | Status::Doing | Status::InReview | Status::Blocked
                )
            })
            .map(|n| n.task.id)
            .collect()
    }

    /// What the app would hand services now: events since the last call, and a snapshot.
    pub fn change(&self) -> StoreChange {
        let store = self.lock();
        let events = store.events_after(self.cursor).unwrap();
        StoreChange {
            events,
            snapshot: Arc::new(store.snapshot().unwrap()),
        }
    }

    /// The summary of everything since the last call, moving the cursor on.
    pub fn summary(&mut self) -> StoreSummary {
        let c = self.change();
        if let Some(last) = c.events.last() {
            self.cursor = last.id;
        }
        StoreSummary::from_change(&c)
    }

    /// Skips everything so far.
    pub fn mark(&mut self) {
        let latest = self.lock().latest_event_id().unwrap();
        self.cursor = latest;
    }
}

pub fn cfg(t3_dir: &Path, claude_dir: &Path) -> SyncConfig {
    let mut c = SyncConfig::defaults(Path::new("/nonexistent-home"));
    c.t3_dir = t3_dir.to_path_buf();
    c.claude_sessions = claude_dir.to_path_buf();
    c
}
