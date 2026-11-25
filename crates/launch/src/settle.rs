//! Settling rows after `t3`: linking started threads and moving the task to `doing`.

use std::time::SystemTime;

use airboss_core::{Error, TaskId};
use airboss_services::ServiceError;

use crate::db::Row;
use crate::start::thread_link;
use crate::{Inner, lock, log};

/// Settles this task's pending rows. (Built with the worker.)
pub(crate) fn settle_task(
    _inner: &Inner,
    _task: TaskId,
    _now: SystemTime,
) -> Result<(), ServiceError> {
    Ok(())
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
