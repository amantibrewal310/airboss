//! Parents are containers (rule 10): a task with steps shows a status worked out from them,
//! never one of its own.
//!
//! A step is a subtask that isn't a proposal: neither one still waiting for me nor one I
//! rejected. A task whose subtasks are all proposals has no steps yet, so it keeps its own
//! status. The stored status of a task with steps is ignored, never migrated.

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::events::bad_text;
use crate::model::Status;
use crate::sql::query_all;
use crate::{Result, TaskId};

/// SQL that is true when the task under `alias` is a proposal I rejected: still `dropped`,
/// and its latest status change was a `rejected` event. The tree hides these (rule 6).
pub(crate) fn rejected(alias: &str) -> String {
    format!(
        "({alias}.status = 'dropped'
          AND (SELECT e.kind FROM task_events e
               WHERE e.task_id = {alias}.id AND e.kind IN ('status', 'rejected')
               ORDER BY e.id DESC LIMIT 1) = 'rejected')"
    )
}

/// True when task `?1` has a step: a subtask that is neither `proposed` nor a rejected
/// proposal ([`rejected`], spelled out so the statement can be cached).
const HAS_STEPS_SQL: &str = "
SELECT EXISTS (
    SELECT 1 FROM tasks c
    WHERE c.parent_id = ?1 AND c.status <> 'proposed'
      AND NOT (c.status = 'dropped'
               AND (SELECT e.kind FROM task_events e
                    WHERE e.task_id = c.id AND e.kind IN ('status', 'rejected')
                    ORDER BY e.id DESC LIMIT 1) = 'rejected'))";

/// True when `id` has steps, so its status follows them and it takes no status change
/// (other than a drop), branch or PR of its own.
pub(crate) fn has_steps(conn: &Connection, id: TaskId) -> Result<bool> {
    let mut stmt = conn.prepare_cached(HAS_STEPS_SQL)?;
    Ok(stmt.query_row(params![id.0], |r| r.get(0))?)
}

/// One task as the roll-up sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    pub id: TaskId,
    pub parent: Option<TaskId>,
    pub position: u32,
    pub status: Status,
    pub blocked_reason: Option<String>,
}

/// What a task shows, and whether that came from its steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shown {
    pub status: Status,
    pub blocked_reason: Option<String>,
    pub has_steps: bool,
}

/// What a parent's direct steps add up to, in sibling order.
#[derive(Debug, Default)]
struct Steps {
    count: usize,
    dropped: usize,
    /// Done or dropped.
    finished: usize,
    active: bool,
    /// The first blocked step's reason, naming the task that is blocked.
    blocked: Option<String>,
}

impl Steps {
    fn add(&mut self, node: &Node, has_steps: bool) {
        match node.status {
            Status::Proposed => return,
            Status::Dropped => {
                self.dropped += 1;
                self.finished += 1;
            }
            Status::Done => self.finished += 1,
            Status::Doing | Status::InReview => self.active = true,
            Status::Blocked if self.blocked.is_none() => {
                let reason = node.blocked_reason.as_deref().unwrap_or("").trim();
                self.blocked = Some(if has_steps {
                    // Already names the step under it that is blocked.
                    reason.to_string()
                } else {
                    format!("#{} is blocked: {reason}", node.id.0)
                });
            }
            Status::Blocked | Status::Todo => {}
        }
        self.count += 1;
    }

    /// The status they show, or `None` when there are no steps.
    ///
    /// Dropped when every step is dropped; done when every step is done or dropped; blocked
    /// when any step is; in progress (`doing`) when any step is doing or in review, or some
    /// are finished and others not; otherwise `todo`.
    fn shown(self) -> Option<(Status, Option<String>)> {
        Some(match self {
            Steps { count: 0, .. } => return None,
            Steps { count, dropped, .. } if dropped == count => (Status::Dropped, None),
            Steps {
                count, finished, ..
            } if finished == count => (Status::Done, None),
            Steps {
                blocked: Some(why), ..
            } => (Status::Blocked, Some(why)),
            Steps {
                active, finished, ..
            } if active || finished > 0 => (Status::Doing, None),
            _ => (Status::Todo, None),
        })
    }
}

/// Replaces every parent's status and blocked reason in `nodes` with what its steps show,
/// from the deepest level up, so a phase's shown status counts in its plan's. Returns, by
/// position in `nodes`, whether each task has steps.
///
/// `nodes` can be any set of tasks; only subtasks inside the set count. A task whose parent
/// isn't in the set is a root. Tasks in a loop of parents (a damaged file) are left as they
/// are.
pub(crate) fn roll_up(nodes: &mut [Node]) -> Vec<bool> {
    let n = nodes.len();
    let index: HashMap<TaskId, usize> = nodes.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        match node.parent.and_then(|p| index.get(&p)) {
            Some(&p) => kids[p].push(i),
            None => roots.push(i),
        }
    }
    for k in &mut kids {
        k.sort_by_key(|&i| (nodes[i].position, nodes[i].id));
    }
    // Post-order without recursion: every task after all of its subtasks.
    let mut order = Vec::with_capacity(n);
    let mut stack: Vec<(usize, bool)> = roots.into_iter().rev().map(|i| (i, false)).collect();
    while let Some((i, expanded)) = stack.pop() {
        if expanded {
            order.push(i);
        } else {
            stack.push((i, true));
            stack.extend(kids[i].iter().rev().map(|&c| (c, false)));
        }
    }
    let mut parent = vec![false; n];
    for i in order {
        let mut steps = Steps::default();
        for &c in &kids[i] {
            steps.add(&nodes[c], parent[c]);
        }
        if let Some((status, why)) = steps.shown() {
            nodes[i].status = status;
            nodes[i].blocked_reason = why;
            parent[i] = true;
        }
    }
    parent
}

/// What every task in `root`'s subtree shows, `root` included. Rejected proposals are left
/// out, as the tree leaves them out; a rejected root isn't in the map.
pub(crate) fn shown_subtree(conn: &Connection, root: TaskId) -> Result<HashMap<TaskId, Shown>> {
    let sql = format!(
        "WITH RECURSIVE sub(id) AS (
             SELECT ?1
             UNION
             SELECT t.id FROM tasks t JOIN sub ON t.parent_id = sub.id
         )
         SELECT t.id, t.parent_id, t.position, t.status, t.blocked_reason
         FROM tasks t JOIN sub ON t.id = sub.id
         WHERE NOT {}",
        rejected("t")
    );
    let mut nodes = query_all(conn, &sql, params![root.0], |r| {
        let status: String = r.get(3)?;
        Ok(Node {
            id: TaskId(r.get(0)?),
            // The root's own parent is outside the walk, so it's a root here.
            parent: r.get::<_, Option<i64>>(1)?.map(TaskId),
            position: r.get(2)?,
            status: status.parse().map_err(|_| bad_text(3, &status))?,
            blocked_reason: r.get(4)?,
        })
    })?;
    let parents = roll_up(&mut nodes);
    Ok(nodes
        .into_iter()
        .zip(parents)
        .map(|(n, has_steps)| {
            (
                n.id,
                Shown {
                    status: n.status,
                    blocked_reason: n.blocked_reason,
                    has_steps,
                },
            )
        })
        .collect())
}

/// What `id` shows, or `None` when it doesn't exist or is a rejected proposal (which keeps
/// its stored status).
pub(crate) fn shown(conn: &Connection, id: TaskId) -> Result<Option<Shown>> {
    // Most tasks are steps: skip the walk when there's nothing under it.
    if !has_steps(conn, id)? {
        return Ok(None);
    }
    Ok(shown_subtree(conn, id)?.remove(&id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: i64, parent: Option<i64>, status: Status) -> Node {
        Node {
            id: TaskId(id),
            parent: parent.map(TaskId),
            position: u32::try_from(id).unwrap_or(0),
            status,
            blocked_reason: (status == Status::Blocked).then(|| format!("why {id}")),
        }
    }

    fn shown(steps: &[Status]) -> (Status, Option<String>) {
        let mut nodes = vec![node(1, None, Status::Done)];
        for (i, &s) in steps.iter().enumerate() {
            let id = i64::try_from(i).unwrap_or(0) + 2;
            nodes.push(node(id, Some(1), s));
        }
        roll_up(&mut nodes);
        (nodes[0].status, nodes[0].blocked_reason.clone())
    }

    use Status::*;

    #[test]
    fn a_parent_shows_what_its_steps_add_up_to() {
        assert_eq!(shown(&[Done, Dropped]).0, Done);
        assert_eq!(shown(&[Done, Done]).0, Done);
        assert_eq!(shown(&[Dropped, Dropped]).0, Dropped);
        assert_eq!(shown(&[Todo, Todo]).0, Todo);
        assert_eq!(shown(&[Todo, Doing]).0, Doing);
        assert_eq!(shown(&[Todo, InReview]).0, Doing);
        assert_eq!(shown(&[Todo, Done]).0, Doing);
        assert_eq!(shown(&[Todo, Dropped]).0, Doing);
        assert_eq!(shown(&[Doing, Blocked, Done]).0, Blocked);
    }

    #[test]
    fn proposals_are_not_steps() {
        assert_eq!(shown(&[Proposed, Done]).0, Done);
        // Nothing but proposals: no steps yet, so it keeps its own status.
        assert_eq!(shown(&[Proposed]).0, Done);
        assert_eq!(shown(&[]).0, Done);
    }

    #[test]
    fn a_blocked_parent_names_its_first_blocked_step() {
        let got = shown(&[Todo, Blocked, Blocked]);
        assert_eq!(got, (Blocked, Some("#3 is blocked: why 3".to_string())));
        assert_eq!(shown(&[Todo, Doing]).1, None);
    }

    #[test]
    fn a_phase_counts_in_its_plan_as_it_shows() {
        // Plan 1 > phase 2 (stored done) > steps 3 (todo), 4 (blocked); phase 5 > step 6 done.
        let mut nodes = vec![
            node(1, None, Todo),
            node(2, Some(1), Done),
            node(3, Some(2), Todo),
            node(4, Some(2), Blocked),
            node(5, Some(1), Todo),
            node(6, Some(5), Done),
        ];
        let parents = roll_up(&mut nodes);
        assert_eq!(parents, vec![true, true, false, false, true, false]);
        assert_eq!(nodes[1].status, Blocked);
        assert_eq!(nodes[4].status, Done);
        assert_eq!(nodes[0].status, Blocked);
        assert_eq!(
            nodes[0].blocked_reason.as_deref(),
            Some("#4 is blocked: why 4")
        );
    }

    #[test]
    fn a_loop_of_parents_is_left_alone() {
        let mut nodes = vec![node(1, Some(2), Todo), node(2, Some(1), Done)];
        assert_eq!(roll_up(&mut nodes), vec![false, false]);
        assert_eq!(nodes[1].status, Done);
    }
}
