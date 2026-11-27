//! Adding, editing and reading tasks.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::events::{self, bad_text};
use crate::model::{Actor, EventKind, NewTask, Origin, Status, Task, TaskEdit, TaskNode};
use crate::permissions::{Call, allowed};
use crate::sql::query_all;
use crate::tx::Tx;
use crate::{Error, EventId, RepoId, Result, TaskId, deps, fault, rollup};

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

    /// True when the task has steps: subtasks that aren't proposals. Its status is then
    /// worked out from them, and it takes no branch, PR or status change but a drop (rule 10).
    pub fn has_steps(&self, id: TaskId) -> Result<bool> {
        ensure_exists(&self.conn, id)?;
        rollup::has_steps(&self.conn, id)
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

/// One task, showing what its steps add up to when it has any (rule 10).
pub(crate) fn get(conn: &Connection, id: TaskId) -> Result<Task> {
    let task = conn
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks t WHERE t.id = ?1"),
            params![id.0],
            from_row,
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    shown(conn, task)
}

/// `task` as read from its row, with what its steps add up to in place of its stored status
/// when it has any (rule 10).
pub(crate) fn shown(conn: &Connection, mut task: Task) -> Result<Task> {
    if let Some(s) = rollup::shown(conn, task.id)? {
        task.status = s.status;
        task.blocked_reason = s.blocked_reason;
    }
    Ok(task)
}

/// Every task in `tasks` as [`shown`] gives it.
pub(crate) fn all_shown(conn: &Connection, tasks: Vec<Task>) -> Result<Vec<Task>> {
    tasks.into_iter().map(|t| shown(conn, t)).collect()
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
///
/// Each node's own newest event comes from one index lookup in the same query; the
/// subtree's newest is then one pass in Rust from the leaves up (rule 28). A parent's status
/// is worked out from its steps (rule 10), and waiting from what its blockers show (rule 16).
pub(crate) fn tree(conn: &Connection) -> Result<Vec<TaskNode>> {
    Ok(tree_and_edges(conn)?.0)
}

/// Every `(waiting, waits_on)` pair.
pub(crate) type Edges = Vec<(TaskId, TaskId)>;

/// [`tree`], and every dependency edge it was worked out with, for callers that need both.
pub(crate) fn tree_and_edges(conn: &Connection) -> Result<(Vec<TaskNode>, Edges)> {
    let sql = format!(
        "WITH RECURSIVE walk(id, depth, path) AS (
             SELECT id, 0, printf('%010d', position) FROM tasks WHERE parent_id IS NULL
             UNION ALL
             SELECT c.id, w.depth + 1, w.path || '.' || printf('%010d', c.position)
             FROM tasks c JOIN walk w ON c.parent_id = w.id
         )
         SELECT {TASK_COLUMNS}, w.depth, le.id, le.at, le.actor
         FROM walk w JOIN tasks t ON t.id = w.id
         LEFT JOIN task_events le
                ON le.id = (SELECT max(e.id) FROM task_events e WHERE e.task_id = t.id)
         WHERE NOT {}
         ORDER BY w.path",
        rollup::rejected("t")
    );
    let n = TASK_COLUMN_COUNT;
    // Every task has a `created` event (rule 21), so a null here is a damaged file and the
    // read fails on it.
    let rows = query_all(conn, &sql, [], |row| {
        let actor: String = row.get(n + 3)?;
        let at: String = row.get(n + 2)?;
        let node = TaskNode {
            task: from_row(row)?,
            depth: row.get(n)?,
            waiting: false,
            has_steps: false,
            last_event_at: at.clone(),
            last_actor: actor.parse().map_err(|_| bad_text(n + 3, &actor))?,
            subtree_last_event_at: at,
        };
        Ok((node, EventId(row.get(n + 1)?)))
    })?;
    let mut nodes = with_subtree_activity(rows);
    roll_up(&mut nodes);
    let edges = deps::edges(conn)?;
    mark_waiting(&mut nodes, &edges);
    Ok((nodes, edges))
}

/// Rule 10 over the whole tree: each parent shows what its steps add up to.
fn roll_up(nodes: &mut [TaskNode]) {
    let mut flat: Vec<rollup::Node> = nodes
        .iter()
        .map(|n| rollup::Node {
            id: n.task.id,
            parent: n.task.parent,
            position: n.task.position,
            status: n.task.status,
            blocked_reason: n.task.blocked_reason.clone(),
        })
        .collect();
    let parents = rollup::roll_up(&mut flat);
    for ((node, shown), has_steps) in nodes.iter_mut().zip(flat).zip(parents) {
        node.task.status = shown.status;
        node.task.blocked_reason = shown.blocked_reason;
        node.has_steps = has_steps;
    }
}

/// Rule 16: a task waits while any task it waits on, as it shows, is neither done nor
/// dropped. A blocker missing from the tree is a rejected proposal, so it's dropped.
fn mark_waiting(nodes: &mut [TaskNode], edges: &[(TaskId, TaskId)]) {
    let status: HashMap<TaskId, Status> =
        nodes.iter().map(|n| (n.task.id, n.task.status)).collect();
    let index: HashMap<TaskId, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.task.id, i))
        .collect();
    for (task, blocker) in edges {
        let open = status
            .get(blocker)
            .is_some_and(|s| !matches!(s, Status::Done | Status::Dropped));
        if open && let Some(&i) = index.get(task) {
            nodes[i].waiting = true;
        }
    }
}

/// Fills in each node's `subtree_last_event_at` from its own newest event and its
/// descendants'. `rows` are depth-first, so walking them backwards reaches every child
/// before its parent. Newest goes by event id.
fn with_subtree_activity(mut rows: Vec<(TaskNode, EventId)>) -> Vec<TaskNode> {
    let index: HashMap<TaskId, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, (node, _))| (node.task.id, i))
        .collect();
    for i in (0..rows.len()).rev() {
        let Some(&p) = rows[i].0.task.parent.and_then(|p| index.get(&p)) else {
            continue;
        };
        if rows[i].1 > rows[p].1 {
            rows[p].1 = rows[i].1;
            rows[p].0.subtree_last_event_at = rows[i].0.subtree_last_event_at.clone();
        }
    }
    rows.into_iter().map(|(node, _)| node).collect()
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

/// Rule 25: `Error::InvalidTarget` unless `git` and `t3` can only read `name` as a branch:
/// `[A-Za-z0-9._/-]`, at most 200 characters, no `..`, not starting with `-` or `/`, and not
/// ending with `/` or `.lock`.
pub(crate) fn check_branch_name(name: &str) -> Result<()> {
    let chars_ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
    let shape_ok = !name.contains("..")
        && !name.starts_with(['-', '/'])
        && !name.ends_with('/')
        && !name.ends_with(".lock");
    if chars_ok && shape_ok && name.len() <= 200 {
        Ok(())
    } else {
        Err(Error::InvalidTarget)
    }
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
    let branch = branch.map(str::trim).filter(|b| !b.is_empty());
    if let Some(name) = branch {
        check_branch_name(name)?;
    }
    let before = get(tx, id)?;
    if branch == before.branch.as_deref() {
        return Ok(before);
    }
    // Rule 10: a parent's work happens on its steps' branches. Clearing an old one is fine.
    if branch.is_some() && rollup::has_steps(tx, id)? {
        return Err(Error::HasSteps);
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
