//! Plans arriving as trees: `add_tasks` (rules 19, 20 and 21).
//!
//! One call is one transaction. Everything that can be checked from the plan alone is
//! checked before the first write; the parent's status, and the loop and ancestor checks
//! that need the new rows, run inside the same transaction, so a failure there still
//! leaves nothing behind.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};

use crate::deps;
use crate::events;
use crate::model::{Actor, DepRef, EventKind, Origin, PlanResult, PlanTask, Status};
use crate::permissions::{Call, allowed};
use crate::positions::ensure_open_parent;
use crate::tasks::{clean_title, ensure_exists};
use crate::tx::Tx;
use crate::{Error, Result, TaskId, fault};

impl crate::Store {
    /// Adds a whole plan under `parent` (None: top level), all or nothing (rule 19).
    /// Steps whose key already exists under the same parent are left as they are, so
    /// sending the same plan again changes nothing (rule 20). `me` and `plan` only.
    pub fn add_tasks(
        &mut self,
        parent: Option<TaskId>,
        plan: &[PlanTask],
        actor: &Actor,
    ) -> Result<PlanResult> {
        self.write(|tx| add_tasks(tx, parent, plan, actor))
    }
}

fn add_tasks(
    tx: &mut Tx<'_>,
    parent: Option<TaskId>,
    plan: &[PlanTask],
    actor: &Actor,
) -> Result<PlanResult> {
    allowed(Call::AddPlan, actor)?;
    if let Some(p) = parent {
        ensure_exists(tx, p)?;
    }
    let steps = flatten(plan);
    validate(tx, &steps)?;

    let mut out = PlanResult::default();
    place(tx, parent, plan, &mut out)?;
    fault::hit(fault::PLAN_TASKS_WRITTEN)?;

    // `out.keys` is in the same depth-first order as `steps`.
    let by_key: HashMap<&str, TaskId> = out.keys.iter().map(|(k, id)| (k.as_str(), *id)).collect();
    let mut added: Vec<Vec<TaskId>> = Vec::with_capacity(steps.len());
    for (step, (_, id)) in steps.iter().zip(&out.keys) {
        let mut waits_on = Vec::new();
        for dep in &step.depends_on {
            let blocker = match dep {
                DepRef::Key(k) => *by_key.get(k.as_str()).ok_or(Error::NotFound)?,
                DepRef::Task(t) => *t,
            };
            // Rules 14 and 15, against everything written so far in this transaction.
            if deps::check_new(tx, *id, blocker)? {
                deps::insert(tx, *id, blocker, actor)?;
                waits_on.push(blocker);
            }
        }
        added.push(waits_on);
    }

    // Rule 21: one event per task this call changed.
    let created: HashSet<TaskId> = out.created.iter().copied().collect();
    for ((_, id), waits_on) in out.keys.iter().zip(&added) {
        let body = (!waits_on.is_empty()).then(|| {
            let list: Vec<String> = waits_on.iter().map(|t| format!("#{}", t.0)).collect();
            format!("waits on {}", list.join(", "))
        });
        if created.contains(id) {
            events::record(tx, *id, EventKind::Created, None, body.as_deref(), actor)?;
        } else if body.is_some() {
            events::record(
                tx,
                *id,
                EventKind::WaitsOnAdded,
                None,
                body.as_deref(),
                actor,
            )?;
        }
    }
    Ok(out)
}

/// Every step, depth-first: a phase, then its steps, then the next phase.
fn flatten(plan: &[PlanTask]) -> Vec<&PlanTask> {
    let mut out = Vec::new();
    for step in plan {
        out.push(step);
        out.extend(flatten(&step.children));
    }
    out
}

/// The checks that need only the plan and the rows already there (rule 19).
fn validate(conn: &Connection, steps: &[&PlanTask]) -> Result<()> {
    let mut keys = HashSet::new();
    for step in steps {
        if !keys.insert(step.key.as_str()) {
            return Err(Error::DuplicateKey(step.key.clone()));
        }
        clean_title(&step.title)?;
        if let Some(repo) = step.repo {
            crate::tasks::ensure_repo_exists(conn, repo)?;
        }
    }
    for dep in steps.iter().flat_map(|s| &s.depends_on) {
        match dep {
            DepRef::Key(k) if !keys.contains(k.as_str()) => return Err(Error::NotFound),
            DepRef::Key(_) => {}
            DepRef::Task(t) => ensure_exists(conn, *t)?,
        }
    }
    Ok(())
}

/// Resolves or creates each step under `parent`, then its children under it, recording
/// every step in `out` in plan order.
fn place(
    tx: &Tx<'_>,
    parent: Option<TaskId>,
    steps: &[PlanTask],
    out: &mut PlanResult,
) -> Result<()> {
    for step in steps {
        let id = match existing(tx, parent, &step.key)? {
            Some(id) => {
                out.existing.push(id);
                id
            }
            None => {
                let id = insert(tx, parent, step)?;
                out.created.push(id);
                id
            }
        };
        out.keys.push((step.key.clone(), id));
        place(tx, Some(id), &step.children, out)?;
    }
    Ok(())
}

/// The task a key already names under `parent` (rule 20), matched like the unique index.
fn existing(conn: &Connection, parent: Option<TaskId>, key: &str) -> Result<Option<TaskId>> {
    Ok(conn
        .query_row(
            "SELECT id FROM tasks WHERE ifnull(parent_id, 0) = ?1 AND plan_key = ?2",
            params![parent.map_or(0, |p| p.0), key],
            |r| r.get(0),
        )
        .optional()?
        .map(TaskId))
}

/// Writes one new step at the end of its siblings. Its event comes later, once its
/// dependencies are known.
fn insert(conn: &Connection, parent: Option<TaskId>, step: &PlanTask) -> Result<TaskId> {
    if let Some(p) = parent {
        // Rule 12. Only checked where something new goes: a finished step whose plan
        // is sent again unchanged still changes nothing.
        ensure_open_parent(conn, p)?;
    }
    let position = crate::tasks::next_position(conn, parent)?;
    conn.execute(
        "INSERT INTO tasks (parent_id, repo_id, title, body, status, position, origin, plan_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            parent.map(|p| p.0),
            step.repo.map(|r| r.0),
            clean_title(&step.title)?,
            step.body,
            Status::Todo.as_str(),
            position,
            Origin::Plan.as_str(),
            step.key
        ],
    )?;
    Ok(TaskId(conn.last_insert_rowid()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::{NewTask, Store};

    fn counts(s: &Store) -> (i64, i64, i64) {
        let n = |sql: &str| s.conn.query_row(sql, [], |r| r.get(0)).unwrap();
        (
            n("SELECT count(*) FROM tasks"),
            n("SELECT count(*) FROM task_events"),
            n("SELECT count(*) FROM task_deps"),
        )
    }

    #[test]
    fn a_failure_between_tasks_and_deps_leaves_nothing_behind() {
        let mut s = Store::open_in_memory().unwrap();
        let root = s.add_task(NewTask::new("Root"), &Actor::Me).unwrap().id;
        let before = counts(&s);
        let committed = s.last_committed.clone();
        let plan = vec![
            PlanTask::new("a", "A").child(PlanTask::new("a.1", "A1")),
            PlanTask::new("b", "B").after("a.1"),
        ];
        fault::arm(fault::PLAN_TASKS_WRITTEN);
        assert!(s.add_tasks(Some(root), &plan, &Actor::Plan).is_err());
        assert_eq!(counts(&s), before);
        assert_eq!(s.last_committed, committed);

        // The same plan goes through once the fault is gone.
        s.add_tasks(Some(root), &plan, &Actor::Plan).unwrap();
        assert_eq!(counts(&s), (before.0 + 3, before.1 + 3, before.2 + 1));
    }
}
