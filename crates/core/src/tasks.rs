//! Adding and reading tasks and repos.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::events::{self, bad_text};
use crate::model::{Actor, EventKind, NewTask, Origin, Repo, Status, Task, TaskEdit, TaskNode};
use crate::permissions::{Call, allowed};
use crate::sql::query_all;
use crate::status::DROPPED_WITH;
use crate::tx::Tx;
use crate::{Error, RepoId, Result, TaskId, fault};

pub(crate) const TASK_COLUMNS: &str = "t.id, t.parent_id, t.repo_id, t.title, t.body, t.status, \
     t.blocked_reason, t.position, t.origin, t.plan_key, t.created_at";

/// Reads a row selected with [`TASK_COLUMNS`] (table alias `t`).
pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
    let status: String = row.get(5)?;
    let origin: String = row.get(8)?;
    Ok(Task {
        id: TaskId(row.get(0)?),
        parent: row.get::<_, Option<i64>>(1)?.map(TaskId),
        repo: row.get::<_, Option<i64>>(2)?.map(RepoId),
        title: row.get(3)?,
        body: row.get(4)?,
        status: status.parse().map_err(|_| bad_text(5, &status))?,
        blocked_reason: row.get(6)?,
        position: row.get(7)?,
        origin: Origin::parse(&origin).ok_or_else(|| bad_text(8, &origin))?,
        plan_key: row.get(9)?,
        created_at: row.get(10)?,
    })
}

/// `Error::NotFound` unless the task exists.
pub(crate) fn ensure_exists(conn: &Connection, id: TaskId) -> Result<()> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM tasks WHERE id = ?1", params![id.0], |r| {
            r.get(0)
        })
        .optional()?;
    found.map(|_| ()).ok_or(Error::NotFound)
}

pub(crate) fn get(conn: &Connection, id: TaskId) -> Result<Task> {
    conn.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM tasks t WHERE t.id = ?1"),
        params![id.0],
        from_row,
    )
    .optional()?
    .ok_or(Error::NotFound)
}

/// The next free position among `parent`'s children (rule 13: 1-based, gap-free).
pub(crate) fn next_position(conn: &Connection, parent: Option<TaskId>) -> Result<u32> {
    Ok(conn.query_row(
        "SELECT ifnull(max(position), 0) + 1 FROM tasks WHERE parent_id IS ?1",
        params![parent.map(|p| p.0)],
        |r| r.get(0),
    )?)
}

/// A task title with surrounding spaces removed; `Error::EmptyTitle` if nothing is left.
pub(crate) fn clean_title(title: &str) -> Result<&str> {
    let title = title.trim();
    if title.is_empty() {
        Err(Error::EmptyTitle)
    } else {
        Ok(title)
    }
}

pub(crate) fn add(tx: &mut Tx<'_>, new: NewTask, actor: &Actor) -> Result<Task> {
    allowed(Call::AddTask, actor)?;
    // An agent's task is only a proposal until I accept it (rule 8).
    let (origin, status) = match actor {
        Actor::Agent(_) => (Origin::Agent, Status::Proposed),
        _ => (Origin::Me, Status::Todo),
    };
    let title = clean_title(&new.title)?;
    if let Some(parent) = new.parent {
        // Rule 12: not under a proposed, done or dropped parent.
        crate::positions::ensure_open_parent(tx, parent)?;
    }
    if let Some(repo) = new.repo {
        ensure_repo_exists(tx, repo)?;
    }
    let position = next_position(tx, new.parent)?;
    tx.execute(
        "INSERT INTO tasks (parent_id, repo_id, title, body, status, position, origin)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            new.parent.map(|p| p.0),
            new.repo.map(|r| r.0),
            title,
            new.body,
            status.as_str(),
            position,
            origin.as_str()
        ],
    )?;
    let id = TaskId(tx.last_insert_rowid());
    fault::hit(fault::TASK_INSERTED)?;
    events::record(tx, id, EventKind::Created, None, None, actor)?;
    get(tx, id)
}

/// The whole tree, depth-first, siblings in position order. Hides proposals I rejected:
/// agent tasks dropped without ever being accepted, other than by dropping their parent.
pub(crate) fn tree(conn: &Connection) -> Result<Vec<TaskNode>> {
    let sql = format!(
        "WITH RECURSIVE walk(id, depth, path) AS (
             SELECT id, 0, printf('%010d', position) FROM tasks WHERE parent_id IS NULL
             UNION ALL
             SELECT c.id, w.depth + 1, w.path || '.' || printf('%010d', c.position)
             FROM tasks c JOIN walk w ON c.parent_id = w.id
         )
         SELECT {TASK_COLUMNS}, w.depth,
                EXISTS (SELECT 1 FROM task_deps d JOIN tasks b ON b.id = d.waits_on_id
                        WHERE d.task_id = t.id AND b.status NOT IN ('done', 'dropped'))
         FROM walk w JOIN tasks t ON t.id = w.id
         WHERE NOT (t.origin = 'agent' AND t.status = 'dropped'
                    AND NOT EXISTS (SELECT 1 FROM task_events e
                                    WHERE e.task_id = t.id AND e.kind = 'status'
                                      AND (e.to_status NOT IN ('proposed', 'dropped')
                                           OR e.body LIKE '{DROPPED_WITH}%')))
         ORDER BY w.path"
    );
    query_all(conn, &sql, [], |row| {
        Ok(TaskNode {
            task: from_row(row)?,
            depth: row.get(11)?,
            waiting: row.get(12)?,
        })
    })
}

pub(crate) fn ensure_repo_exists(conn: &Connection, id: RepoId) -> Result<()> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM repos WHERE id = ?1", params![id.0], |r| {
            r.get(0)
        })
        .optional()?;
    found.map(|_| ()).ok_or(Error::NotFound)
}

pub(crate) fn add_repo(
    tx: &mut Tx<'_>,
    name: &str,
    path: &Path,
    remote: Option<&str>,
    actor: &Actor,
) -> Result<Repo> {
    allowed(Call::AddRepo, actor)?;
    let path = path.to_string_lossy();
    tx.execute(
        "INSERT INTO repos (name, path, remote_url) VALUES (?1, ?2, ?3)",
        params![name, path, remote],
    )
    .map_err(|e| match e {
        rusqlite::Error::SqliteFailure(f, _)
            if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
        {
            Error::AlreadyExists
        }
        other => Error::Db(other),
    })?;
    Ok(Repo {
        id: RepoId(tx.last_insert_rowid()),
        name: name.to_string(),
        path: path.into_owned(),
        remote_url: remote.map(str::to_string),
    })
}

pub(crate) fn repos(conn: &Connection) -> Result<Vec<Repo>> {
    query_all(
        conn,
        "SELECT id, name, path, remote_url FROM repos ORDER BY name",
        [],
        |r| {
            Ok(Repo {
                id: RepoId(r.get(0)?),
                name: r.get(1)?,
                path: r.get(2)?,
                remote_url: r.get(3)?,
            })
        },
    )
}

/// Applies `me`'s edit and writes one `edited` event naming the fields that changed.
/// An edit that changes nothing writes nothing.
pub(crate) fn edit(tx: &mut Tx<'_>, id: TaskId, edit: TaskEdit, actor: &Actor) -> Result<Task> {
    allowed(Call::EditTask, actor)?;
    let before = get(tx, id)?;
    let title = match &edit.title {
        Some(t) => clean_title(t)?.to_string(),
        None => before.title.clone(),
    };
    let body = edit.body.unwrap_or_else(|| before.body.clone());
    let repo = edit.repo.unwrap_or(before.repo);
    if let Some(r) = repo {
        ensure_repo_exists(tx, r)?;
    }
    let changed: Vec<&str> = [
        (title != before.title, "title"),
        (body != before.body, "body"),
        (repo != before.repo, "repo"),
    ]
    .into_iter()
    .filter_map(|(c, name)| c.then_some(name))
    .collect();
    if changed.is_empty() {
        return Ok(before);
    }
    tx.execute(
        "UPDATE tasks SET title = ?2, body = ?3, repo_id = ?4 WHERE id = ?1",
        params![id.0, title, body, repo.map(|r| r.0)],
    )?;
    fault::hit(fault::TASK_EDITED)?;
    events::record(
        tx,
        id,
        EventKind::Edited,
        None,
        Some(&changed.join(", ")),
        actor,
    )?;
    get(tx, id)
}

/// Writes a `note` event; rule 4 says a note has text, so a blank one is `NoteRequired`.
pub(crate) fn add_note(tx: &mut Tx<'_>, id: TaskId, text: &str, actor: &Actor) -> Result<()> {
    allowed(Call::AddNote, actor)?;
    if text.trim().is_empty() {
        return Err(Error::NoteRequired);
    }
    ensure_exists(tx, id)?;
    events::record(tx, id, EventKind::Note, None, Some(text), actor)?;
    fault::hit(fault::NOTE_RECORDED)
}

impl crate::Store {
    /// Changes a task's title, notes or repo. Only `me` does this (rule 8).
    pub fn edit_task(&mut self, id: TaskId, change: TaskEdit, actor: &Actor) -> Result<Task> {
        self.write(|tx| edit(tx, id, change, actor))
    }

    /// Adds a note to a task's history. Anyone but `plan` may (rule 8).
    pub fn add_note(&mut self, id: TaskId, text: &str, actor: &Actor) -> Result<()> {
        self.write(|tx| add_note(tx, id, text, actor))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::{Store, fault};

    fn counts(store: &Store) -> (i64, i64) {
        let n = |sql: &str| store.conn.query_row(sql, [], |r| r.get(0)).unwrap();
        (
            n("SELECT count(*) FROM task_events"),
            n("SELECT count(*) FROM tasks WHERE title = 'Before'"),
        )
    }

    #[test]
    fn a_failed_edit_leaves_the_task_and_history_unchanged() {
        let mut store = Store::open_in_memory().unwrap();
        let t = store.add_task(NewTask::new("Before"), &Actor::Me).unwrap();
        let before = counts(&store);
        fault::arm(fault::TASK_EDITED);
        let edit = TaskEdit::default().title("After");
        assert!(store.edit_task(t.id, edit, &Actor::Me).is_err());
        assert_eq!(counts(&store), before);
        assert_eq!(store.task(t.id).unwrap(), t);
    }

    #[test]
    fn a_failed_note_leaves_no_event() {
        let mut store = Store::open_in_memory().unwrap();
        let t = store.add_task(NewTask::new("Before"), &Actor::Me).unwrap();
        let before = counts(&store);
        fault::arm(fault::NOTE_RECORDED);
        assert!(store.add_note(t.id, "lost", &Actor::Me).is_err());
        assert_eq!(counts(&store), before);
    }
}
