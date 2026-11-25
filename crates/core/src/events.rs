//! History: writing events inside a change, and reading them back.

use rusqlite::{Connection, Row, params};

use crate::model::{Actor, Event, EventKind, Status};
use crate::permissions::valid_actor;
use crate::sql::{SUBTREE_CTE, query_all};
use crate::tx::Tx;
use crate::{EventId, Result, TaskId};

impl crate::Store {
    /// Everything that happened to a task, oldest first.
    pub fn history(&self, id: TaskId) -> Result<Vec<Event>> {
        history(&self.conn, id)
    }

    /// Up to `limit` events from a task and everything under it, newest first: a parent's
    /// activity feed (rule 28).
    pub fn history_with_subtasks(&self, id: TaskId, limit: u32) -> Result<Vec<Event>> {
        subtree_history(&self.conn, id, limit)
    }
}

/// Records one event inside the change it describes (rule 21) and collects its id for the
/// change notification.
pub(crate) fn record(
    tx: &mut Tx<'_>,
    task: TaskId,
    kind: EventKind,
    to_status: Option<Status>,
    body: Option<&str>,
    actor: &Actor,
) -> Result<()> {
    // An actor that can't be read back would break this task's history for good.
    valid_actor(actor)?;
    tx.execute(
        "INSERT INTO task_events (task_id, kind, to_status, body, actor) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            task.0,
            kind.as_str(),
            to_status.map(Status::as_str),
            body,
            actor.to_string()
        ],
    )?;
    tx.push_event(EventId(tx.last_insert_rowid()));
    Ok(())
}

const EVENT_COLUMNS: &str = "id, task_id, kind, to_status, body, actor, at";

/// Reads a row selected with [`EVENT_COLUMNS`].
fn from_row(row: &Row<'_>) -> rusqlite::Result<Event> {
    let kind: String = row.get(2)?;
    let to_status: Option<String> = row.get(3)?;
    let actor: String = row.get(5)?;
    Ok(Event {
        id: EventId(row.get(0)?),
        task: TaskId(row.get(1)?),
        kind: EventKind::parse(&kind).ok_or_else(|| bad_text(2, &kind))?,
        to_status: match to_status {
            Some(s) => Some(s.parse().map_err(|_| bad_text(3, &s))?),
            None => None,
        },
        body: row.get(4)?,
        actor: actor.parse().map_err(|_| bad_text(5, &actor))?,
        at: row.get(6)?,
    })
}

/// A stored value `core` doesn't recognise: only possible if the file was edited by hand.
pub(crate) fn bad_text(column: usize, text: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        format!("unknown value {text:?}").into(),
    )
}

/// A task's history, oldest first.
pub(crate) fn history(conn: &Connection, task: TaskId) -> Result<Vec<Event>> {
    crate::tasks::ensure_exists(conn, task)?;
    query_all(
        conn,
        &format!("SELECT {EVENT_COLUMNS} FROM task_events WHERE task_id = ?1 ORDER BY id"),
        params![task.0],
        from_row,
    )
}

/// The newest `limit` events of `task`'s subtree, newest first.
pub(crate) fn subtree_history(conn: &Connection, task: TaskId, limit: u32) -> Result<Vec<Event>> {
    crate::tasks::ensure_exists(conn, task)?;
    query_all(
        conn,
        &format!(
            "{SUBTREE_CTE}
             SELECT {EVENT_COLUMNS} FROM task_events
             WHERE task_id IN (SELECT id FROM sub)
             ORDER BY id DESC LIMIT ?2"
        ),
        params![task.0, limit],
        from_row,
    )
}

/// Every event with an id above `cursor`, in id order.
pub(crate) fn after(conn: &Connection, cursor: EventId) -> Result<Vec<Event>> {
    query_all(
        conn,
        &format!("SELECT {EVENT_COLUMNS} FROM task_events WHERE id > ?1 ORDER BY id"),
        params![cursor.0],
        from_row,
    )
}
