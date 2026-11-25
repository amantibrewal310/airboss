//! The undo stack (20 entries of inverse calls) and deferred rejects.
//!
//! `core` can't turn a task back into a proposal, so Reject is written 5 s later (or at quit);
//! until then undo simply cancels it. That's the only honest undo for it.

use std::collections::{HashSet, VecDeque};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

use airboss_core::{Actor, EventId, Status, TaskId};
use airboss_services::NeedsKey;
use tauri_plugin_log::log;

use crate::dto::UndoEntry;
use crate::state::{AppState, lock};

/// How deep the stack goes; the oldest entry drops off.
pub const DEPTH: usize = 20;

/// One call that undoes part of an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inverse {
    /// Back to the previous status, with the previous blocked reason as the note.
    SetStatus {
        id: TaskId,
        status: Status,
        note: Option<String>,
    },
    /// Back to the previous parent and position.
    Move {
        id: TaskId,
        parent: Option<TaskId>,
        position: u32,
    },
    /// `attention.restore(key)`: undoes Dismiss, Keep open and Snooze.
    Restore(NeedsKey),
    /// Cancels a reject that hasn't been written yet.
    CancelReject(u64),
}

/// One undoable action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    /// e.g. "Mark done 144".
    pub label: String,
    /// Applied in order.
    pub inverse: Vec<Inverse>,
    /// The tasks it changed; a change to any of them by someone else refuses the undo.
    pub tasks: Vec<TaskId>,
    /// The newest event id before the action; later events by others are "changed since".
    pub after: EventId,
    /// Said after "Undid …", e.g. that a drop's proposals stay dropped.
    pub note: Option<String>,
}

/// The stack, newest last.
#[derive(Debug, Default)]
pub struct UndoStack {
    entries: VecDeque<Entry>,
    next: u64,
}

impl UndoStack {
    /// Pushes an entry, dropping the oldest past [`DEPTH`]; returns what the UI shows.
    pub fn push(
        &mut self,
        label: String,
        inverse: Vec<Inverse>,
        tasks: Vec<TaskId>,
        after: EventId,
        note: Option<String>,
    ) -> UndoEntry {
        self.next += 1;
        let id = self.next;
        if self.entries.len() == DEPTH {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            id,
            label: label.clone(),
            inverse,
            tasks,
            after,
            note,
        });
        UndoEntry { id, label }
    }

    /// Takes the entry `id`, or the newest with `None`.
    pub fn take(&mut self, id: Option<u64>) -> Option<Entry> {
        let at = match id {
            Some(id) => self.entries.iter().position(|e| e.id == id)?,
            None => self.entries.len().checked_sub(1)?,
        };
        self.entries.remove(at)
    }

    /// The newest entry's label, for the Task menu's "Undo <label>".
    pub fn top_label(&self) -> Option<String> {
        self.entries.back().map(|e| e.label.clone())
    }

    /// How many entries there are.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// No entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drops every entry that names a task no longer in the tree.
    pub fn retain_tasks(&mut self, existing: &HashSet<TaskId>) {
        self.entries
            .retain(|e| e.tasks.iter().all(|t| existing.contains(t)));
    }

    /// Drops the entry that cancels reject `reject`, once it's been written.
    pub fn forget_reject(&mut self, reject: u64) {
        self.entries
            .retain(|e| !e.inverse.contains(&Inverse::CancelReject(reject)));
    }
}

/// A reject waiting to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReject {
    pub id: u64,
    pub ids: Vec<TaskId>,
    pub reason: Option<String>,
    pub due: Instant,
}

/// The pending rejects and the condvar their timer waits on.
#[derive(Debug, Default)]
pub struct Rejects {
    pending: Mutex<(Vec<PendingReject>, u64)>,
    changed: Condvar,
}

impl Rejects {
    /// Queues a reject; returns its id.
    pub fn add(&self, ids: Vec<TaskId>, reason: Option<String>, due: Instant) -> u64 {
        let mut p = lock(&self.pending);
        p.1 += 1;
        let id = p.1;
        p.0.push(PendingReject {
            id,
            ids,
            reason,
            due,
        });
        self.changed.notify_all();
        id
    }

    /// Cancels a pending reject; `false` if it was already written.
    pub fn cancel(&self, id: u64) -> bool {
        let mut p = lock(&self.pending);
        let before = p.0.len();
        p.0.retain(|r| r.id != id);
        self.changed.notify_all();
        p.0.len() != before
    }

    /// Takes every reject due by `now` (all of them with `None`).
    fn take_due(&self, now: Option<Instant>) -> Vec<PendingReject> {
        let mut p = lock(&self.pending);
        let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut p.0)
            .into_iter()
            .partition(|r| now.is_none_or(|n| r.due <= n));
        p.0 = keep;
        due
    }

    /// Wakes the timer (at quit).
    pub fn wake(&self) {
        self.changed.notify_all();
    }
}

impl AppState {
    /// Writes rejects now: those due, or every one with `all` (at quit).
    pub(crate) fn write_rejects(&self, all: bool) {
        let due = self.rejects.take_due((!all).then(Instant::now));
        if due.is_empty() {
            return;
        }
        for r in &due {
            for id in &r.ids {
                // Accepted (or changed) since the reject was queued: leave it.
                match self.core(|s| s.task(*id)) {
                    Ok(t) if t.status == Status::Proposed => {}
                    Ok(_) | Err(airboss_core::Error::NotFound) => continue,
                    Err(e) => {
                        log::warn!("couldn't check {} before rejecting it: {e}", id.0);
                        continue;
                    }
                }
                let reason = r.reason.as_deref();
                if let Err(e) =
                    self.core(|s| s.set_status(*id, Status::Dropped, reason, &Actor::Me))
                {
                    log::warn!("couldn't write the reject of {}: {e}", id.0);
                }
            }
            lock(&self.undo).forget_reject(r.id);
        }
        if let Err(e) = self.catch_up() {
            log::warn!("catch-up after rejects failed: {e}");
        }
        self.refresh_menus();
    }
}

/// The deferred-reject timer: sleeps until the earliest reject is due, writes it, repeats.
/// It waits on a condvar with a deadline; it never polls.
pub(crate) fn reject_timer(state: &AppState) {
    use std::sync::atomic::Ordering;
    loop {
        if state.stopping.load(Ordering::Relaxed) {
            return;
        }
        {
            let p = lock(&state.rejects.pending);
            let next = p.0.iter().map(|r| r.due).min();
            match next {
                None => {
                    drop(
                        state
                            .rejects
                            .changed
                            .wait(p)
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                    );
                    continue;
                }
                Some(due) => {
                    if let Some(wait) = due.checked_duration_since(Instant::now()) {
                        drop(
                            state
                                .rejects
                                .changed
                                .wait_timeout(p, wait)
                                .unwrap_or_else(std::sync::PoisonError::into_inner),
                        );
                        continue;
                    }
                }
            }
        }
        state.write_rejects(false);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_stack_keeps_twenty_and_drops_the_oldest() {
        let mut s = UndoStack::default();
        for i in 0..25 {
            s.push(format!("e{i}"), vec![], vec![], EventId(0), None);
        }
        assert_eq!(s.len(), DEPTH);
        assert_eq!(s.top_label().unwrap(), "e24");
        let oldest = s.take(Some(6));
        assert_eq!(oldest.unwrap().label, "e5");
        assert!(s.take(Some(1)).is_none(), "e0 dropped off");
    }

    #[test]
    fn take_without_an_id_takes_the_newest() {
        let mut s = UndoStack::default();
        s.push("a".into(), vec![], vec![], EventId(0), None);
        s.push("b".into(), vec![], vec![], EventId(0), None);
        assert_eq!(s.take(None).unwrap().label, "b");
        assert_eq!(s.take(None).unwrap().label, "a");
        assert!(s.take(None).is_none());
    }
}
