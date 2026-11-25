//! Adding and reading tasks and repos.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::events::{self, bad_text};
use crate::model::{Actor, EventKind, NewTask, Origin, Repo, Status, Task, TaskNode};
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
        status: Status::parse(&status).ok_or_else(|| bad_text(5, &status))?,
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
fn next_position(conn: &Connection, parent: Option<TaskId>) -> Result<u32> {
    Ok(conn.query_row(
        "SELECT ifnull(max(position), 0) + 1 FROM tasks WHERE parent_id IS ?1",
        params![parent.map(|p| p.0)],
        |r| r.get(0),
    )?)
}

pub(crate) fn add(conn: &Connection, new: NewTask, actor: &Actor) -> Result<Task> {
    let (origin, status) = match actor {
        Actor::Me => (Origin::Me, Status::Todo),
        _ => return Err(Error::NotAllowed),
    };
    let title = new.title.trim();
    if title.is_empty() {
        return Err(Error::EmptyTitle);
    }
    if let Some(parent) = new.parent {
        ensure_exists(conn, parent)?;
    }
    if let Some(repo) = new.repo {
        ensure_repo_exists(conn, repo)?;
    }
    let position = next_position(conn, new.parent)?;
    conn.execute(
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
    let id = TaskId(conn.last_insert_rowid());
    fault::hit("tasks::add after insert")?;
    events::record(conn, id, EventKind::Created, None, None, actor)?;
    get(conn, id)
}

/// The whole tree, depth-first, siblings in position order.
pub(crate) fn tree(conn: &Connection) -> Result<Vec<TaskNode>> {
    let sql = format!(
        "WITH RECURSIVE walk(id, depth, path) AS (
             SELECT id, 0, printf('%08d', position) FROM tasks WHERE parent_id IS NULL
             UNION ALL
             SELECT c.id, w.depth + 1, w.path || '.' || printf('%08d', c.position)
             FROM tasks c JOIN walk w ON c.parent_id = w.id
         )
         SELECT {TASK_COLUMNS}, w.depth FROM walk w JOIN tasks t ON t.id = w.id ORDER BY w.path"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(TaskNode {
            task: from_row(row)?,
            depth: row.get(11)?,
            waiting: false,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::from)
}

fn ensure_repo_exists(conn: &Connection, id: RepoId) -> Result<()> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM repos WHERE id = ?1", params![id.0], |r| {
            r.get(0)
        })
        .optional()?;
    found.map(|_| ()).ok_or(Error::NotFound)
}

pub(crate) fn add_repo(
    conn: &Connection,
    name: &str,
    path: &Path,
    remote: Option<&str>,
    actor: &Actor,
) -> Result<Repo> {
    if *actor != Actor::Me {
        return Err(Error::NotAllowed);
    }
    let path = path.to_string_lossy();
    conn.execute(
        "INSERT INTO repos (name, path, remote_url) VALUES (?1, ?2, ?3)",
        params![name, path, remote],
    )?;
    Ok(Repo {
        id: RepoId(conn.last_insert_rowid()),
        name: name.to_string(),
        path: path.into_owned(),
        remote_url: remote.map(str::to_string),
    })
}

pub(crate) fn repos(conn: &Connection) -> Result<Vec<Repo>> {
    let mut stmt = conn.prepare("SELECT id, name, path, remote_url FROM repos ORDER BY name")?;
    let rows = stmt.query_map([], |r| {
        Ok(Repo {
            id: RepoId(r.get(0)?),
            name: r.get(1)?,
            path: r.get(2)?,
            remote_url: r.get(3)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::from)
}
