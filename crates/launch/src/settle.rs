//! Settling rows: pending ones through `sync.find_thread`, then linking started threads and
//! moving their task to `doing`.

use std::path::Path;
use std::time::{Duration, SystemTime};

use airboss_core::{Error, TaskId};
use airboss_services::{ServiceError, Source};

use crate::db::{Row, State};
use crate::start::thread_link;
use crate::{Inner, lock, log};

/// Settles every pending row and links every unlinked one. `true` if any row changed.
pub(crate) fn settle_all(inner: &Inner, now: SystemTime) -> Result<bool, ServiceError> {
    let (pending, unlinked) = {
        let db = lock(&inner.db);
        (db.pending()?, db.unlinked()?)
    };
    settle(inner, pending, unlinked, now)
}

/// Settles one task's pending rows and links its unlinked ones, before `start` looks at them.
pub(crate) fn settle_task(
    inner: &Inner,
    task: TaskId,
    now: SystemTime,
) -> Result<(), ServiceError> {
    let rows = lock(&inner.db).rows_for_task(task)?;
    let (pending, started): (Vec<Row>, Vec<Row>) =
        rows.into_iter().partition(|r| r.state == State::Pending);
    let unlinked = started
        .into_iter()
        .filter(|r| r.state == State::Started && !r.linked)
        .collect();
    if settle(inner, pending, unlinked, now)? {
        inner.changes.bump();
    }
    Ok(())
}

fn settle(
    inner: &Inner,
    pending: Vec<Row>,
    unlinked: Vec<Row>,
    now: SystemTime,
) -> Result<bool, ServiceError> {
    let mut changed = false;
    for row in &pending {
        changed |= settle_pending(inner, row, now)?;
    }
    for row in &unlinked {
        changed |= link(inner, row);
    }
    Ok(changed)
}

/// A pending Do it row is `started` once `sync` finds its thread on its branch, or `lost`
/// `settle_after` after it was made. A row with no branch (Plan it, research) has nothing
/// to look up, so it stays pending until "Start again".
fn settle_pending(inner: &Inner, row: &Row, now: SystemTime) -> Result<bool, ServiceError> {
    let Some(branch) = &row.branch else {
        return Ok(false);
    };
    let found = inner
        .sync
        .find_thread(Path::new(&row.project_path), branch)
        .filter(|key| key.source == Source::T3);
    if let Some(key) = found {
        // Bound first, so the table lock is released before `link` takes it again.
        let started = lock(&inner.db).started(row.id, &key.r#ref, now)?;
        match started {
            Some(started) => {
                link(inner, &started);
                // What `start` would have done had it seen the answer.
                move_to_doing(inner, started.task);
                return Ok(true);
            }
            None => log(&format!(
                "sync found thread {} for task {}, but another row already has it",
                key.r#ref, row.task.0
            )),
        }
    }
    if now >= row.created_at + inner.cfg.settle_after {
        return lock(&inner.db).lost(row.id, now);
    }
    Ok(false)
}

/// When a pending Do it row is next worth looking up: 10 s, 30 s and 60 s after it was made,
/// then at `settle_after`.
pub(crate) fn next_check(row: &Row, settle_after: Duration, now: SystemTime) -> Option<SystemTime> {
    row.branch.as_ref()?;
    [
        Duration::from_secs(10),
        Duration::from_secs(30),
        Duration::from_secs(60),
        settle_after,
    ]
    .into_iter()
    .filter(|after| *after <= settle_after)
    .map(|after| row.created_at + after)
    .filter(|at| *at > now)
    .min()
}

/// Links a started row's thread to its task as `launch`. `true` once `core` has the link.
/// A task that's gone has its rows forgotten; any other failure leaves the row unlinked for
/// the worker's next pass.
pub(crate) fn link(inner: &Inner, row: &Row) -> bool {
    let Some(thread) = &row.thread_id else {
        return false;
    };
    match inner.core.link(row.task, thread_link(thread, &row.title)) {
        Ok(_) => match lock(&inner.db).linked(row.id) {
            Ok(()) => true,
            Err(e) => {
                log(&format!("couldn't record the link of thread {thread}: {e}"));
                false
            }
        },
        Err(Error::NotFound) => {
            forget(inner, row.task);
            false
        }
        Err(e) => {
            log(&format!(
                "couldn't link thread {thread} to task {}: {e}",
                row.task.0
            ));
            false
        }
    }
}

/// Moves the task to `doing` as `launch`. `InvalidTransition` means someone (usually the
/// agent) already moved it, which is fine. `true` only if this call moved it.
pub(crate) fn move_to_doing(inner: &Inner, task: TaskId) -> bool {
    match inner.core.move_to_doing(task) {
        Ok(_) => true,
        Err(Error::InvalidTransition) => false,
        Err(Error::NotFound) => {
            forget(inner, task);
            false
        }
        Err(e) => {
            log(&format!("couldn't move task {} to doing: {e}", task.0));
            false
        }
    }
}

fn forget(inner: &Inner, task: TaskId) {
    if let Err(e) = lock(&inner.db).forget_task(task) {
        log(&format!(
            "couldn't forget the rows of deleted task {}: {e}",
            task.0
        ));
    }
}
