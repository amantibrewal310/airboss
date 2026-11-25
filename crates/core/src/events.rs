//! History: writing events inside a change, and reading them back.

use rusqlite::{Connection, Row, params};

use crate::model::{Actor, Event, EventKind, Status};
use crate::{Error, EventId, Result, TaskId};

/// Records one event. Always called inside the transaction of the change it describes (rule 21).
pub(crate) fn record(
    conn: &Connection,
    task: TaskId,
    kind: EventKind,
    to_status: Option<Status>,
    body: Option<&str>,
    actor: &Actor,
) -> Result<EventId> {
    conn.execute(
        "INSERT INTO task_events (task_id, kind, to_status, body, actor) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            task.0,
            kind.as_str(),
            to_status.map(Status::as_str),
            body,
            actor.to_string()
        ],
    )?;
    Ok(EventId(conn.last_insert_rowid()))
}

pub(crate) const EVENT_COLUMNS: &str = "id, task_id, kind, to_status, body, actor, at";

/// Reads a row selected with [`EVENT_COLUMNS`].
pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Event> {
    let kind: String = row.get(2)?;
    let to_status: Option<String> = row.get(3)?;
    let actor: String = row.get(5)?;
    Ok(Event {
        id: EventId(row.get(0)?),
        task: TaskId(row.get(1)?),
        kind: EventKind::parse(&kind).ok_or_else(|| bad_text(2, &kind))?,
        to_status: match to_status {
            Some(s) => Some(Status::parse(&s).ok_or_else(|| bad_text(3, &s))?),
            None => None,
        },
        body: row.get(4)?,
        actor: Actor::parse(&actor).ok_or_else(|| bad_text(5, &actor))?,
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
    let mut stmt = conn.prepare(&format!(
        "SELECT {EVENT_COLUMNS} FROM task_events WHERE task_id = ?1 ORDER BY id"
    ))?;
    let rows = stmt.query_map(params![task.0], from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::from)
}
