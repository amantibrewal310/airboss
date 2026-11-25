//! The finish check: a task's PRs merged on GitHub finish it, once no agent on it works.
//!
//! It runs only on a trigger (a PR left open, a link's first read, a hold ending), never
//! because I changed a task, so a task I reopen stays open. Each step is safe to repeat.

use std::collections::HashMap;
use std::time::SystemTime;

use airboss_core::{Actor, Error, LinkKind, PrState, Status, TaskId};
use airboss_services::{SharedStore, Source, ThreadKey, ThreadState};

use crate::db::SyncDb;
use crate::worker::{is_open, log};

/// What one finish check did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The task is now done.
    Done,
    /// Its PRs merged but an agent on it still works.
    Held,
    /// It doesn't pass (yet), or it isn't open.
    NotYet,
}

fn lock(store: &SharedStore) -> std::sync::MutexGuard<'_, airboss_core::Store> {
    store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn release(db: &SyncDb, task: TaskId) {
    if let Err(e) = db.release(task) {
        log(&format!(
            "couldn't release the hold on task {}: {e}",
            task.0
        ));
    }
}

/// The finish check for one task (SPEC-sync, "Finishing"). `fresh` holds states GitHub
/// gave this pass, which `observed_prs` doesn't have yet: it's written last, so that a pass
/// cut short repeats its triggers.
pub(crate) fn check(
    store: &SharedStore,
    db: &SyncDb,
    task: TaskId,
    fresh: &HashMap<String, PrState>,
    now: SystemTime,
) -> Outcome {
    let read = {
        let store = lock(store);
        store.task(task).and_then(|t| Ok((t, store.links(task)?)))
    };
    let (t, links) = match read {
        Ok(found) => found,
        Err(Error::NotFound) => {
            release(db, task);
            return Outcome::NotYet;
        }
        Err(e) => {
            log(&format!("couldn't read task {}: {e}", task.0));
            return Outcome::NotYet;
        }
    };
    let prs: Vec<_> = links.iter().filter(|l| l.kind == LinkKind::Pr).collect();
    if !is_open(t.status) || prs.is_empty() {
        release(db, task);
        return Outcome::NotYet;
    }
    let mut states = Vec::new();
    for l in &prs {
        let known = match fresh.get(&l.r#ref) {
            Some(s) => Ok(Some(*s)),
            None => db.pr_state(&l.r#ref),
        };
        match known {
            Ok(Some(s)) => states.push(s),
            // Not read from GitHub yet: no finish, and any hold stays.
            Ok(None) => return Outcome::NotYet,
            Err(e) => {
                log(&format!("couldn't read PR {}: {e}", l.r#ref));
                return Outcome::NotYet;
            }
        }
    }
    if states.contains(&PrState::Open) || !states.contains(&PrState::Merged) {
        release(db, task);
        return Outcome::NotYet;
    }
    // Core first learns GitHub's state for each of the task's PRs.
    {
        let mut store = lock(store);
        for (l, gh) in prs.iter().zip(&states) {
            if l.pr_state != Some(*gh)
                && let Err(e) = store.set_pr_state(&l.r#ref, *gh, &Actor::SyncGh)
            {
                log(&format!("couldn't record PR {} as {gh}: {e}", l.r#ref));
            }
        }
    }
    let busy = links.iter().any(|l| {
        let source = match l.kind {
            LinkKind::T3Thread => Source::T3,
            LinkKind::ClaudeSession => Source::Claude,
            _ => return false,
        };
        let key = ThreadKey {
            source,
            r#ref: l.r#ref.clone(),
        };
        matches!(
            db.thread_state(&key),
            Ok(Some(ThreadState::Running | ThreadState::NeedsMe))
        )
    });
    if busy {
        if let Err(e) = db.hold(task, now) {
            log(&format!("couldn't hold task {}: {e}", task.0));
        }
        return Outcome::Held;
    }
    let merged: Vec<&str> = prs
        .iter()
        .zip(&states)
        .filter(|(_, s)| **s == PrState::Merged)
        .map(|(l, _)| l.r#ref.as_str())
        .collect();
    let note = format!("PRs merged: {}", merged.join(", "));
    let result = lock(store).set_status(task, Status::Done, Some(&note), &Actor::SyncGh);
    release(db, task);
    match result {
        Ok(_) => Outcome::Done,
        Err(e) => {
            // NotAllowed or InvalidTransition: logged and skipped, never retried in a loop.
            log(&format!("core refused to finish task {}: {e}", task.0));
            Outcome::NotYet
        }
    }
}

/// Checks every held task again (after a thread state changed, and at start).
pub(crate) fn recheck_held(store: &SharedStore, db: &SyncDb, now: SystemTime) -> bool {
    let held = match db.holds() {
        Ok(h) => h,
        Err(e) => {
            log(&format!("couldn't read finish holds: {e}"));
            return false;
        }
    };
    let mut any = false;
    for task in held {
        any |= check(store, db, task, &HashMap::new(), now) == Outcome::Done;
    }
    any
}
