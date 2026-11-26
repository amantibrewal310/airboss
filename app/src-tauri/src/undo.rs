//! The undo stack (20 entries of inverse calls) and deferred rejects.
//!
//! `core` can't turn a task back into a proposal, so Reject is written 5 s later (or at quit);
//! until then undo simply cancels it. That's the only honest undo for it.

use std::collections::{HashSet, VecDeque};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

use airboss_core::{Actor, EventId, RejectOutcome, Status, TaskId};
use airboss_services::NeedsKey;
use tauri_plugin_log::log;

use crate::dto::{ToastArgs, ToastEvent, ToastEventAction, ToastTone, UndoEntry};
use crate::errors::{AppError, Ctx};
use crate::shell::UiEvent;
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

/// SQLite's busy or locked, by its own wording: the app depends on `core`, not rusqlite.
fn is_busy(e: &airboss_core::Error) -> bool {
    let text = e.to_string();
    matches!(e, airboss_core::Error::Db(_))
        && (text.contains("database is locked")
            || text.contains("database is busy")
            || text.contains("database table is locked"))
}

/// "153", "153 and 154", "153, 154 and 155".
fn ids_text(ids: &[TaskId]) -> String {
    let all: Vec<String> = ids.iter().map(|t| t.0.to_string()).collect();
    match all.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// "153 was already accepted; left as is", "153 and 154 were already accepted and 155
/// already dropped; left as is": the proposals a reject found no longer proposed. A proposal
/// that's open or done was accepted; one that's dropped was rejected or dropped elsewhere.
pub fn skipped_text(skipped: &[(TaskId, Status)]) -> Option<String> {
    let (dropped, accepted): (Vec<_>, Vec<_>) = skipped
        .iter()
        .filter(|(_, s)| *s != Status::Proposed)
        .partition(|(_, s)| *s == Status::Dropped);
    let ids = |v: &[&(TaskId, Status)]| ids_text(&v.iter().map(|(t, _)| *t).collect::<Vec<_>>());
    let verb = |n: usize| if n == 1 { "was" } else { "were" };
    let text = match (accepted.as_slice(), dropped.as_slice()) {
        ([], []) => return None,
        (a, []) => format!("{} {} already accepted", ids(a), verb(a.len())),
        ([], d) => format!("{} {} already dropped", ids(d), verb(d.len())),
        (a, d) => format!(
            "{} {} already accepted and {} already dropped",
            ids(a),
            verb(a.len()),
            ids(d)
        ),
    };
    Some(format!("{text}; left as is"))
}

/// "Couldn't reject 153: the database stayed busy.", with Retry (the same reject again).
fn reject_failed(ids: Vec<TaskId>, reason: Option<String>, why: &str) -> ToastEvent {
    ToastEvent {
        text: format!("Couldn't reject {}: {why}", ids_text(&ids)),
        task: ids.first().copied(),
        actions: vec![ToastEventAction {
            label: "Retry".into(),
            command: "reject_proposals".into(),
            args: ToastArgs::Reject { ids, reason },
        }],
        undo: None,
        tone: Some(ToastTone::Warning),
    }
}

impl AppState {
    /// Writes every pending reject now, as quitting does.
    pub fn write_rejects_now(&self) {
        self.write_rejects(true);
    }

    /// Writes rejects now: those due, or every one with `all` (at quit).
    ///
    /// Each pending reject is one `core` call, `reject_proposals`, which checks that each task
    /// is still a proposal and writes in one transaction, so an `airboss accept` from another
    /// process can't land in between. Ones accepted or dropped meanwhile are left as they are
    /// and named in a toast. A busy database is retried (`reject_retry`); a reject that still
    /// can't be written says so in a toast with Retry, instead of the proposal quietly coming
    /// back.
    pub(crate) fn write_rejects(&self, all: bool) {
        let due = self.rejects.take_due((!all).then(Instant::now));
        if due.is_empty() {
            return;
        }
        for r in &due {
            match self.reject_with_retry(&r.ids, r.reason.as_deref()) {
                Ok(outcome) => {
                    if let Some(text) = skipped_text(&outcome.skipped) {
                        self.shell.emit(UiEvent::Toast(ToastEvent {
                            text,
                            actions: Vec::new(),
                            undo: None,
                            tone: Some(ToastTone::Info),
                            task: outcome.skipped.first().map(|(t, _)| *t),
                        }));
                    }
                }
                Err(e) => {
                    log::warn!("couldn't write a reject of {} tasks: {e}", r.ids.len());
                    // A broken database shows its own full-window state instead.
                    if !e.is_storage_failure() {
                        let why = if is_busy(&e) {
                            "the database stayed busy.".to_string()
                        } else {
                            let first = r.ids.first().copied().unwrap_or(TaskId(0));
                            AppError::core(e, Ctx::Task(first)).message
                        };
                        self.shell.emit(UiEvent::Toast(reject_failed(
                            r.ids.clone(),
                            r.reason.clone(),
                            &why,
                        )));
                    }
                }
            }
            lock(&self.undo).forget_reject(r.id);
        }
        if let Err(e) = self.catch_up() {
            log::warn!("catch-up after rejects failed: {e}");
        }
        self.refresh_menus();
    }

    /// One reject, tried again after each of `reject_retry`'s waits while SQLite is busy.
    fn reject_with_retry(
        &self,
        ids: &[TaskId],
        reason: Option<&str>,
    ) -> airboss_core::Result<RejectOutcome> {
        let mut waits = self.config.reject_retry.iter();
        loop {
            match self.core(|st| st.reject_proposals(ids, reason, &Actor::Me)) {
                Err(e) if is_busy(&e) => match waits.next() {
                    Some(wait) => {
                        log::info!("the database is busy; retrying a reject");
                        std::thread::sleep(*wait);
                    }
                    None => return Err(e),
                },
                done => return done,
            }
        }
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
    fn skipped_proposals_are_named_by_what_happened_to_them() {
        let t = TaskId;
        assert_eq!(skipped_text(&[]), None);
        assert_eq!(
            skipped_text(&[(t(153), Status::Done)]).unwrap(),
            "153 was already accepted; left as is"
        );
        assert_eq!(
            skipped_text(&[(t(1), Status::Dropped), (t(2), Status::Dropped)]).unwrap(),
            "1 and 2 were already dropped; left as is"
        );
        assert_eq!(
            skipped_text(&[(t(1), Status::Todo), (t(2), Status::Dropped)]).unwrap(),
            "1 was already accepted and 2 already dropped; left as is"
        );
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
