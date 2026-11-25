//! Needs me: dismiss, snooze and restore, through `attention`. Dismiss and Snooze push an undo
//! entry whose inverse is `attention.restore(key)`.

use airboss_services::NeedsKey;

use crate::dto::{UndoEntry, UndoableWrite};
use crate::errors::{AppError, AppResult};
use crate::state::{AppState, Caller};
use crate::undo::Inverse;

/// The task an item is about, for its undo label.
fn item_task(s: &AppState, key: &NeedsKey) -> Option<airboss_core::TaskId> {
    s.vm(|vm| {
        vm.needs
            .items
            .iter()
            .find(|i| i.key == *key)
            .map(|i| i.task)
    })
}

fn label(verb: &str, task: Option<airboss_core::TaskId>) -> String {
    match task {
        Some(t) => format!("{verb} {}", t.0),
        None => verb.to_string(),
    }
}

/// A Needs me write touches no row yet: `attention` recomputes, and a patch follows.
fn hidden(undo: UndoEntry) -> UndoableWrite {
    UndoableWrite {
        patch: None,
        undo: Some(undo),
    }
}

/// Hides this occurrence for good (Dismiss, and Keep open on Finish it?).
pub fn needs_dismiss(s: &AppState, _c: Caller, key: String) -> AppResult<UndoableWrite> {
    let key = NeedsKey(key);
    let task = item_task(s, &key);
    s.services.attention.dismiss(&key).map_err(AppError::from)?;
    let after = s.latest_event();
    Ok(hidden(s.push_undo(
        label("Dismiss", task),
        vec![Inverse::Restore(key)],
        Vec::new(),
        after,
    )))
}

/// Hides it until `until`, or `attention`'s default 3 days with none.
pub fn needs_snooze(
    s: &AppState,
    _c: Caller,
    key: String,
    until: Option<String>,
) -> AppResult<UndoableWrite> {
    let key = NeedsKey(key);
    let task = item_task(s, &key);
    s.services
        .attention
        .snooze(&key, until)
        .map_err(AppError::from)?;
    let after = s.latest_event();
    Ok(hidden(s.push_undo(
        label("Snooze", task),
        vec![Inverse::Restore(key)],
        Vec::new(),
        after,
    )))
}

/// Undoes a dismiss or snooze.
pub fn needs_restore(s: &AppState, _c: Caller, key: String) -> AppResult<()> {
    s.services
        .attention
        .restore(&NeedsKey(key))
        .map_err(AppError::from)
}
