//! Task writes, undo and deferred rejects. Each write makes its `core` calls (one lock each),
//! then catches up (which tells the services, after the lock is released) and returns its
//! patch.

use std::time::Instant;

use airboss_core::{Actor, EventId, NewTask, Status, TaskEdit, TaskId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::classify::classify;
use crate::commands::launch;
use crate::dto::{
    Created, LaunchAttempt, NavOpen, Patch, StatusDto, ToastDto, ToastTone, UndoInfo, UndoResult,
    Undoable,
};
use crate::errors::{AppError, AppResult, Ctx, ErrorCode, MSG_DEP_FAMILY};
use crate::shell::UiEvent;
use crate::state::{AppState, Caller, lock};
use crate::undo::Inverse;

/// What `create_task` does after creating.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Then {
    #[default]
    None,
    DoIt,
    PlanIt,
}

/// One link pasted into New task: the text and an optional title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NewLinkArg {
    pub text: String,
    pub title: Option<String>,
}

/// Up or down among its siblings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Down,
}

impl AppState {
    /// The newest event id now, before a write: later events by others refuse its undo.
    pub(crate) fn latest_event(&self) -> EventId {
        self.core(|s| s.latest_event_id()).unwrap_or(EventId(0))
    }

    /// Pushes an undo entry and relabels the Task menu's Undo.
    pub(crate) fn push_undo(
        &self,
        label: String,
        inverse: Vec<Inverse>,
        tasks: Vec<TaskId>,
        after: EventId,
    ) -> UndoInfo {
        let info = lock(&self.undo).push(label, inverse, tasks, after);
        self.refresh_menus();
        info
    }

    /// When quick add asked for something that went wrong, the main window shows it.
    pub(crate) fn surface(&self, caller: Caller, task: TaskId, toast: ToastDto) {
        if caller == Caller::QuickAdd {
            self.shell.show_main();
            self.shell.emit(UiEvent::NavOpen(NavOpen {
                view: None,
                task: Some(task),
                needs_key: None,
            }));
            self.shell.emit(UiEvent::Toast(toast));
        }
    }
}

/// `create_task`: the task with its repos in one change, then each link, then Do it or Plan
/// it if asked. A link or launch that fails after the task exists leaves the task.
#[allow(clippy::too_many_arguments)]
pub fn create_task(
    s: &AppState,
    c: Caller,
    title: String,
    body: Option<String>,
    parent: Option<TaskId>,
    repos: Vec<airboss_core::RepoId>,
    links: Vec<NewLinkArg>,
    then: Option<Then>,
) -> AppResult<Created> {
    let new = NewTask {
        title,
        parent,
        repos,
        body: body.filter(|b| !b.trim().is_empty()),
    };
    let ctx = Ctx::Move(parent.unwrap_or(TaskId(0)));
    let task = s
        .core(|st| st.add_task(new, &Actor::Me))
        .map_err(|e| match (e, parent) {
            (airboss_core::Error::NotFound, Some(p)) => AppError::task_gone(p),
            (e, _) => AppError::core(e, ctx),
        })?;
    let id = task.id;
    let mut failed_links = Vec::new();
    for l in &links {
        let added = classify(&l.text, l.title.as_deref(), &s.config.home)
            .and_then(|nl| s.core_as(Ctx::Link, |st| st.link(id, nl, &Actor::Me)));
        if added.is_err() {
            failed_links.push(l.title.clone().unwrap_or_else(|| l.text.clone()));
        }
    }
    let patch = s.after_write()?;
    if let Some(first) = failed_links.first() {
        s.surface(
            c,
            id,
            ToastDto {
                text: format!("Created {}, but couldn't link {first}", id.0),
                tone: ToastTone::Warning,
                actions: Vec::new(),
                undo: None,
                task: Some(id),
            },
        );
    }
    let launch = match then.unwrap_or_default() {
        Then::None => None,
        Then::DoIt => Some(launch::do_it(s, c, id, None, None)),
        Then::PlanIt => Some(launch::plan_it(s, c, id, None)),
    }
    .map(|r| match r {
        Ok(outcome) => LaunchAttempt {
            outcome: Some(outcome),
            error: None,
        },
        Err(e) => LaunchAttempt {
            outcome: None,
            error: Some(e),
        },
    });
    Ok(Created {
        id,
        patch,
        launch,
        failed_links,
    })
}

/// Renames a task.
pub fn rename_task(s: &AppState, _c: Caller, id: TaskId, title: String) -> AppResult<Patch> {
    s.core_as(Ctx::Task(id), |st| {
        st.edit_task(id, TaskEdit::default().title(title), &Actor::Me)
    })?;
    s.after_write()
}

/// Replaces a task's notes; `None` or blank clears them.
pub fn edit_notes(s: &AppState, _c: Caller, id: TaskId, body: Option<String>) -> AppResult<Patch> {
    let body = body.filter(|b| !b.trim().is_empty());
    s.core_as(Ctx::Task(id), |st| {
        st.edit_task(id, TaskEdit::default().body(body.as_deref()), &Actor::Me)
    })?;
    s.after_write()
}

/// The undo label for a status change: "Mark done 144", "Drop 144", "Reopen 144" …
fn status_label(id: TaskId, from: StatusDto, to: StatusDto) -> String {
    match to {
        StatusDto::Done => format!("Mark done {}", id.0),
        StatusDto::Dropped => format!("Drop {}", id.0),
        _ if !from.is_open() => format!("Reopen {}", id.0),
        StatusDto::Blocked => format!("Mark blocked {}", id.0),
        to => format!("Set {} to {}", id.0, to.label()),
    }
}

/// The status to undo back to: proposed can't be set by me, so it comes back as todo.
fn undo_status(s: StatusDto) -> Status {
    match s {
        StatusDto::Proposed => Status::Todo,
        other => other.into(),
    }
}

/// Changes a task's status (blocked needs `note`), with an undo entry. Dropping takes every
/// open subtask with it, and undo restores each one's previous status.
pub fn set_status(
    s: &AppState,
    _c: Caller,
    id: TaskId,
    status: StatusDto,
    note: Option<String>,
) -> AppResult<Undoable> {
    // Previous statuses come from the view model before the change.
    let before: Vec<(TaskId, StatusDto, Option<String>)> = s.vm(|vm| {
        let ids = if status == StatusDto::Dropped {
            vm.index.subtree(id)
        } else {
            vec![id]
        };
        ids.into_iter()
            .filter_map(|t| vm.rows.get(&t))
            .filter(|r| r.id == id || r.status.is_open())
            .map(|r| (r.id, r.status, r.blocked_reason.clone()))
            .collect()
    });
    let Some(&(_, from, _)) = before.first() else {
        return Err(AppError::task_gone(id));
    };
    let after = s.latest_event();
    let note = note.filter(|n| !n.trim().is_empty());
    s.core_as(Ctx::Task(id), |st| {
        st.set_status(id, status.into(), note.as_deref(), &Actor::Me)
    })?;
    let patch = s.after_write()?;
    if from == status && status != StatusDto::Dropped {
        return Ok(Undoable { patch, undo: None });
    }
    let inverse = before
        .iter()
        .filter(|(_, prev, _)| *prev != StatusDto::Dropped)
        .map(|(t, prev, reason)| Inverse::SetStatus {
            id: *t,
            status: undo_status(*prev),
            note: if *prev == StatusDto::Blocked {
                reason.clone()
            } else {
                None
            },
        })
        .collect();
    let tasks = before.iter().map(|(t, _, _)| *t).collect();
    let undo = s.push_undo(status_label(id, from, status), inverse, tasks, after);
    Ok(Undoable {
        patch,
        undo: Some(undo),
    })
}

/// Space: done, or back to what it was before done when reopening.
pub fn toggle_done(s: &AppState, c: Caller, id: TaskId) -> AppResult<Undoable> {
    let row = s.row(id).ok_or_else(|| AppError::task_gone(id))?;
    if row.status != StatusDto::Done {
        return set_status(s, c, id, StatusDto::Done, None);
    }
    // The status before the latest move to done, from history.
    let history = s.core_as(Ctx::Task(id), |st| st.history(id))?;
    let done_at = history
        .iter()
        .rposition(|e| e.to_status == Some(Status::Done));
    let previous = done_at
        .and_then(|i| {
            history[..i]
                .iter()
                .rev()
                .find(|e| e.to_status.is_some_and(|s| s != Status::Done))
                .map(|e| (e.to_status, e.body.clone()))
        })
        .and_then(|(st, body)| st.map(|st| (st, body)));
    let (to, note) = match previous {
        Some((Status::Blocked, reason)) => (StatusDto::Blocked, reason),
        Some((Status::Doing, _)) => (StatusDto::Doing, None),
        Some((Status::InReview, _)) => (StatusDto::InReview, None),
        _ => (StatusDto::Todo, None),
    };
    let note = if to == StatusDto::Blocked && note.is_none() {
        // A blocked status needs its reason; without one, reopen as todo.
        return set_status(s, c, id, StatusDto::Todo, None);
    } else {
        note
    };
    set_status(s, c, id, to, note)
}

/// Accepts proposals: each to todo. Not undoable (`core` can't make a proposal again).
pub fn accept_proposals(s: &AppState, _c: Caller, ids: Vec<TaskId>) -> AppResult<Patch> {
    let mut first_err = None;
    for id in ids {
        if let Err(e) = s.core_as(Ctx::Task(id), |st| {
            st.set_status(id, Status::Todo, None, &Actor::Me)
        }) {
            first_err.get_or_insert(e);
        }
    }
    let patch = s.after_write()?;
    match first_err {
        Some(e) => Err(e),
        None => Ok(patch),
    }
}

/// Rejects proposals 5 s from now (or at quit); until then undo cancels it.
pub fn reject_proposals(
    s: &AppState,
    _c: Caller,
    ids: Vec<TaskId>,
    reason: Option<String>,
) -> AppResult<UndoInfo> {
    if ids.is_empty() {
        return Err(AppError::new(ErrorCode::NotFound, "Nothing to reject."));
    }
    let reject = s.rejects.add(
        ids.clone(),
        reason.filter(|r| !r.trim().is_empty()),
        Instant::now() + s.config.reject_delay,
    );
    let label = match ids.as_slice() {
        [one] => format!("Reject {}", one.0),
        many => format!("Reject {} proposals", many.len()),
    };
    let after = s.latest_event();
    Ok(s.push_undo(label, vec![Inverse::CancelReject(reject)], ids, after))
}

/// Moves a task under `parent` (top level with none) at `position` (last with none).
pub fn move_task(
    s: &AppState,
    _c: Caller,
    id: TaskId,
    parent: Option<TaskId>,
    position: Option<u32>,
) -> AppResult<Undoable> {
    let row = s.row(id).ok_or_else(|| AppError::task_gone(id))?;
    let after = s.latest_event();
    let position = position.unwrap_or(u32::MAX);
    s.core(|st| st.move_task(id, parent, position, &Actor::Me))
        .map_err(|e| move_error(s, e, id, parent))?;
    let patch = s.after_write()?;
    let moved = s
        .row(id)
        .is_some_and(|r| r.parent != row.parent || r.position != row.position);
    if !moved {
        return Ok(Undoable { patch, undo: None });
    }
    let undo = s.push_undo(
        format!("Move {}", id.0),
        vec![Inverse::Move {
            id,
            parent: row.parent,
            position: row.position,
        }],
        vec![id],
        after,
    );
    Ok(Undoable {
        patch,
        undo: Some(undo),
    })
}

/// A refused move in the user's words: a closed parent, or a dependency that would join a
/// task to its own ancestor or descendant.
fn move_error(
    s: &AppState,
    e: airboss_core::Error,
    id: TaskId,
    parent: Option<TaskId>,
) -> AppError {
    match e {
        airboss_core::Error::InvalidTarget => {
            let parent_open = parent
                .and_then(|p| s.row(p))
                .is_some_and(|r| r.status.is_open() && r.status != StatusDto::Proposed);
            if parent_open {
                AppError::new(
                    ErrorCode::InvalidTarget,
                    "That move would make a task wait on its own parent or subtask.",
                )
            } else {
                AppError::core(airboss_core::Error::InvalidTarget, Ctx::Move(id))
            }
        }
        airboss_core::Error::AlreadyExists => AppError::new(
            ErrorCode::AlreadyExists,
            "That parent already has a step from the same plan.",
        ),
        airboss_core::Error::NotFound => match parent {
            Some(p) if s.row(p).is_none() => AppError::task_gone(p),
            _ => AppError::task_gone(id),
        },
        e => AppError::core(e, Ctx::Move(id)),
    }
}

/// ⌥⌘↑ / ⌥⌘↓: one place among its siblings; at either end nothing happens.
pub fn reorder(s: &AppState, c: Caller, id: TaskId, direction: Direction) -> AppResult<Undoable> {
    let (row, siblings) = s.vm(|vm| {
        let row = vm.rows.get(&id).cloned();
        let n = row
            .as_ref()
            .map(|r| vm.index.children.get(&r.parent).map_or(0, Vec::len))
            .unwrap_or(0);
        (row, n)
    });
    let row = row.ok_or_else(|| AppError::task_gone(id))?;
    let to = match direction {
        Direction::Up if row.position > 1 => row.position - 1,
        Direction::Down if (row.position as usize) < siblings => row.position + 1,
        _ => {
            return Ok(Undoable {
                patch: Patch::empty(s.seq()),
                undo: None,
            });
        }
    };
    move_task(s, c, id, row.parent, Some(to))
}

/// Links a repo to a task as me.
pub fn link_repo(
    s: &AppState,
    _c: Caller,
    id: TaskId,
    repo: airboss_core::RepoId,
) -> AppResult<Patch> {
    s.core_as(Ctx::Task(id), |st| st.link_repo(id, repo, &Actor::Me))?;
    s.after_write()
}

/// Unlinks a repo from a task.
pub fn unlink_repo(
    s: &AppState,
    _c: Caller,
    id: TaskId,
    repo: airboss_core::RepoId,
) -> AppResult<Patch> {
    s.core_as(Ctx::Repo(id), |st| st.unlink_repo(id, repo, &Actor::Me))?;
    s.after_write()
}

/// Sets (or, blank, clears) a task's branch.
pub fn set_branch(
    s: &AppState,
    _c: Caller,
    id: TaskId,
    branch: Option<String>,
) -> AppResult<Patch> {
    let branch = branch.filter(|b| !b.trim().is_empty());
    s.core_as(Ctx::Branch(id), |st| {
        st.set_branch(id, branch.as_deref(), &Actor::Me)
    })?;
    s.after_write()
}

/// `id` waits on `waits_on`.
pub fn add_dependency(s: &AppState, _c: Caller, id: TaskId, waits_on: TaskId) -> AppResult<Patch> {
    let ctx = Ctx::Dependency { id, waits_on };
    if id == waits_on {
        return Err(AppError::new(ErrorCode::InvalidTarget, MSG_DEP_FAMILY));
    }
    s.core(|st| st.add_dependency(id, waits_on, &Actor::Me))
        .map_err(|e| match e {
            airboss_core::Error::NotFound if s.row(id).is_some() => AppError::task_gone(waits_on),
            e => AppError::core(e, ctx),
        })?;
    s.after_write()
}

/// `id` no longer waits on `waits_on`.
pub fn remove_dependency(
    s: &AppState,
    _c: Caller,
    id: TaskId,
    waits_on: TaskId,
) -> AppResult<Patch> {
    s.core_as(Ctx::Dependency { id, waits_on }, |st| {
        st.remove_dependency(id, waits_on, &Actor::Me)
    })?;
    s.after_write()
}

/// "Leave a note for the agent…".
pub fn add_note(s: &AppState, _c: Caller, id: TaskId, text: String) -> AppResult<Patch> {
    s.core_as(Ctx::Task(id), |st| st.add_note(id, &text, &Actor::Me))?;
    s.after_write()
}

/// Undoes entry `entry` (the newest with none), unless someone else changed one of its
/// tasks since; then it's refused and dropped.
pub fn undo(s: &AppState, _c: Caller, entry: Option<u64>) -> AppResult<UndoResult> {
    let Some(e) = lock(&s.undo).take(entry) else {
        s.refresh_menus();
        return Ok(UndoResult {
            patch: None,
            message: "Nothing to undo.".into(),
        });
    };
    s.refresh_menus();
    if !e.tasks.is_empty() {
        let later = s.core_as(Ctx::Read, |st| st.events_after(e.after))?;
        if let Some(ev) = later
            .iter()
            .find(|ev| e.tasks.contains(&ev.task) && ev.actor != Actor::Me)
        {
            return Err(AppError::undo_refused(ev.task));
        }
    }
    let mut touched_core = false;
    for inv in &e.inverse {
        match inv {
            Inverse::SetStatus { id, status, note } => {
                touched_core = true;
                s.core_as(Ctx::Task(*id), |st| {
                    st.set_status(*id, *status, note.as_deref(), &Actor::Me)
                })?;
            }
            Inverse::Move {
                id,
                parent,
                position,
            } => {
                touched_core = true;
                s.core_as(Ctx::Move(*id), |st| {
                    st.move_task(*id, *parent, *position, &Actor::Me)
                })?;
            }
            Inverse::Restore(key) => s.services.attention.restore(key)?,
            Inverse::CancelReject(id) => {
                if !s.rejects.cancel(*id) {
                    return Err(AppError::new(
                        ErrorCode::UndoRefused,
                        "That reject was already saved; nothing was undone.",
                    ));
                }
            }
        }
    }
    let patch = if touched_core {
        Some(s.after_write()?)
    } else {
        None
    };
    Ok(UndoResult {
        patch,
        message: format!("Undid “{}”", e.label),
    })
}
