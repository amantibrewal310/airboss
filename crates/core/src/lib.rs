//! `airboss-core`: the single source of truth for Airboss tasks.
//!
//! Every other module reads and writes tasks only through [`Store`].
//! See `SPEC-core.md` at the repo root for the rules enforced here.

mod db;
mod error;
mod events;
mod links;
mod model;
mod permissions;
mod status;
mod positions;
mod tasks;
mod tx;

use std::path::Path;

use rusqlite::Connection;

use crate::tx::Tx;

pub use error::{Error, Result};
pub use model::TaskEdit;
pub use model::{
    Actor, Event, EventKind, NewTask, Origin, Repo, Status, Task, TaskNode, UnknownValue,
};
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
}

impl Store {
    /// Opens (or creates) the database file and brings `core`'s schema up to date.
    pub fn open(path: &Path) -> Result<Store> {
        Ok(Store {
            conn: db::open_file(path)?,
            last_committed: Vec::new(),
        })
    }

    /// Opens a private in-memory database, mainly for tests.
    pub fn open_in_memory() -> Result<Store> {
        Ok(Store {
            conn: db::open_memory()?,
            last_committed: Vec::new(),
        })
    }

    /// Runs one call as one transaction: all of it lands, events included, or none of it (rule 21).
    fn write<T>(&mut self, change: impl FnOnce(&mut Tx<'_>) -> Result<T>) -> Result<T> {
        let mut tx = Tx::begin(&mut self.conn)?;
        let out = change(&mut tx)?;
        self.last_committed = tx.commit()?;
        Ok(out)
    }

    /// Registers a repo tasks can point at. Only `me` does this.
    pub fn add_repo(
        &mut self,
        name: &str,
        path: &Path,
        remote: Option<&str>,
        actor: &Actor,
    ) -> Result<Repo> {
        self.write(|tx| tasks::add_repo(tx, name, path, remote, actor))
    }

    /// Every registered repo, by name.
    pub fn repos(&self) -> Result<Vec<Repo>> {
        tasks::repos(&self.conn)
    }

    /// Adds one task at the end of its siblings, with a `created` event.
    pub fn add_task(&mut self, new: NewTask, actor: &Actor) -> Result<Task> {
        self.write(|tx| tasks::add(tx, new, actor))
    }

    /// One task; `Error::NotFound` if it doesn't exist.
    pub fn task(&self, id: TaskId) -> Result<Task> {
        tasks::get(&self.conn, id)
    }

    /// The whole tree, depth-first, siblings in their chosen order.
    pub fn tree(&self) -> Result<Vec<TaskNode>> {
        tasks::tree(&self.conn)
    }

    /// Everything that happened to a task, oldest first.
    pub fn history(&self, id: TaskId) -> Result<Vec<Event>> {
        events::history(&self.conn, id)
    }
}

/// Test-only fault injection: makes a named point inside a call fail, to prove the call
/// leaves nothing behind. Compiles to nothing outside tests.
#[cfg(test)]
pub(crate) mod fault {
    use std::cell::Cell;

    thread_local! {
        static ARMED: Cell<Option<&'static str>> = const { Cell::new(None) };
    }

    pub(crate) const TASK_INSERTED: &str = "tasks::add after insert";
    pub(crate) const TASK_EDITED: &str = "tasks::edit after update";
    pub(crate) const NOTE_RECORDED: &str = "tasks::add_note after event";
    pub(crate) const LINK_INSERTED: &str = "links::link after insert";
    pub(crate) const LINK_DELETED: &str = "links::unlink after delete";
    pub(crate) const PR_STATE_RECORDED: &str = "links::set_pr_state after one task";
    pub(crate) const STATUS_UPDATED: &str = "status::apply after update";
    pub(crate) const MOVE_RENUMBERED: &str = "positions::move_task after renumber";

    pub(crate) fn arm(point: &'static str) {
        ARMED.with(|a| a.set(Some(point)));
    }

    pub(crate) fn hit(point: &str) -> crate::Result<()> {
        if ARMED.with(|a| a.get()) == Some(point) {
            ARMED.with(|a| a.set(None));
            return Err(crate::Error::Db(rusqlite::Error::InvalidQuery));
        }
        Ok(())
    }
}

#[cfg(not(test))]
pub(crate) mod fault {
    pub(crate) const TASK_INSERTED: &str = "tasks::add after insert";
    pub(crate) const TASK_EDITED: &str = "tasks::edit after update";
    pub(crate) const NOTE_RECORDED: &str = "tasks::add_note after event";
    pub(crate) const LINK_INSERTED: &str = "links::link after insert";
    pub(crate) const LINK_DELETED: &str = "links::unlink after delete";
    pub(crate) const PR_STATE_RECORDED: &str = "links::set_pr_state after one task";
    pub(crate) const STATUS_UPDATED: &str = "status::apply after update";
    pub(crate) const MOVE_RENUMBERED: &str = "positions::move_task after renumber";

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
}
