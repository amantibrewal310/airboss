//! `airboss-core`: the single source of truth for Airboss tasks.
//!
//! Every other module reads and writes tasks only through [`Store`].
//! See `SPEC-core.md` at the repo root for the rules enforced here.

mod backup;
mod db;
mod delete;
mod deps;
mod error;
mod events;
mod links;
mod location;
mod model;
mod permissions;
mod plans;
mod positions;
mod repos;
mod snapshot;
mod sql;
mod status;
mod tasks;
mod tx;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

use rusqlite::Connection;

use crate::tx::Tx;

pub use db::Migration;
pub use error::{Error, NEWER_DATABASE, Result};
pub use location::{DB_VAR, default_db_path};
pub use model::TaskEdit;
pub use model::{
    Actor, Event, EventKind, NewTask, Origin, Repo, Status, Task, TaskNode, UnknownValue,
};
pub use model::{CoreSnapshot, Dependencies, OutsideWait, ReadyTask, RunOrder};
pub use model::{DepRef, PlanResult, PlanTask};
pub use model::{Link, LinkKind, NewLink, PrState};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub i64);
    };
}

id_type!(
    /// A task's row id. Never reused while the task exists.
    TaskId
);
id_type!(
    /// An event's row id; also the cursor for "what changed since".
    EventId
);
id_type!(
    /// A link's row id.
    LinkId
);
id_type!(
    /// A repo's row id.
    RepoId
);

/// The one handle to the task database. All reads and writes go through it.
///
/// It isn't shared between threads by itself: the app keeps one behind a mutex.
pub struct Store {
    conn: Connection,
    /// Event ids from the last committed call; the change notification (rule 23) sends these.
    last_committed: Vec<EventId>,
    /// Who gets told about each committed change (rule 23). Dropped receivers are pruned.
    subscribers: Vec<Sender<Vec<EventId>>>,
}

impl Store {
    /// Opens (or creates) the database file and brings `core`'s schema up to date.
    pub fn open(path: &Path) -> Result<Store> {
        Ok(Store::with(db::open_file(path)?))
    }

    /// Opens a private in-memory database, mainly for tests.
    pub fn open_in_memory() -> Result<Store> {
        Ok(Store::with(db::open_memory()?))
    }

    /// The database file's absolute path, as SQLite resolved it, so another module can open
    /// its own `Store` on the same file from any working directory. `None` in memory.
    pub fn path(&self) -> Option<PathBuf> {
        self.conn
            .path()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
    }

    fn with(conn: Connection) -> Store {
        Store {
            conn,
            last_committed: Vec::new(),
            subscribers: Vec::new(),
        }
    }

    /// Runs one call as one transaction: all of it lands, events included, or none of it (rule 21).
    fn write<T>(&mut self, change: impl FnOnce(&mut Tx<'_>) -> Result<T>) -> Result<T> {
        let mut tx = Tx::begin(&mut self.conn)?;
        // A newer Airboss may have migrated the file since this handle opened it, e.g. under
        // a long-running `airboss mcp`. Its rules could differ, so write nothing.
        db::refuse_newer_core(&tx)?;
        let out = change(&mut tx)?;
        self.last_committed = tx.commit()?;
        // Sent only after a commit, and only when something was written, so a refused,
        // failed or no-op call tells nobody anything.
        if !self.last_committed.is_empty() {
            let ids = &self.last_committed;
            self.subscribers.retain(|s| s.send(ids.clone()).is_ok());
        }
        Ok(out)
    }

    /// Returns a channel that receives, after each committed call, the ids of the events
    /// it wrote. The app updates from this instead of polling (rule 23).
    pub fn subscribe(&mut self) -> Receiver<Vec<EventId>> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.subscribers.push(tx);
        rx
    }

    /// SQLite's `PRAGMA data_version`. It changes when **another** connection, such as
    /// `airboss mcp` in its own process, commits to the file, and never for this handle's own
    /// commits, which `subscribe` reports instead (rule 23). Compare two readings to see
    /// whether anything outside changed.
    pub fn data_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "data_version", |r| r.get(0))?)
    }

    /// Every event after `cursor`, oldest first. Event ids only grow, so this is the
    /// reliable way to catch up after missing notifications.
    pub fn events_after(&self, cursor: EventId) -> Result<Vec<Event>> {
        events::after(&self.conn, cursor)
    }
}

/// Test-only fault injection: makes a named point inside a call fail, to prove the call
/// leaves nothing behind. Compiles to nothing outside tests.
pub(crate) mod fault {
    #[cfg(test)]
    use std::cell::Cell;

    #[cfg(test)]
    thread_local! {
        static ARMED: Cell<Option<&'static str>> = const { Cell::new(None) };
    }

    pub(crate) const TASK_INSERTED: &str = "tasks::add after insert";
    pub(crate) const TASK_EDITED: &str = "tasks::edit after update";
    pub(crate) const BRANCH_SET: &str = "tasks::set_branch after update";
    pub(crate) const NOTE_RECORDED: &str = "tasks::add_note after event";
    pub(crate) const REPO_LINKED: &str = "repos::link_repo after insert";
    pub(crate) const REPO_UNLINKED: &str = "repos::unlink_repo after delete";
    pub(crate) const REPO_EDITED: &str = "repos::edit_repo after update";
    pub(crate) const REPO_REMOVED: &str = "repos::remove_repo after unlinking, before delete";
    pub(crate) const LINK_INSERTED: &str = "links::link after insert";
    pub(crate) const LINK_DELETED: &str = "links::unlink after delete";
    pub(crate) const PR_STATE_RECORDED: &str = "links::set_pr_state after one task";
    pub(crate) const STATUS_UPDATED: &str = "status::apply after update";
    pub(crate) const MOVE_RENUMBERED: &str = "positions::move_task after renumber";
    pub(crate) const DEP_ADDED: &str = "deps::add after event";
    pub(crate) const DEP_REMOVED: &str = "deps::remove after event";
    pub(crate) const DEPS_CLEARED: &str = "deps::on_task_deleting after events";
    pub(crate) const SUBTREE_DROPPED: &str = "status::drop_subtree after events";
    pub(crate) const DELETE_RENUMBERED: &str = "delete::delete_task after renumber";
    pub(crate) const PLAN_TASKS_WRITTEN: &str = "plans::add_tasks after tasks, before deps";

    #[cfg(test)]
    pub(crate) fn arm(point: &'static str) {
        ARMED.with(|a| a.set(Some(point)));
    }

    #[cfg(test)]
    pub(crate) fn hit(point: &str) -> crate::Result<()> {
        if ARMED.with(|a| a.get()) == Some(point) {
            ARMED.with(|a| a.set(None));
            return Err(crate::Error::Db(rusqlite::Error::InvalidQuery));
        }
        Ok(())
    }

    #[cfg(not(test))]
    #[inline(always)]
    pub(crate) fn hit(_point: &str) -> crate::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn count(store: &Store, table: &str) -> i64 {
        store
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn a_failure_after_the_insert_leaves_no_task_and_no_event() {
        let mut store = Store::open_in_memory().unwrap();
        fault::arm(fault::TASK_INSERTED);
        assert!(store.add_task(NewTask::new("Doomed"), &Actor::Me).is_err());
        assert_eq!(
            (count(&store, "tasks"), count(&store, "task_events")),
            (0, 0)
        );

        // The store is still usable afterwards.
        store.add_task(NewTask::new("Fine"), &Actor::Me).unwrap();
        assert_eq!(
            (count(&store, "tasks"), count(&store, "task_events")),
            (1, 1)
        );
    }

    #[test]
    fn a_committed_call_reports_exactly_the_events_it_wrote() {
        let mut store = Store::open_in_memory().unwrap();
        let t = store.add_task(NewTask::new("One"), &Actor::Me).unwrap();
        let history = store.history(t.id).unwrap();
        assert_eq!(store.last_committed, vec![history[0].id]);
    }

    #[test]
    fn a_failed_call_reports_no_events() {
        let mut store = Store::open_in_memory().unwrap();
        store.add_task(NewTask::new("Before"), &Actor::Me).unwrap();
        let before = store.last_committed.clone();
        fault::arm(fault::TASK_INSERTED);
        assert!(store.add_task(NewTask::new("Doomed"), &Actor::Me).is_err());
        assert_eq!(store.last_committed, before);
    }

    #[test]
    fn a_call_that_fails_partway_sends_no_notification() {
        let mut store = Store::open_in_memory().unwrap();
        let rx = store.subscribe();
        fault::arm(fault::TASK_INSERTED);
        assert!(store.add_task(NewTask::new("Doomed"), &Actor::Me).is_err());
        assert!(rx.try_recv().is_err());
    }
}
