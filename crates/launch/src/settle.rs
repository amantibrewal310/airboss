//! Settling rows: pending T3 ones through `sync.find_thread`, pending Herdr ones through the
//! labels on Herdr's terminals, then linking started threads and moving their task to `doing`.

use std::path::Path;
use std::time::{Duration, SystemTime};

use airboss_core::{Error, TaskId};
use airboss_services::{ServiceError, Source};

use crate::db::{Row, Runs, State};
use crate::herdr::{self, Tool};
use crate::start::thread_link;
use crate::{Inner, LaunchConfig, lock, log};

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

/// A pending T3 Do it row is `started` once `sync` finds its thread on its branch, or `lost`
/// `settle_after` after it was made. A T3 row with no branch (Plan it, research) has nothing
/// to look up, so it stays pending until "Start again". Herdr rows: see [`settle_herdr`].
fn settle_pending(inner: &Inner, row: &Row, now: SystemTime) -> Result<bool, ServiceError> {
    if row.runs == Runs::Herdr {
        return settle_herdr(inner, row, now);
    }
    let Some(branch) = &row.branch else {
        return Ok(false);
    };
    let found = inner
        .sync
        .find_thread(Path::new(&row.project_path), branch)
        .filter(|key| key.source == Source::T3);
    if let Some(key) = found
        && bind(inner, row, &key.r#ref, now)?
    {
        return Ok(true);
    }
    if now >= row.created_at + inner.cfg.settle_after {
        return lock(&inner.db).lost(row.id, now);
    }
    Ok(false)
}

/// A pending Herdr row is `started` once a live terminal carries its labels (`airboss.task`
/// and `airboss.launch`), or `lost` `herdr_settle_after` after it was made. While `hc`
/// can't answer, the row stays pending: a terminal may exist.
fn settle_herdr(inner: &Inner, row: &Row, now: SystemTime) -> Result<bool, ServiceError> {
    let result = lock(&inner.herdr).run(
        Tool::Hc,
        &herdr::find_argv(row.task, row.id),
        herdr::QUICK_TIMEOUT,
    );
    let found = match &result {
        Ok(out) => herdr::read_found(out),
        Err(_) => None,
    };
    let Some(found) = found else {
        log(&format!(
            "couldn't ask hc about launch row {}: {result:?}",
            row.id
        ));
        return Ok(false);
    };
    if let Some(address) = found
        && bind(inner, row, &address, now)?
    {
        return Ok(true);
    }
    if now >= row.created_at + inner.cfg.herdr_settle_after {
        return lock(&inner.db).lost(row.id, now);
    }
    Ok(false)
}

/// Binds a pending row to the thread found for it, links it and moves its task to `doing`.
/// `false` if another row already has that thread.
fn bind(inner: &Inner, row: &Row, thread: &str, now: SystemTime) -> Result<bool, ServiceError> {
    // Bound first, so the table lock is released before `link` takes it again.
    let started = lock(&inner.db).started(row.id, thread, now)?;
    match started {
        Some(started) => {
            link(inner, &started);
            // What `start` would have done had it seen the answer.
            move_to_doing(inner, started.task);
            Ok(true)
        }
        None => {
            log(&format!(
                "found thread {thread} for task {}, but another row already has it",
                row.task.0
            ));
            Ok(false)
        }
    }
}

/// When a pending row is next worth looking up: 10 s, 30 s and 60 s after it was made, then
/// when it would be `lost`. A T3 row with no branch has nothing to look up.
pub(crate) fn next_check(row: &Row, cfg: &LaunchConfig, now: SystemTime) -> Option<SystemTime> {
    let settle_after = match row.runs {
        Runs::T3 => {
            row.branch.as_ref()?;
            cfg.settle_after
        }
        Runs::Herdr => cfg.herdr_settle_after,
    };
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
    match inner
        .core
        .link(row.task, thread_link(row.runs, thread, &row.title))
    {
        Ok(_) => {
            // Bound first, so the table lock is released before anything else runs.
            let recorded = lock(&inner.db).linked(row.id);
            match recorded {
                Ok(()) => true,
                Err(e) => {
                    log(&format!("couldn't record the link of thread {thread}: {e}"));
                    false
                }
            }
        }
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
