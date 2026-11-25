//! Deleting a task and its subtree: rules 5, 13 and 17.

use rusqlite::params;

use crate::model::Actor;
use crate::permissions::{Call, allowed};
use crate::tx::Tx;
use crate::{Result, TaskId, deps, fault, positions, tasks};

fn delete(tx: &mut Tx<'_>, id: TaskId, actor: &Actor) -> Result<()> {
    allowed(Call::DeleteTask, actor)?;
    let parent = tasks::get(tx, id)?.parent;
    // Rule 17 first, while the subtree's dependencies are still there to report.
    deps::on_task_deleting(tx, id, actor)?;
    // Rule 5: the cascade takes the subtree with its events, links and dependencies.
    tx.execute("DELETE FROM tasks WHERE id = ?1", params![id.0])?;
    positions::renumber(tx, parent)?;
    fault::hit(fault::DELETE_RENUMBERED)
}

impl crate::Store {
    /// Deletes a task and its whole subtree, with their events, links and dependencies.
    /// `me` only. Tasks that waited on anything deleted get a `waits_on_removed` event,
    /// and the old siblings are renumbered 1..n.
    pub fn delete_task(&mut self, id: TaskId, actor: &Actor) -> Result<()> {
        self.write(|tx| delete(tx, id, actor))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::{NewLink, NewTask, Status, Store};

    type Row = (i64, Option<i64>, i64, String, Option<String>);

    /// Every row that matters: tasks (id, parent, position, status, reason) plus the
    /// sizes of the event, link and dependency tables.
    fn snapshot(s: &Store) -> (Vec<Row>, [i64; 3]) {
        let mut stmt = s
            .conn
            .prepare(
                "SELECT id, parent_id, position, status, blocked_reason FROM tasks ORDER BY id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let n = |t: &str| {
            s.conn
                .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        };
        (rows, [n("task_events"), n("links"), n("task_deps")])
    }

    fn add(s: &mut Store, title: &str, parent: Option<TaskId>) -> TaskId {
        let mut new = NewTask::new(title);
        new.parent = parent;
        s.add_task(new, &Actor::Me).unwrap().id
    }

    /// P has A, X and B; X has X1, which has X2. X2 has a link and waits on A; W waits on X1.
    fn fixture(s: &mut Store) -> (TaskId, TaskId) {
        let p = add(s, "P", None);
        let a = add(s, "A", Some(p));
        let x = add(s, "X", Some(p));
        add(s, "B", Some(p));
        let x1 = add(s, "X1", Some(x));
        let x2 = add(s, "X2", Some(x1));
        let w = add(s, "W", None);
        s.link(x2, NewLink::pr("acme/web#1"), &Actor::Me).unwrap();
        s.add_dependency(x2, a, &Actor::Me).unwrap();
        s.add_dependency(w, x1, &Actor::Me).unwrap();
        s.set_status(x1, Status::Blocked, Some("why"), &Actor::Me)
            .unwrap();
        (x, w)
    }

    #[test]
    fn deleting_cascades_to_every_row_of_the_subtree() {
        let mut s = Store::open_in_memory().unwrap();
        let (x, w) = fixture(&mut s);
        let (tasks_before, [events, links, deps]) = snapshot(&s);

        s.delete_task(x, &Actor::Me).unwrap();

        let (tasks_after, counts) = snapshot(&s);
        assert_eq!(tasks_after.len(), tasks_before.len() - 3);
        // X, X1, X2 created; X2 linked and waiting on A; X1 blocked.
        let subtree_events: i64 = 3 + 3;
        assert_eq!(
            counts,
            [events - subtree_events + 1, links - 1, deps - 2],
            "only W's waits_on_removed event is new"
        );
        assert_eq!(s.last_committed.len(), 1);
        assert!(s.waits_on(w).unwrap().is_empty());
    }

    #[test]
    fn a_failed_delete_leaves_everything_as_it_was() {
        let mut s = Store::open_in_memory().unwrap();
        let (x, _) = fixture(&mut s);
        let before = snapshot(&s);
        let committed = s.last_committed.clone();

        fault::arm(fault::DELETE_RENUMBERED);
        assert!(s.delete_task(x, &Actor::Me).is_err());
        assert_eq!(snapshot(&s), before);
        assert_eq!(s.last_committed, committed);

        s.delete_task(x, &Actor::Me).unwrap();
    }

    #[test]
    fn a_failed_subtree_drop_changes_nothing() {
        let mut s = Store::open_in_memory().unwrap();
        let (x, _) = fixture(&mut s);
        let before = snapshot(&s);
        let committed = s.last_committed.clone();

        fault::arm(fault::SUBTREE_DROPPED);
        assert!(s.set_status(x, Status::Dropped, None, &Actor::Me).is_err());
        assert_eq!(snapshot(&s), before);
        assert_eq!(s.last_committed, committed);

        s.set_status(x, Status::Dropped, None, &Actor::Me).unwrap();
        assert_eq!(s.last_committed.len(), 3, "X, X1 and X2");
    }
}
