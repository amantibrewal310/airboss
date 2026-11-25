//! Dependencies between tasks: adding and removing them, the loop check, waiting and run
//! order (rules 14–18).

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::events;
use crate::model::{Actor, Dependencies, EventKind, RunOrder, Status, Task};
use crate::permissions::{Call, allowed};
use crate::sql::{SUBTREE_CTE, query_all};
use crate::tasks::{self, TASK_COLUMNS, ensure_exists};
use crate::tx::Tx;
use crate::{Error, Result, TaskId, fault};

impl crate::Store {
    /// Makes `id` wait on `waits_on`. Adding a pair that already exists changes nothing.
    pub fn add_dependency(&mut self, id: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()> {
        self.write(|tx| add(tx, id, waits_on, actor))
    }

    /// Stops `id` waiting on `waits_on`; `Error::NotFound` if it wasn't.
    pub fn remove_dependency(&mut self, id: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()> {
        self.write(|tx| remove(tx, id, waits_on, actor))
    }

    /// The tasks `id` still waits on: its blockers that are neither done nor dropped.
    pub fn waits_on(&self, id: TaskId) -> Result<Vec<Task>> {
        open_blockers(&self.conn, id)
    }

    /// Every task `id` waits on and every task waiting on it, finished ones included, so
    /// the app can draw done blockers too (rule 27).
    pub fn dependencies(&self, id: TaskId) -> Result<Dependencies> {
        dependencies(&self.conn, id)
    }

    /// What can be picked up now: `todo` tasks with no open blockers, in tree order.
    pub fn ready(&self) -> Result<Vec<Task>> {
        Ok(tasks::tree(&self.conn)?
            .into_iter()
            .filter(|n| n.task.status == Status::Todo && !n.waiting)
            .map(|n| n.task)
            .collect())
    }

    /// The waves `parent`'s open subtasks can run in (rule 18). Subtasks that lifting
    /// deeper dependencies tangles into a loop go in `tangled`, after the waves.
    pub fn run_order(&self, parent: TaskId) -> Result<RunOrder> {
        run_order(&self.conn, parent)
    }
}

/// True when either task is an ancestor of the other (rule 15).
const RELATED_SQL: &str = "
WITH RECURSIVE up(id, start) AS (
    SELECT parent_id, id FROM tasks WHERE id IN (?1, ?2) AND parent_id IS NOT NULL
    UNION
    SELECT t.parent_id, up.start FROM tasks t JOIN up ON t.id = up.id
    WHERE t.parent_id IS NOT NULL
)
SELECT EXISTS (SELECT 1 FROM up WHERE (start = ?1 AND id = ?2) OR (start = ?2 AND id = ?1))";

/// True when `?1` can be reached from `?2` by following blockers, anywhere in the tree
/// (rule 14). `UNION` drops repeats, so diamonds are walked once.
const REACHES_SQL: &str = "
WITH RECURSIVE reach(id) AS (
    SELECT ?2
    UNION
    SELECT d.waits_on_id FROM task_deps d JOIN reach r ON d.task_id = r.id
)
SELECT EXISTS (SELECT 1 FROM reach WHERE id = ?1)";

fn add(tx: &mut Tx<'_>, task: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()> {
    allowed(Call::ChangeDependency, actor)?;
    ensure_exists(tx, task)?;
    ensure_exists(tx, waits_on)?;
    if !check_new(tx, task, waits_on)? {
        return Ok(());
    }
    insert(tx, task, waits_on, actor)?;
    let body = format!("waits on #{}", waits_on.0);
    events::record(tx, task, EventKind::WaitsOnAdded, None, Some(&body), actor)?;
    fault::hit(fault::DEP_ADDED)
}

/// Rules 14 and 15 for one new pair of existing tasks: `Ok(false)` if `task` already waits on
/// `waits_on`, `Ok(true)` if the pair may be added, and `Loop` or `InvalidTarget` if not.
pub(crate) fn check_new(conn: &Connection, task: TaskId, waits_on: TaskId) -> Result<bool> {
    if task == waits_on {
        return Err(Error::Loop);
    }
    if exists(conn, task, waits_on)? {
        return Ok(false);
    }
    let related: bool = conn.query_row(RELATED_SQL, params![task.0, waits_on.0], |r| r.get(0))?;
    if related {
        return Err(Error::InvalidTarget);
    }
    let loops: bool = conn.query_row(REACHES_SQL, params![task.0, waits_on.0], |r| r.get(0))?;
    if loops {
        return Err(Error::Loop);
    }
    Ok(true)
}

/// Stores one pair that [`check_new`] passed. Writes no event: the caller does.
pub(crate) fn insert(
    conn: &Connection,
    task: TaskId,
    waits_on: TaskId,
    actor: &Actor,
) -> Result<()> {
    conn.execute(
        "INSERT INTO task_deps (task_id, waits_on_id, added_by) VALUES (?1, ?2, ?3)",
        params![task.0, waits_on.0, actor.to_string()],
    )?;
    Ok(())
}

fn remove(tx: &mut Tx<'_>, task: TaskId, waits_on: TaskId, actor: &Actor) -> Result<()> {
    allowed(Call::ChangeDependency, actor)?;
    let removed = tx.execute(
        "DELETE FROM task_deps WHERE task_id = ?1 AND waits_on_id = ?2",
        params![task.0, waits_on.0],
    )?;
    if removed == 0 {
        return Err(Error::NotFound);
    }
    let body = format!("no longer waits on #{}", waits_on.0);
    events::record(
        tx,
        task,
        EventKind::WaitsOnRemoved,
        None,
        Some(&body),
        actor,
    )?;
    fault::hit(fault::DEP_REMOVED)
}

fn exists(conn: &Connection, task: TaskId, waits_on: TaskId) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM task_deps WHERE task_id = ?1 AND waits_on_id = ?2)",
        params![task.0, waits_on.0],
        |r| r.get(0),
    )?)
}

fn open_blockers(conn: &Connection, id: TaskId) -> Result<Vec<Task>> {
    ensure_exists(conn, id)?;
    query_all(
        conn,
        &format!(
            "SELECT {TASK_COLUMNS} FROM task_deps d JOIN tasks t ON t.id = d.waits_on_id
             WHERE d.task_id = ?1 AND t.status NOT IN ('done', 'dropped')
             ORDER BY t.id"
        ),
        params![id.0],
        tasks::from_row,
    )
}

fn dependencies(conn: &Connection, id: TaskId) -> Result<Dependencies> {
    ensure_exists(conn, id)?;
    // `from` is the column naming `id`, `to` the one naming the other task.
    let side = |from: &str, to: &str| {
        query_all(
            conn,
            &format!(
                "SELECT {TASK_COLUMNS} FROM task_deps d JOIN tasks t ON t.id = d.{to}
                 WHERE d.{from} = ?1 ORDER BY t.id"
            ),
            params![id.0],
            tasks::from_row,
        )
    };
    Ok(Dependencies {
        waits_on: side("task_id", "waits_on_id")?,
        blocks: side("waits_on_id", "task_id")?,
    })
}

/// Rule 17, run by `delete_task` before it deletes `id`: every task outside the doomed
/// subtree that waits on something in it loses those dependencies and gets one
/// `waits_on_removed` event. Dependencies inside the subtree go with it, unrecorded.
pub(crate) fn on_task_deleting(tx: &mut Tx<'_>, id: TaskId, actor: &Actor) -> Result<()> {
    // Each waiter outside the subtree, with the blockers it loses, e.g. `"#12, #14"`.
    let waiters = query_all(
        tx,
        &format!(
            "{SUBTREE_CTE}
             SELECT d.task_id, group_concat('#' || d.waits_on_id, ', ')
             FROM task_deps d
             WHERE d.waits_on_id IN (SELECT id FROM sub) AND d.task_id NOT IN (SELECT id FROM sub)
             GROUP BY d.task_id
             ORDER BY d.task_id"
        ),
        params![id.0],
        |r| Ok((TaskId(r.get(0)?), r.get::<_, String>(1)?)),
    )?;
    tx.execute(
        &format!(
            "{SUBTREE_CTE}
             DELETE FROM task_deps
             WHERE waits_on_id IN (SELECT id FROM sub) AND task_id NOT IN (SELECT id FROM sub)"
        ),
        params![id.0],
    )?;
    for (waiter, blockers) in waiters {
        let body = format!("no longer waits on {blockers}: deleted");
        events::record(
            tx,
            waiter,
            EventKind::WaitsOnRemoved,
            None,
            Some(&body),
            actor,
        )?;
    }
    fault::hit(fault::DEPS_CLEARED)
}

/// Every dependency between two open tasks that starts in `?1`'s subtree, lifted to the
/// direct subtasks the two ends sit under. A null blocker side means the blocker is outside.
const LIFTED_SQL: &str = "
WITH RECURSIVE sub(id, top) AS (
    SELECT id, id FROM tasks WHERE parent_id = ?1
    UNION ALL
    SELECT t.id, sub.top FROM tasks t JOIN sub ON t.parent_id = sub.id
)
SELECT DISTINCT s.top, b.top
FROM sub s
JOIN tasks st ON st.id = s.id
JOIN task_deps d ON d.task_id = s.id
JOIN tasks bt ON bt.id = d.waits_on_id
LEFT JOIN sub b ON b.id = d.waits_on_id
WHERE st.status NOT IN ('proposed', 'done', 'dropped')
  AND bt.status NOT IN ('proposed', 'done', 'dropped')";

fn run_order(conn: &Connection, parent: TaskId) -> Result<RunOrder> {
    ensure_exists(conn, parent)?;
    let children = query_all(
        conn,
        &format!(
            "SELECT {TASK_COLUMNS} FROM tasks t
             WHERE t.parent_id = ?1 AND t.status NOT IN ('proposed', 'done', 'dropped')
             ORDER BY t.position"
        ),
        params![parent.0],
        tasks::from_row,
    )?;
    let index: HashMap<TaskId, usize> = children
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id, i))
        .collect();
    let n = children.len();

    // `unblocks[y]` lists the subtasks waiting on y.
    let mut unblocks: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut waits_for = vec![0usize; n];
    let mut outside = vec![false; n];
    let mut stmt = conn.prepare(LIFTED_SQL)?;
    let mut rows = stmt.query(params![parent.0])?;
    while let Some(row) = rows.next()? {
        // A subtask left out (done or dropped) neither waits nor holds anything up.
        let Some(&x) = index.get(&TaskId(row.get(0)?)) else {
            continue;
        };
        match row.get::<_, Option<i64>>(1)? {
            None => outside[x] = true,
            Some(y) => {
                if let Some(&y) = index.get(&TaskId(y))
                    && y != x
                {
                    unblocks[y].push(x);
                    waits_for[x] += 1;
                }
            }
        }
    }

    // Kahn's algorithm, one wave at a time; sorting indexes keeps sibling order.
    let mut waves: Vec<Vec<usize>> = Vec::new();
    let mut wave: Vec<usize> = (0..n).filter(|&i| waits_for[i] == 0).collect();
    while !wave.is_empty() {
        let mut next = Vec::new();
        for &y in &wave {
            for &x in &unblocks[y] {
                waits_for[x] -= 1;
                if waits_for[x] == 0 {
                    next.push(x);
                }
            }
        }
        next.sort_unstable();
        waves.push(wave);
        wave = next;
    }
    // Whatever Kahn's algorithm couldn't place waits, through lifting, on a loop.
    let mut tangled = vec![true; n];
    for &i in waves.iter().flatten() {
        tangled[i] = false;
    }

    // Waiting on a subtask that waits outside is waiting outside too. Waves are in
    // dependency order, so one pass reaches everything downstream.
    for &y in waves.iter().flatten() {
        if outside[y] {
            for &x in &unblocks[y] {
                outside[x] = true;
            }
        }
    }
    let pick = |ids: &[usize], want_outside: bool| -> Vec<Task> {
        ids.iter()
            .filter(|&&i| !tangled[i] && outside[i] == want_outside)
            .map(|&i| children[i].clone())
            .collect()
    };
    Ok(RunOrder {
        waves: waves
            .iter()
            .map(|w| pick(w, false))
            .filter(|w| !w.is_empty())
            .collect(),
        waiting_outside: pick(&(0..n).collect::<Vec<_>>(), true),
        tangled: (0..n)
            .filter(|&i| tangled[i])
            .map(|i| children[i].clone())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::{NewTask, Store};

    fn top(s: &mut Store, title: &str) -> TaskId {
        s.add_task(NewTask::new(title), &Actor::Me).unwrap().id
    }

    fn under(s: &mut Store, parent: TaskId, title: &str) -> TaskId {
        s.add_task(NewTask::new(title).under(parent), &Actor::Me)
            .unwrap()
            .id
    }

    fn count(s: &Store, table: &str) -> i64 {
        s.conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn kinds(s: &Store, id: TaskId) -> Vec<EventKind> {
        s.history(id).unwrap().into_iter().map(|e| e.kind).collect()
    }

    #[test]
    fn a_failure_while_adding_leaves_no_dependency_and_no_event() {
        let mut s = Store::open_in_memory().unwrap();
        let a = top(&mut s, "A");
        let b = top(&mut s, "B");
        let events = count(&s, "task_events");
        crate::fault::arm(crate::fault::DEP_ADDED);
        assert!(s.add_dependency(b, a, &Actor::Me).is_err());
        assert_eq!(
            (count(&s, "task_deps"), count(&s, "task_events")),
            (0, events)
        );
        s.add_dependency(b, a, &Actor::Me).unwrap();
        assert_eq!(count(&s, "task_deps"), 1);
    }

    #[test]
    fn a_failure_while_removing_keeps_the_dependency_and_writes_no_event() {
        let mut s = Store::open_in_memory().unwrap();
        let a = top(&mut s, "A");
        let b = top(&mut s, "B");
        s.add_dependency(b, a, &Actor::Me).unwrap();
        let events = count(&s, "task_events");
        crate::fault::arm(crate::fault::DEP_REMOVED);
        assert!(s.remove_dependency(b, a, &Actor::Me).is_err());
        assert_eq!(
            (count(&s, "task_deps"), count(&s, "task_events")),
            (1, events)
        );
    }

    /// X has X1 and X2, and X2 has X2a. W1 waits on X, W2 on X2a, W3 on X1 and X2a, and
    /// X1 on X2a (inside the subtree). U waits on V, unrelated.
    struct Fixture {
        x: TaskId,
        x1: TaskId,
        w: [TaskId; 3],
        u: TaskId,
    }

    fn deleting_fixture(s: &mut Store) -> Fixture {
        let x = top(s, "X");
        let x1 = under(s, x, "X1");
        let x2 = under(s, x, "X2");
        let x2a = under(s, x2, "X2a");
        let w1 = top(s, "W1");
        let w2 = top(s, "W2");
        let w3 = top(s, "W3");
        let u = top(s, "U");
        let v = top(s, "V");
        for (task, on) in [(w1, x), (w2, x2a), (w3, x1), (w3, x2a), (x1, x2a), (u, v)] {
            s.add_dependency(task, on, &Actor::Me).unwrap();
        }
        Fixture {
            x,
            x1,
            w: [w1, w2, w3],
            u,
        }
    }

    #[test]
    fn deleting_a_blocker_tells_each_outside_waiter_once() {
        let mut s = Store::open_in_memory().unwrap();
        let f = deleting_fixture(&mut s);
        let x1_before = kinds(&s, f.x1);
        let u_before = kinds(&s, f.u);

        s.write(|tx| on_task_deleting(tx, f.x, &Actor::Me)).unwrap();

        for w in f.w {
            let history = s.history(w).unwrap();
            let removed: Vec<_> = history
                .iter()
                .filter(|e| e.kind == EventKind::WaitsOnRemoved)
                .collect();
            assert_eq!(removed.len(), 1, "one event for {w:?}");
            assert_eq!(removed[0].actor, Actor::Me);
            assert!(s.waits_on(w).unwrap().is_empty());
        }
        assert_eq!(kinds(&s, f.x1), x1_before, "inside the subtree: no event");
        assert_eq!(kinds(&s, f.u), u_before, "unrelated: no event");
        assert_eq!(s.waits_on(f.u).unwrap().len(), 1);
        assert_eq!(s.waits_on(f.x1).unwrap().len(), 1, "goes with the subtree");
        assert_eq!(s.last_committed.len(), 3);
    }

    #[test]
    fn a_failure_while_clearing_a_deleted_blocker_leaves_everything() {
        let mut s = Store::open_in_memory().unwrap();
        let f = deleting_fixture(&mut s);
        let (deps, events) = (count(&s, "task_deps"), count(&s, "task_events"));
        crate::fault::arm(crate::fault::DEPS_CLEARED);
        assert!(s.write(|tx| on_task_deleting(tx, f.x, &Actor::Me)).is_err());
        assert_eq!(
            (count(&s, "task_deps"), count(&s, "task_events")),
            (deps, events)
        );
    }
}
