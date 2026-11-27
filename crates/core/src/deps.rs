//! Dependencies between tasks: adding and removing them, the loop check, waiting and run
//! order (rules 14–18).

use std::collections::{BTreeSet, HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};

use crate::events;
use crate::model::{
    Actor, Dependencies, EventKind, OutsideWait, ReadyTask, RunOrder, Status, Task, TaskNode,
};
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

    /// The tasks `id` still waits on: its blockers that are open, neither proposed, done nor
    /// dropped (rule 16).
    pub fn waits_on(&self, id: TaskId) -> Result<Vec<Task>> {
        open_blockers(&self.conn, id)
    }

    /// Every task `id` waits on and every task waiting on it, finished ones included, so
    /// the app can draw done blockers too (rule 27).
    pub fn dependencies(&self, id: TaskId) -> Result<Dependencies> {
        dependencies(&self.conn, id)
    }

    /// What can be picked up now: `todo` tasks with no open blockers and no steps, in tree
    /// order, each with the time it became ready (rule 26).
    pub fn ready(&self) -> Result<Vec<ReadyTask>> {
        ready(&self.conn)
    }

    /// Every dependency in the store as `(waiting, waits_on)`, finished tasks included, so
    /// a map can draw every line from one read.
    pub fn dependency_edges(&self) -> Result<Vec<(TaskId, TaskId)>> {
        edges(&self.conn)
    }

    /// The waves `parent`'s open subtasks can run in (rule 18). Subtasks that lifting
    /// deeper dependencies tangles into a loop go in `tangled`, after the waves.
    pub fn run_order(&self, parent: TaskId) -> Result<RunOrder> {
        run_order(&self.conn, parent)
    }
}

/// For every `todo` task, the time of the newest event that could have made it ready: its
/// own `created` or `waits_on_removed` event, or a current blocker finishing. Newest goes
/// by event id (rule 26). A blocker with steps finishes through them (rule 10), which
/// [`READY_SINCE_ONE_SQL`] counts for the few tasks waiting on one.
const READY_SINCE_SQL: &str = "
SELECT t.id, ifnull(
    (SELECT e.at FROM task_events e WHERE e.id = max(
        ifnull((SELECT max(o.id) FROM task_events o
                WHERE o.task_id = t.id AND o.kind IN ('created', 'waits_on_removed')), 0),
        ifnull((SELECT max(f.id) FROM task_deps d JOIN task_events f ON f.task_id = d.waits_on_id
                WHERE d.task_id = t.id
                  AND (f.kind = 'rejected'
                       OR (f.kind = 'status' AND f.to_status IN ('done', 'dropped')))), 0))),
    t.created_at)
FROM tasks t WHERE t.status = 'todo'";

/// [`READY_SINCE_SQL`] for task `?1` alone, with each blocker's steps finishing counted as
/// the blocker finishing: a parent finishes when its last step does (rule 10).
const READY_SINCE_ONE_SQL: &str = "
WITH RECURSIVE under(top, id) AS (
    SELECT waits_on_id, waits_on_id FROM task_deps WHERE task_id = ?1
    UNION
    SELECT u.top, c.id FROM tasks c JOIN under u ON c.parent_id = u.id
)
SELECT e.at FROM task_events e WHERE e.id = max(
    ifnull((SELECT max(o.id) FROM task_events o
            WHERE o.task_id = ?1 AND o.kind IN ('created', 'waits_on_removed')), 0),
    ifnull((SELECT max(f.id) FROM under u JOIN task_events f ON f.task_id = u.id
            WHERE (f.kind = 'rejected' AND u.id = u.top)
               OR (f.kind = 'status' AND f.to_status IN ('done', 'dropped'))), 0))";

fn ready(conn: &Connection) -> Result<Vec<ReadyTask>> {
    let (tree, edges) = tasks::tree_and_edges(conn)?;
    ready_from(conn, &tree, &edges)
}

/// The ready list worked out from a tree already built on `conn`, so a caller that has the
/// tree never builds it twice (rule 30). Read it in the same transaction as the tree.
pub(crate) fn ready_from(
    conn: &Connection,
    tree: &[TaskNode],
    edges: &[(TaskId, TaskId)],
) -> Result<Vec<ReadyTask>> {
    let mut since: HashMap<TaskId, String> = query_all(conn, READY_SINCE_SQL, [], |r| {
        Ok((TaskId(r.get(0)?), r.get(1)?))
    })?
    .into_iter()
    .collect();
    // A parent isn't started itself; its steps are (rule 10).
    let ready: Vec<&TaskNode> = tree
        .iter()
        .filter(|n| n.task.status == Status::Todo && !n.waiting && !n.has_steps)
        .collect();
    for task in waiting_on_a_parent(tree, edges, &ready) {
        let mut stmt = conn.prepare_cached(READY_SINCE_ONE_SQL)?;
        if let Some(at) = stmt.query_row(params![task.0], |r| r.get(0)).optional()? {
            since.insert(task, at);
        }
    }
    Ok(ready
        .into_iter()
        .map(|n| ReadyTask {
            since: since
                .remove(&n.task.id)
                .unwrap_or_else(|| n.task.created_at.clone()),
            task: n.task.clone(),
        })
        .collect())
}

/// The tasks in `ready` that wait on a task with steps.
fn waiting_on_a_parent(
    tree: &[TaskNode],
    edges: &[(TaskId, TaskId)],
    ready: &[&TaskNode],
) -> BTreeSet<TaskId> {
    let parents: HashSet<TaskId> = tree
        .iter()
        .filter(|n| n.has_steps)
        .map(|n| n.task.id)
        .collect();
    if parents.is_empty() {
        return BTreeSet::new();
    }
    let ready: HashSet<TaskId> = ready.iter().map(|n| n.task.id).collect();
    edges
        .iter()
        .filter(|(task, blocker)| ready.contains(task) && parents.contains(blocker))
        .map(|&(task, _)| task)
        .collect()
}

/// Every `(waiting, waits_on)` pair, finished tasks included, sorted (rule 30).
pub(crate) fn edges(conn: &Connection) -> Result<Vec<(TaskId, TaskId)>> {
    query_all(
        conn,
        "SELECT task_id, waits_on_id FROM task_deps ORDER BY task_id, waits_on_id",
        [],
        |r| Ok((TaskId(r.get(0)?), TaskId(r.get(1)?))),
    )
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
    Ok(dependencies(conn, id)?
        .waits_on
        .into_iter()
        .filter(|t| holds(t.status))
        .collect())
}

/// True when a blocker in `status` holds up the tasks waiting on it (rule 16): it's open, and
/// not a proposal I haven't accepted yet.
pub(crate) fn holds(status: Status) -> bool {
    !matches!(status, Status::Proposed | Status::Done | Status::Dropped)
}

/// Both sides, each task as it shows (rule 10).
fn dependencies(conn: &Connection, id: TaskId) -> Result<Dependencies> {
    ensure_exists(conn, id)?;
    // `from` is the column naming `id`, `to` the one naming the other task.
    let side = |from: &str, to: &str| {
        let found = query_all(
            conn,
            &format!(
                "SELECT {TASK_COLUMNS} FROM task_deps d JOIN tasks t ON t.id = d.{to}
                 WHERE d.{from} = ?1 ORDER BY t.id"
            ),
            params![id.0],
            tasks::from_row,
        )?;
        tasks::all_shown(conn, found)
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

/// Every dependency that starts in `?1`'s subtree: the waiting task, the direct subtask it
/// sits under, the direct subtask the blocker sits under (null when the blocker is outside),
/// and the blocker. Which ends are open is decided by what they show (rule 10).
const LIFTED_SQL: &str = "
WITH RECURSIVE sub(id, top) AS (
    SELECT id, id FROM tasks WHERE parent_id = ?1
    UNION ALL
    SELECT t.id, sub.top FROM tasks t JOIN sub ON t.parent_id = sub.id
)
SELECT s.id, s.top, b.top, d.waits_on_id
FROM sub s
JOIN task_deps d ON d.task_id = s.id
LEFT JOIN sub b ON b.id = d.waits_on_id";

fn run_order(conn: &Connection, parent: TaskId) -> Result<RunOrder> {
    ensure_exists(conn, parent)?;
    // What everything under the parent shows; anything missing is a rejected proposal.
    let shown = crate::rollup::shown_subtree(conn, parent)?;
    let open_inside = |id: &TaskId| shown.get(id).is_some_and(|s| holds(s.status));
    let children: Vec<Task> = query_all(
        conn,
        &format!("SELECT {TASK_COLUMNS} FROM tasks t WHERE t.parent_id = ?1 ORDER BY t.position"),
        params![parent.0],
        tasks::from_row,
    )?
    .into_iter()
    .filter_map(|mut t| {
        let s = shown.get(&t.id)?;
        t.status = s.status;
        t.blocked_reason = s.blocked_reason.clone();
        holds(t.status).then_some(t)
    })
    .collect();
    let index: HashMap<TaskId, usize> = children
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id, i))
        .collect();
    let n = children.len();

    // `unblocks[y]` lists the subtasks waiting on y.
    let mut unblocks: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut waits_for = vec![0usize; n];
    // The open tasks outside the parent holding each subtask; empty while nothing does.
    let mut outside: Vec<BTreeSet<TaskId>> = vec![BTreeSet::new(); n];
    // Outside blockers, as they show, read once each.
    let mut outside_open: HashMap<TaskId, bool> = HashMap::new();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let lifted = query_all(conn, LIFTED_SQL, params![parent.0], |r| {
        Ok((
            TaskId(r.get(0)?),
            TaskId(r.get(1)?),
            r.get::<_, Option<i64>>(2)?.map(TaskId),
            TaskId(r.get(3)?),
        ))
    })?;
    for (step, top, blocker_top, blocker) in lifted {
        // A subtask left out (done or dropped) neither waits nor holds anything up.
        let Some(&x) = index.get(&top) else {
            continue;
        };
        if !open_inside(&step) {
            continue;
        }
        match blocker_top {
            None => {
                let open = match outside_open.get(&blocker) {
                    Some(&open) => open,
                    None => {
                        let open = holds(tasks::get(conn, blocker)?.status);
                        outside_open.insert(blocker, open);
                        open
                    }
                };
                if open {
                    outside[x].insert(blocker);
                }
            }
            Some(y) => {
                if open_inside(&blocker)
                    && let Some(&y) = index.get(&y)
                    && y != x
                    && seen.insert((x, y))
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

    // Waiting on a subtask that waits outside is waiting outside too, on the same blockers.
    // Waves are in dependency order, so one pass reaches everything downstream.
    for &y in waves.iter().flatten() {
        if !outside[y].is_empty() {
            let held = outside[y].clone();
            for &x in &unblocks[y] {
                outside[x].extend(&held);
            }
        }
    }
    let placed = |i: &usize| !tangled[*i];
    Ok(RunOrder {
        waves: waves
            .iter()
            .map(|w| {
                w.iter()
                    .filter(|&i| placed(i) && outside[*i].is_empty())
                    .map(|&i| children[i].clone())
                    .collect::<Vec<_>>()
            })
            .filter(|w| !w.is_empty())
            .collect(),
        waiting_outside: (0..n)
            .filter(|i| placed(i) && !outside[*i].is_empty())
            .map(|i| OutsideWait {
                task: children[i].clone(),
                blockers: outside[i].iter().copied().collect(),
            })
            .collect(),
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
