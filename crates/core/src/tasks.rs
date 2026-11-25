//! Adding, editing and reading tasks.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::events::{self, bad_text};
use crate::model::{Actor, EventKind, NewTask, Origin, Status, Task, TaskEdit, TaskNode};
use crate::permissions::{Call, allowed};
use crate::sql::query_all;
use crate::tx::Tx;
use crate::{Error, RepoId, Result, TaskId, fault};

impl crate::Store {
    /// Adds one task at the end of its siblings, with a `created` event.
    pub fn add_task(&mut self, new: NewTask, actor: &Actor) -> Result<Task> {
        self.write(|tx| add(tx, new, actor))
    }

    /// One task; `Error::NotFound` if it doesn't exist.
    pub fn task(&self, id: TaskId) -> Result<Task> {
        get(&self.conn, id)
    }

    /// The whole tree, depth-first, siblings in their chosen order.
    pub fn tree(&self) -> Result<Vec<TaskNode>> {
        tree(&self.conn)
    }

    /// Changes a task's title or notes. Only `me` does this (rule 8).
    pub fn edit_task(&mut self, id: TaskId, change: TaskEdit, actor: &Actor) -> Result<Task> {
        self.write(|tx| edit(tx, id, change, actor))
    }

    /// Sets or clears the branch the task's threads share (rule 25). `me` and `launch` only.
    pub fn set_branch(&mut self, id: TaskId, branch: Option<&str>, actor: &Actor) -> Result<Task> {
        self.write(|tx| set_branch(tx, id, branch, actor))
    }

    /// Adds a note to a task's history. Anyone but `plan` may (rule 8).
    pub fn add_note(&mut self, id: TaskId, text: &str, actor: &Actor) -> Result<()> {
        self.write(|tx| add_note(tx, id, text, actor))
    }
}

/// A task's columns (table alias `t`), its repos last as ids joined by commas in repo-name
/// order. The repos come from an index lookup per row, so every reader gets them without a
/// second query.
pub(crate) const TASK_COLUMNS: &str = "t.id, t.parent_id, t.title, t.body, t.status, \
     t.blocked_reason, t.position, t.origin, t.plan_key, t.created_at, t.branch, \
     (SELECT group_concat(tr.repo_id, ',' ORDER BY r.name) \
      FROM task_repos tr JOIN repos r ON r.id = tr.repo_id WHERE tr.task_id = t.id)";

/// How many columns [`TASK_COLUMNS`] selects, so queries can read their own columns after it.
pub(crate) const TASK_COLUMN_COUNT: usize = 12;

/// Reads a row selected with [`TASK_COLUMNS`] (table alias `t`).
pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
    let status: String = row.get(4)?;
    let origin: String = row.get(7)?;
    let repos: Option<String> = row.get(11)?;
    Ok(Task {
        id: TaskId(row.get(0)?),
        parent: row.get::<_, Option<i64>>(1)?.map(TaskId),
        repos: match repos {
            Some(list) => list
                .split(',')
                .map(|id| id.parse().map(RepoId).map_err(|_| bad_text(11, &list)))
                .collect::<rusqlite::Result<_>>()?,
            None => Vec::new(),
        },
        title: row.get(2)?,
        body: row.get(3)?,
        status: status.parse().map_err(|_| bad_text(4, &status))?,
        blocked_reason: row.get(5)?,
        position: row.get(6)?,
        origin: Origin::parse(&origin).ok_or_else(|| bad_text(7, &origin))?,
        plan_key: row.get(8)?,
        branch: row.get(10)?,
        created_at: row.get(9)?,
    })
}

/// `Error::NotFound` unless the task exists.
pub(crate) fn ensure_exists(conn: &Connection, id: TaskId) -> Result<()> {
    ensure_row(conn, "SELECT 1 FROM tasks WHERE id = ?1", id.0)
}

/// `Error::NotFound` unless the repo exists.
pub(crate) fn ensure_repo_exists(conn: &Connection, id: RepoId) -> Result<()> {
    ensure_row(conn, "SELECT 1 FROM repos WHERE id = ?1", id.0)
}

/// `Error::NotFound` unless `sql` finds a row for `id`.
fn ensure_row(conn: &Connection, sql: &str, id: i64) -> Result<()> {
    let found: Option<i64> = conn.query_row(sql, params![id], |r| r.get(0)).optional()?;
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

/// One new task row for [`insert_row`]. `title` is already cleaned.
pub(crate) struct NewRow<'a> {
    pub parent: Option<TaskId>,
    pub repos: &'a [RepoId],
    pub title: &'a str,
    pub body: Option<&'a str>,
    pub origin: Origin,
    pub plan_key: Option<&'a str>,
}

/// Writes one task row at the end of `parent`'s children, and links its repos, with no
/// event. An agent's task is only a proposal until I accept it (rule 8); any other starts
/// `todo`.
pub(crate) fn insert_row(conn: &Connection, row: NewRow<'_>, actor: &Actor) -> Result<TaskId> {
    if let Some(parent) = row.parent {
        // Rule 12: not under a proposed, done or dropped parent.
        crate::positions::ensure_open_parent(conn, parent)?;
    }
    let status = match row.origin {
        Origin::Agent => Status::Proposed,
        _ => Status::Todo,
    };
    let position = next_position(conn, row.parent)?;
    conn.execute(
        "INSERT INTO tasks (parent_id, title, body, status, position, origin, plan_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            row.parent.map(|p| p.0),
            row.title,
            row.body,
            status.as_str(),
            position,
            row.origin.as_str(),
            row.plan_key
        ],
    )?;
    let id = TaskId(conn.last_insert_rowid());
    for &repo in row.repos {
        crate::repos::insert_link(conn, id, repo, actor)?;
    }
    Ok(id)
}

pub(crate) fn add(tx: &mut Tx<'_>, new: NewTask, actor: &Actor) -> Result<Task> {
    allowed(Call::AddTask, actor)?;
    let origin = match actor {
        Actor::Agent(_) => Origin::Agent,
        _ => Origin::Me,
    };
    let id = insert_row(
        tx,
        NewRow {
            parent: new.parent,
            repos: &new.repos,
            title: clean_title(&new.title)?,
            body: new.body.as_deref(),
            origin,
            plan_key: None,
        },
        actor,
    )?;
    fault::hit(fault::TASK_INSERTED)?;
    events::record(tx, id, EventKind::Created, None, None, actor)?;
    get(tx, id)
}

/// The whole tree, depth-first, siblings in position order. Hides proposals I rejected: tasks
/// still `dropped` whose latest status change was a `rejected` event.
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
         WHERE NOT (t.status = 'dropped'
                    AND (SELECT e.kind FROM task_events e
                         WHERE e.task_id = t.id AND e.kind IN ('status', 'rejected')
                         ORDER BY e.id DESC LIMIT 1) = 'rejected')
         ORDER BY w.path"
    );
    query_all(conn, &sql, [], |row| {
        Ok(TaskNode {
            task: from_row(row)?,
            depth: row.get(TASK_COLUMN_COUNT)?,
            waiting: row.get(TASK_COLUMN_COUNT + 1)?,
        })
    })
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
    let changed: Vec<&str> = [
        (title != before.title, "title"),
        (body != before.body, "body"),
    ]
    .into_iter()
    .filter_map(|(c, name)| c.then_some(name))
    .collect();
    if changed.is_empty() {
        return Ok(before);
    }
    tx.execute(
        "UPDATE tasks SET title = ?2, body = ?3 WHERE id = ?1",
        params![id.0, title, body],
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

/// Sets the branch and writes one `edited` event naming it. A blank name clears it, and
/// setting the branch it already has writes nothing (rule 25).
pub(crate) fn set_branch(
    tx: &mut Tx<'_>,
    id: TaskId,
    branch: Option<&str>,
    actor: &Actor,
) -> Result<Task> {
    allowed(Call::SetBranch, actor)?;
    let before = get(tx, id)?;
    let branch = branch.map(str::trim).filter(|b| !b.is_empty());
    if branch == before.branch.as_deref() {
        return Ok(before);
    }
    tx.execute(
        "UPDATE tasks SET branch = ?2 WHERE id = ?1",
        params![id.0, branch],
    )?;
    fault::hit(fault::BRANCH_SET)?;
    let body = match branch {
        Some(b) => format!("branch: {b}"),
        None => "branch cleared".to_string(),
    };
    events::record(tx, id, EventKind::Edited, None, Some(&body), actor)?;
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
    fn a_failed_set_branch_leaves_the_task_and_history_unchanged() {
        let mut store = Store::open_in_memory().unwrap();
        let t = store.add_task(NewTask::new("Before"), &Actor::Me).unwrap();
        let before = counts(&store);
        let committed = store.last_committed.clone();
        fault::arm(fault::BRANCH_SET);
        assert!(store.set_branch(t.id, Some("b"), &Actor::Launch).is_err());
        assert_eq!(counts(&store), before);
        assert_eq!(store.task(t.id).unwrap(), t);
        assert_eq!(store.last_committed, committed);
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
