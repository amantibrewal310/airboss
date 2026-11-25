//! The `core` calls launch makes, and failures tests can arm.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use airboss_core::{Actor, Error, Link, NewLink, Status, Store, Task, TaskId};
use airboss_services::SharedStore;

/// A `core` write launch makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreWrite {
    SetBranch,
    Link,
    SetStatus,
}

/// What an armed write fails with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailWith {
    /// The database failed.
    Db,
    /// `core` refused the change from the task's status.
    InvalidTransition,
    /// The task is gone.
    NotFound,
}

impl FailWith {
    fn error(self) -> Error {
        match self {
            FailWith::Db => Error::Db(rusqlite::Error::InvalidQuery),
            FailWith::InvalidTransition => Error::InvalidTransition,
            FailWith::NotFound => Error::NotFound,
        }
    }
}

/// The shared store, with one short lock per call, acting as `launch`. Clones share the
/// store and the armed failures.
#[derive(Clone)]
pub struct CoreAccess {
    store: SharedStore,
    armed: Arc<Mutex<Vec<(CoreWrite, FailWith)>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every core call is one transaction, so a panic mid-call can't leave it half-written.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl CoreAccess {
    pub fn new(store: SharedStore) -> CoreAccess {
        CoreAccess {
            store,
            armed: Arc::default(),
        }
    }

    /// Makes the next `write` fail once, with `with`. Arming twice fails the next two.
    pub fn fail_next(&self, write: CoreWrite, with: FailWith) {
        lock(&self.armed).push((write, with));
    }

    fn armed(&self, write: CoreWrite) -> Result<(), Error> {
        let mut armed = lock(&self.armed);
        match armed.iter().position(|(w, _)| *w == write) {
            Some(at) => Err(armed.remove(at).1.error()),
            None => Ok(()),
        }
    }

    /// Reads from the store under one lock.
    pub(crate) fn read<T>(&self, f: impl FnOnce(&Store) -> Result<T, Error>) -> Result<T, Error> {
        f(&lock(&self.store))
    }

    /// The store's file, so launch can keep its tables beside `core`'s.
    pub(crate) fn path(&self) -> Option<std::path::PathBuf> {
        lock(&self.store).path()
    }

    pub(crate) fn set_branch(&self, task: TaskId, branch: &str) -> Result<Task, Error> {
        self.armed(CoreWrite::SetBranch)?;
        lock(&self.store).set_branch(task, Some(branch), &Actor::Launch)
    }

    pub(crate) fn link(&self, task: TaskId, link: NewLink) -> Result<Link, Error> {
        self.armed(CoreWrite::Link)?;
        lock(&self.store).link(task, link, &Actor::Launch)
    }

    pub(crate) fn move_to_doing(&self, task: TaskId) -> Result<Task, Error> {
        self.armed(CoreWrite::SetStatus)?;
        lock(&self.store).set_status(task, Status::Doing, None, &Actor::Launch)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use airboss_core::{LinkKind, NewTask};

    fn core() -> (CoreAccess, TaskId) {
        let mut store = Store::open_in_memory().unwrap();
        let t = store.add_task(NewTask::new("T"), &Actor::Me).unwrap();
        (CoreAccess::new(Arc::new(Mutex::new(store))), t.id)
    }

    #[test]
    fn an_armed_write_fails_once_with_its_error() {
        let (core, t) = core();
        core.fail_next(CoreWrite::SetBranch, FailWith::Db);
        assert!(matches!(core.set_branch(t, "b"), Err(Error::Db(_))));
        assert_eq!(
            core.set_branch(t, "b").unwrap().branch.as_deref(),
            Some("b")
        );
    }

    #[test]
    fn only_the_armed_kind_fails() {
        let (core, t) = core();
        core.fail_next(CoreWrite::SetStatus, FailWith::InvalidTransition);
        core.link(t, NewLink::new(LinkKind::T3Thread, "th"))
            .unwrap();
        assert!(matches!(
            core.move_to_doing(t),
            Err(Error::InvalidTransition)
        ));
        assert_eq!(core.move_to_doing(t).unwrap().status, Status::Doing);
    }

    #[test]
    fn arming_twice_fails_twice_and_clones_share() {
        let (core, t) = core();
        let other = core.clone();
        other.fail_next(CoreWrite::Link, FailWith::NotFound);
        other.fail_next(CoreWrite::Link, FailWith::Db);
        let l = || NewLink::new(LinkKind::T3Thread, "th");
        assert!(matches!(core.link(t, l()), Err(Error::NotFound)));
        assert!(matches!(core.link(t, l()), Err(Error::Db(_))));
        assert!(core.link(t, l()).is_ok());
    }

    #[test]
    fn writes_act_as_launch() {
        let (core, t) = core();
        let link = core
            .link(t, NewLink::new(LinkKind::T3Thread, "th"))
            .unwrap();
        assert_eq!(link.linked_by, Actor::Launch);
    }
}
