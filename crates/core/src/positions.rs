//! Sibling order and moving tasks around the tree: rules 11, 12 and 13.
//!
//! Positions are 1-based and gap-free within each sibling group (the tasks sharing a
//! `parent_id`, where NULL is the top level). There is deliberately no UNIQUE index on
//! `(parent_id, position)`: see the spec's schema notes.

use rusqlite::{Connection, OptionalExtension, params};

use crate::events;
use crate::model::{Actor, EventKind, Status};
use crate::permissions::{Call, allowed};
use crate::tx::Tx;
use crate::{Error, Result, TaskId, fault};

/// Renumbers `parent`'s children 1..n, keeping their current order (ties broken by id).
/// Writes no events (rule 13).
pub(crate) fn renumber(tx: &Tx<'_>, parent: Option<TaskId>) -> Result<()> {
    tx.execute(
        "UPDATE tasks SET position = r.n
         FROM (SELECT id, ROW_NUMBER() OVER (ORDER BY position, id) AS n
               FROM tasks WHERE parent_id IS ?1) AS r
         WHERE tasks.id = r.id AND tasks.position <> r.n",
        params![parent.map(|p| p.0)],
    )?;
    Ok(())
}

/// Rule 12: `parent` must exist and be able to take children, i.e. not be `proposed`,
/// `done` or `dropped`.
pub(crate) fn ensure_open_parent(conn: &Connection, parent: TaskId) -> Result<()> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![parent.0],
            |r| r.get(0),
        )
        .optional()?;
    let status: Status = status
        .ok_or(Error::NotFound)?
        .parse()
        .map_err(|_| Error::InvalidTarget)?;
    match status {
        Status::Proposed | Status::Done | Status::Dropped => Err(Error::InvalidTarget),
        _ => Ok(()),
    }
}

/// Rule 11: `Error::Loop` if `target` is `id` or anywhere in `id`'s subtree.
fn ensure_not_in_subtree(conn: &Connection, id: TaskId, target: TaskId) -> Result<()> {
    let inside: bool = conn.query_row(
        "WITH RECURSIVE sub(id) AS (
             SELECT ?1
             UNION
             SELECT t.id FROM tasks t JOIN sub ON t.parent_id = sub.id
         )
         SELECT EXISTS (SELECT 1 FROM sub WHERE id = ?2)",
        params![id.0, target.0],
        |r| r.get(0),
    )?;
    if inside { Err(Error::Loop) } else { Ok(()) }
}

fn move_in(
    tx: &mut Tx<'_>,
    id: TaskId,
    new_parent: Option<TaskId>,
    position: u32,
    actor: &Actor,
) -> Result<()> {
    allowed(Call::MoveTask, actor)?;
    let old_parent = crate::tasks::get(tx, id)?.parent;
    if let Some(target) = new_parent {
        crate::tasks::ensure_exists(tx, target)?;
        ensure_not_in_subtree(tx, id, target)?;
        ensure_open_parent(tx, target)?;
    }

    // Park the task at the end of its new group, then close the gaps in both groups.
    tx.execute(
        "UPDATE tasks SET parent_id = ?1, position = ?2 WHERE id = ?3",
        params![new_parent.map(|p| p.0), i64::MAX, id.0],
    )?;
    if old_parent != new_parent {
        renumber(tx, old_parent)?;
    }
    renumber(tx, new_parent)?;
    fault::hit(fault::MOVE_RENUMBERED)?;

    // The task is now last, at n + 1. Clamp the request into 1..=n+1 and open a slot there.
    let last: u32 = tx.query_row(
        "SELECT position FROM tasks WHERE id = ?1",
        params![id.0],
        |r| r.get(0),
    )?;
    let position = position.clamp(1, last);
    tx.execute(
        "UPDATE tasks SET position = position + 1
         WHERE parent_id IS ?1 AND id <> ?2 AND position >= ?3",
        params![new_parent.map(|p| p.0), id.0, position],
    )?;
    tx.execute(
        "UPDATE tasks SET position = ?1 WHERE id = ?2",
        params![position, id.0],
    )?;

    events::record(tx, id, EventKind::Moved, None, None, actor)?;
    Ok(())
}

impl crate::Store {
    /// Moves a task (and its subtree) under `new_parent` (None: top level) at `position`,
    /// clamped into range. `me` only. Renumbers the old and new sibling groups and writes
    /// one `moved` event, on the moved task only.
    pub fn move_task(
        &mut self,
        id: TaskId,
        new_parent: Option<TaskId>,
        position: u32,
        actor: &Actor,
    ) -> Result<()> {
        self.write(|tx| move_in(tx, id, new_parent, position, actor))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::{NewTask, Store};

    fn add(store: &mut Store, title: &str, parent: Option<TaskId>) -> TaskId {
        let mut new = NewTask::new(title);
        new.parent = parent;
        store.add_task(new, &Actor::Me).unwrap().id
    }

    fn snapshot(store: &Store) -> (Vec<(i64, Option<i64>, i64)>, i64) {
        let mut stmt = store
            .conn
            .prepare("SELECT id, parent_id, position FROM tasks ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let events = store
            .conn
            .query_row("SELECT count(*) FROM task_events", [], |r| r.get(0))
            .unwrap();
        (rows, events)
    }

    #[test]
    fn a_failure_inside_a_move_leaves_positions_and_events_unchanged() {
        let mut s = Store::open_in_memory().unwrap();
        let a = add(&mut s, "A", None);
        let b = add(&mut s, "B", None);
        for i in 0..3 {
            add(&mut s, &format!("a{i}"), Some(a));
            add(&mut s, &format!("b{i}"), Some(b));
        }
        let mover = add(&mut s, "a-last", Some(a));
        // Move something out of the middle, so a half-done renumber would be visible.
        s.move_task(mover, Some(a), 1, &Actor::Me).unwrap();
        let before = snapshot(&s);
        let committed = s.last_committed.clone();

        fault::arm(fault::MOVE_RENUMBERED);
        assert!(s.move_task(mover, Some(b), 2, &Actor::Me).is_err());
        assert_eq!(snapshot(&s), before);
        assert_eq!(s.last_committed, committed);

        // Still usable afterwards.
        s.move_task(mover, Some(b), 2, &Actor::Me).unwrap();
        assert_eq!(s.task(mover).unwrap().parent, Some(b));
    }

    #[test]
    fn renumber_closes_gaps_and_breaks_ties_by_id() {
        let mut s = Store::open_in_memory().unwrap();
        let ids: Vec<TaskId> = (0..4)
            .map(|i| add(&mut s, &format!("t{i}"), None))
            .collect();
        // Positions 7, 3, 3, 10 -> order t1, t2 (tie by id), t0, t3.
        for (id, pos) in ids.iter().zip([7, 3, 3, 10]) {
            s.conn
                .execute(
                    "UPDATE tasks SET position = ?1 WHERE id = ?2",
                    params![pos, id.0],
                )
                .unwrap();
        }
        let tx = Tx::begin(&mut s.conn).unwrap();
        renumber(&tx, None).unwrap();
        tx.commit().unwrap();

        let got: Vec<u32> = ids.iter().map(|&id| s.task(id).unwrap().position).collect();
        assert_eq!(got, vec![3, 1, 2, 4]);
    }
}
