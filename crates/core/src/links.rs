//! Links to outside things (threads, PRs, docs …) and the state of linked PRs.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::events::{self, bad_text};
use crate::model::{Actor, EventKind, Link, LinkKind, NewLink, PrState, Status};
use crate::permissions::{Call, allowed};
use crate::tasks::ensure_exists;
use crate::tx::Tx;
use crate::{Error, LinkId, Result, TaskId, fault, status};

const LINK_COLUMNS: &str = "id, task_id, kind, ref, url, pr_state, linked_by, linked_at";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Link> {
    let kind: String = row.get(2)?;
    let pr_state: Option<String> = row.get(5)?;
    let linked_by: String = row.get(6)?;
    Ok(Link {
        id: LinkId(row.get(0)?),
        task: TaskId(row.get(1)?),
        kind: LinkKind::parse(&kind).ok_or_else(|| bad_text(2, &kind))?,
        r#ref: row.get(3)?,
        url: row.get(4)?,
        pr_state: match pr_state {
            Some(s) => Some(PrState::parse(&s).ok_or_else(|| bad_text(5, &s))?),
            None => None,
        },
        linked_by: linked_by.parse().map_err(|_| bad_text(6, &linked_by))?,
        linked_at: row.get(7)?,
    })
}

fn get(conn: &Connection, id: LinkId) -> Result<Link> {
    conn.query_row(
        &format!("SELECT {LINK_COLUMNS} FROM links WHERE id = ?1"),
        params![id.0],
        from_row,
    )
    .optional()?
    .ok_or(Error::NotFound)
}

/// Attaches `new` to the task, or returns the link that's already there (no event).
pub(crate) fn link(tx: &mut Tx<'_>, task: TaskId, new: NewLink, actor: &Actor) -> Result<Link> {
    allowed(Call::Link, actor)?;
    ensure_exists(tx, task)?;
    let existing = tx
        .query_row(
            &format!(
                "SELECT {LINK_COLUMNS} FROM links WHERE task_id = ?1 AND kind = ?2 AND ref = ?3"
            ),
            params![task.0, new.kind.as_str(), new.r#ref],
            from_row,
        )
        .optional()?;
    if let Some(found) = existing {
        return Ok(found);
    }
    // Rule 3: a PR link always has a state; the database rejects a state on anything else.
    let pr_state = match new.kind {
        LinkKind::Pr => Some(new.pr_state.unwrap_or(PrState::Open)),
        _ => new.pr_state,
    };
    tx.execute(
        "INSERT INTO links (task_id, kind, ref, url, pr_state, linked_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            task.0,
            new.kind.as_str(),
            new.r#ref,
            new.url,
            pr_state.map(PrState::as_str),
            actor.to_string()
        ],
    )?;
    let id = LinkId(tx.last_insert_rowid());
    fault::hit(fault::LINK_INSERTED)?;
    let body = format!("{} {}", new.kind, new.r#ref);
    events::record(tx, task, EventKind::Linked, None, Some(&body), actor)?;
    // Rule 7: code can't stay done while an open PR someone else just linked is unreviewed.
    if new.kind == LinkKind::Pr
        && pr_state == Some(PrState::Open)
        && *actor != Actor::Me
        && crate::tasks::get(tx, task)?.status == Status::Done
    {
        let why = format!("reopened: PR {} linked after done", new.r#ref);
        status::apply(tx, task, Status::InReview, Some(&why), actor)?;
    }
    get(tx, id)
}

/// Removes a link. `me` removes any; an agent only those it added (rule 8).
pub(crate) fn unlink(tx: &mut Tx<'_>, id: LinkId, actor: &Actor) -> Result<()> {
    allowed(Call::Unlink, actor)?;
    let link = get(tx, id)?;
    if *actor != Actor::Me && link.linked_by != *actor {
        return Err(Error::NotAllowed);
    }
    tx.execute("DELETE FROM links WHERE id = ?1", params![id.0])?;
    fault::hit(fault::LINK_DELETED)?;
    let body = format!("{} {}", link.kind, link.r#ref);
    events::record(tx, link.task, EventKind::Unlinked, None, Some(&body), actor).map(|_| ())
}

/// Records a PR's new state on every task linked to it, one `link_state` event each, and
/// returns the tasks whose link actually changed. A PR reopened on a `done` task moves the
/// task back to `in_review` (rule 7).
pub(crate) fn set_pr_state(
    tx: &mut Tx<'_>,
    pr_ref: &str,
    state: PrState,
    actor: &Actor,
) -> Result<Vec<TaskId>> {
    allowed(Call::SetPrState, actor)?;
    let tasks: Vec<TaskId> = {
        let mut stmt = tx.prepare(
            "SELECT task_id FROM links WHERE kind = 'pr' AND ref = ?1 AND pr_state <> ?2
             ORDER BY task_id",
        )?;
        let rows = stmt.query_map(params![pr_ref, state.as_str()], |r| r.get(0))?;
        rows.map(|r| r.map(TaskId))
            .collect::<rusqlite::Result<_>>()?
    };
    let body = format!("{pr_ref} {state}");
    for &task in &tasks {
        tx.execute(
            "UPDATE links SET pr_state = ?3 WHERE task_id = ?1 AND kind = 'pr' AND ref = ?2",
            params![task.0, pr_ref, state.as_str()],
        )?;
        events::record(tx, task, EventKind::LinkState, None, Some(&body), actor)?;
        fault::hit(fault::PR_STATE_RECORDED)?;
        // Rule 7: a PR reopened on GitHub needs review again, so its task can't stay done.
        if state == PrState::Open && crate::tasks::get(tx, task)?.status == Status::Done {
            let why = format!("reopened: PR {pr_ref} reopened");
            status::apply(tx, task, Status::InReview, Some(&why), actor)?;
        }
    }
    Ok(tasks)
}

pub(crate) fn links(conn: &Connection, task: TaskId) -> Result<Vec<Link>> {
    ensure_exists(conn, task)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {LINK_COLUMNS} FROM links WHERE task_id = ?1 ORDER BY id"
    ))?;
    let rows = stmt.query_map(params![task.0], from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::from)
}

pub(crate) fn tasks_for_link(
    conn: &Connection,
    kind: LinkKind,
    r#ref: &str,
) -> Result<Vec<TaskId>> {
    let mut stmt =
        conn.prepare("SELECT task_id FROM links WHERE kind = ?1 AND ref = ?2 ORDER BY task_id")?;
    let rows = stmt.query_map(params![kind.as_str(), r#ref], |r| r.get(0))?;
    rows.map(|r| r.map(TaskId))
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::from)
}

impl crate::Store {
    /// Attaches an outside thing to a task. Linking the same thing twice returns the
    /// existing link, so callers can link without checking first.
    pub fn link(&mut self, task: TaskId, new: NewLink, actor: &Actor) -> Result<Link> {
        self.write(|tx| link(tx, task, new, actor))
    }

    /// Removes a link. An agent may only remove links it added.
    pub fn unlink(&mut self, link: LinkId, actor: &Actor) -> Result<()> {
        self.write(|tx| unlink(tx, link, actor))
    }

    /// Records a PR's state on every task linked to it; returns the tasks that changed, so
    /// `sync:gh` knows which ones to check for finishing.
    pub fn set_pr_state(
        &mut self,
        pr_ref: &str,
        state: PrState,
        actor: &Actor,
    ) -> Result<Vec<TaskId>> {
        self.write(|tx| set_pr_state(tx, pr_ref, state, actor))
    }

    /// The tasks linked to one outside thing, e.g. every task a PR belongs to.
    pub fn tasks_for_link(&self, kind: LinkKind, r#ref: &str) -> Result<Vec<TaskId>> {
        tasks_for_link(&self.conn, kind, r#ref)
    }

    /// A task's links, oldest first.
    pub fn links(&self, task: TaskId) -> Result<Vec<Link>> {
        links(&self.conn, task)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::{NewTask, Store, fault};

    fn counts(store: &Store) -> (i64, i64, i64) {
        let n = |sql: &str| store.conn.query_row(sql, [], |r| r.get(0)).unwrap();
        (
            n("SELECT count(*) FROM links"),
            n("SELECT count(*) FROM task_events"),
            n("SELECT count(*) FROM links WHERE pr_state = 'merged'"),
        )
    }

    fn task(store: &mut Store) -> TaskId {
        store.add_task(NewTask::new("x"), &Actor::Me).unwrap().id
    }

    #[test]
    fn a_failed_link_leaves_no_link_and_no_event() {
        let mut store = Store::open_in_memory().unwrap();
        let t = task(&mut store);
        let before = counts(&store);
        fault::arm(fault::LINK_INSERTED);
        assert!(store.link(t, NewLink::pr("a/b#1"), &Actor::Me).is_err());
        assert_eq!(counts(&store), before);
    }

    #[test]
    fn a_failed_unlink_keeps_the_link_and_writes_no_event() {
        let mut store = Store::open_in_memory().unwrap();
        let t = task(&mut store);
        let link = store.link(t, NewLink::pr("a/b#1"), &Actor::Me).unwrap();
        let before = counts(&store);
        fault::arm(fault::LINK_DELETED);
        assert!(store.unlink(link.id, &Actor::Me).is_err());
        assert_eq!(counts(&store), before);
    }

    #[test]
    fn a_failed_pr_state_change_rolls_back_every_task_it_had_reached() {
        let mut store = Store::open_in_memory().unwrap();
        for _ in 0..2 {
            let t = task(&mut store);
            store.link(t, NewLink::pr("a/b#1"), &Actor::Me).unwrap();
        }
        let before = counts(&store);
        // Fails after the first task's update and event are written.
        fault::arm(fault::PR_STATE_RECORDED);
        assert!(
            store
                .set_pr_state("a/b#1", PrState::Merged, &Actor::SyncGh)
                .is_err()
        );
        assert_eq!(counts(&store), before);
    }
}
