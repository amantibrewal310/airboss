//! Status changes and who may make them: SPEC-core rule 6, plus rule 1's blocked reason.

use rusqlite::{Connection, OptionalExtension, params};

use crate::events::{self, bad_text};
use crate::model::{Actor, EventKind, Status, Task};
use crate::sql::{SUBTREE_CTE, query_all};
use crate::tx::Tx;
use crate::{Error, Result, TaskId, fault, tasks};

/// What rule 6 needs to know about a task, read inside the change's transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TaskFacts {
    status: Status,
    has_pr: bool,
    /// True when no linked PR is `open` (vacuously true with no PRs).
    prs_all_merged_or_closed: bool,
    any_pr_merged: bool,
}

fn facts(conn: &Connection, id: TaskId) -> Result<TaskFacts> {
    let row: Option<(String, i64, i64, i64)> = conn
        .query_row(
            "SELECT t.status, count(l.id),
                    ifnull(sum(l.pr_state = 'open'), 0), ifnull(sum(l.pr_state = 'merged'), 0)
             FROM tasks t LEFT JOIN links l ON l.task_id = t.id AND l.kind = 'pr'
             WHERE t.id = ?1 GROUP BY t.id",
            params![id.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let (status, prs, open, merged) = row.ok_or(Error::NotFound)?;
    Ok(TaskFacts {
        status: status.parse().map_err(|_| bad_text(0, &status))?,
        has_pr: prs > 0,
        prs_all_merged_or_closed: open == 0,
        any_pr_merged: merged > 0,
    })
}

/// Agents move work forward and can finish PR-less work; code finishes only when its PRs
/// merge. `InvalidTransition` means the table has a row for this actor and target, but not
/// from this status; `NotAllowed` means no row lets this actor do this at all.
fn check_status_change(
    actor: &Actor,
    task: &TaskFacts,
    to: Status,
    note: Option<&str>,
) -> Result<()> {
    use Status::*;
    let open = matches!(task.status, Todo | Doing | InReview | Blocked);
    let from = |ok: bool| {
        if ok {
            Ok(())
        } else {
            Err(Error::InvalidTransition)
        }
    };
    let allowed_if = |ok: bool| if ok { Ok(()) } else { Err(Error::NotAllowed) };
    match (actor, to) {
        (Actor::Agent(r), _) if r.trim().is_empty() => Err(Error::NotAllowed),
        (Actor::Me, Proposed) => Err(Error::NotAllowed),
        (Actor::Me, _) => Ok(()),
        (Actor::Agent(_), Doing | InReview | Blocked) => from(open),
        (Actor::Agent(_), Done) => from(open).and(allowed_if(!task.has_pr)),
        (Actor::SyncGh, Done) => from(open).and(allowed_if(
            task.prs_all_merged_or_closed && task.any_pr_merged,
        )),
        (Actor::Launch, Doing) => from(matches!(task.status, Todo | Blocked)),
        _ => Err(Error::NotAllowed),
    }?;
    // Rule 1, and rule 6's agent-done row: these changes must say why.
    let needs_note = to == Blocked || (to == Done && matches!(actor, Actor::Agent(_)));
    if needs_note && note.is_none_or(|n| n.trim().is_empty()) {
        return Err(Error::NoteRequired);
    }
    Ok(())
}

/// Writes a status change and its event, without checking who may make it. The note
/// becomes the blocked reason when blocking; any other status clears the reason (rule 1).
pub(crate) fn apply(
    tx: &mut Tx<'_>,
    id: TaskId,
    to: Status,
    note: Option<&str>,
    actor: &Actor,
) -> Result<()> {
    let reason = if to == Status::Blocked { note } else { None };
    tx.execute(
        "UPDATE tasks SET status = ?2, blocked_reason = ?3 WHERE id = ?1",
        params![id.0, to.as_str(), reason],
    )?;
    fault::hit(fault::STATUS_UPDATED)?;
    events::record(tx, id, EventKind::Status, Some(to), note, actor)
}

pub(crate) fn set_status(
    tx: &mut Tx<'_>,
    id: TaskId,
    to: Status,
    note: Option<&str>,
    actor: &Actor,
) -> Result<Task> {
    let note = note.filter(|n| !n.trim().is_empty());
    check_status_change(actor, &facts(tx, id)?, to, note)?;
    // A change that changes nothing writes nothing, like an empty edit.
    let before = tasks::get(tx, id)?;
    let reason = if to == Status::Blocked { note } else { None };
    if before.status != to || before.blocked_reason.as_deref() != reason {
        apply(tx, id, to, note, actor)?;
    }
    // Still runs when the task was already dropped, to catch anything reopened under it.
    if to == Status::Dropped {
        drop_subtree(tx, id, actor)?;
    }
    tasks::get(tx, id)
}

/// How rule 9's events start, so `tree()` can tell a task dropped with its parent from a
/// proposal I rejected.
pub(crate) const DROPPED_WITH: &str = "dropped with #";

/// Rule 9: drops every open task under `id`, with one event each. `done` ones stay `done`.
fn drop_subtree(tx: &mut Tx<'_>, id: TaskId, actor: &Actor) -> Result<()> {
    let open = query_all(
        tx,
        &format!(
            "{SUBTREE_CTE}
             SELECT t.id FROM tasks t JOIN sub ON t.id = sub.id
             WHERE t.id <> ?1 AND t.status NOT IN ('done', 'dropped') ORDER BY t.id"
        ),
        params![id.0],
        |r| r.get(0).map(TaskId),
    )?;
    let body = format!("{DROPPED_WITH}{}", id.0);
    for task in open {
        apply(tx, task, Status::Dropped, Some(&body), actor)?;
    }
    fault::hit(fault::SUBTREE_DROPPED)
}

impl crate::Store {
    /// Moves a task to a new status if rule 6 lets `actor` make that change. The note is
    /// the reason when blocking, and what was done when an agent finishes.
    pub fn set_status(
        &mut self,
        id: TaskId,
        to: Status,
        note: Option<&str>,
        actor: &Actor,
    ) -> Result<Task> {
        self.write(|tx| set_status(tx, id, to, note, actor))
    }

    /// Agents' proposals waiting for me to accept (`todo`) or reject (`dropped`), in tree order.
    pub fn proposed(&self) -> Result<Vec<Task>> {
        Ok(tasks::tree(&self.conn)?
            .into_iter()
            .map(|n| n.task)
            .filter(|t| t.status == Status::Proposed)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::{NewLink, NewTask, Store, fault};

    fn snapshot(store: &Store, id: TaskId) -> (Task, i64, i64) {
        let n = |sql: &str| store.conn.query_row(sql, [], |r| r.get(0)).unwrap();
        (
            store.task(id).unwrap(),
            n("SELECT count(*) FROM task_events"),
            n("SELECT count(*) FROM links"),
        )
    }

    #[test]
    fn a_failed_status_change_leaves_the_task_and_history_unchanged() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.add_task(NewTask::new("x"), &Actor::Me).unwrap().id;
        let before = snapshot(&store, id);
        fault::arm(fault::STATUS_UPDATED);
        let got = store.set_status(id, Status::Blocked, Some("why"), &Actor::Me);
        assert!(got.is_err());
        assert_eq!(snapshot(&store, id), before);
    }

    #[test]
    fn a_failed_reopen_also_undoes_the_link() {
        let mut store = Store::open_in_memory().unwrap();
        let id = store.add_task(NewTask::new("x"), &Actor::Me).unwrap().id;
        store
            .set_status(id, Status::Done, None, &Actor::Me)
            .unwrap();
        let before = snapshot(&store, id);
        fault::arm(fault::STATUS_UPDATED);
        let got = store.link(id, NewLink::pr("a/b#1"), &Actor::SyncGh);
        assert!(got.is_err());
        assert_eq!(snapshot(&store, id), before);
    }
}
