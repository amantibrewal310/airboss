//! Dropping and deleting subtrees: rules 5, 9, 13 and 17.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, Error, EventKind, LinkKind, NewLink, NewTask, Status, Store, TaskId};
use common::{agent, store};

fn top(s: &mut Store, title: &str) -> TaskId {
    s.add_task(NewTask::new(title), &Actor::Me).unwrap().id
}

fn under(s: &mut Store, parent: TaskId, title: &str) -> TaskId {
    s.add_task(NewTask::new(title).under(parent), &Actor::Me)
        .unwrap()
        .id
}

fn status(s: &Store, id: TaskId) -> Status {
    s.task(id).unwrap().status
}

fn set(s: &mut Store, id: TaskId, to: Status) {
    s.set_status(id, to, Some("setup"), &Actor::Me).unwrap();
}

fn event_count(s: &Store, id: TaskId) -> usize {
    s.history(id).unwrap().len()
}

/// The ids under `parent`, in position order, with their positions.
fn group(s: &Store, parent: Option<TaskId>) -> Vec<(TaskId, u32)> {
    let mut kids: Vec<_> = s
        .tree()
        .unwrap()
        .into_iter()
        .map(|n| n.task)
        .filter(|t| t.parent == parent)
        .map(|t| (t.id, t.position))
        .collect();
    kids.sort_by_key(|&(_, p)| p);
    kids
}

// ---- Rule 9: dropping drops the subtree ----

#[test]
fn dropping_a_three_level_subtree_drops_every_open_task_and_keeps_done_ones() {
    let mut s = store();
    let root = top(&mut s, "Root");
    let a = under(&mut s, root, "A");
    let b = under(&mut s, root, "B");
    let a1 = under(&mut s, a, "A1");
    let a2 = under(&mut s, a, "A2");
    let b1 = under(&mut s, b, "B1");
    let proposal = s
        .add_task(NewTask::new("Agent idea").under(b), &agent())
        .unwrap()
        .id;
    let outside = top(&mut s, "Outside");
    set(&mut s, a, Status::Doing);
    set(&mut s, a2, Status::Done);
    set(&mut s, b1, Status::Blocked);
    assert_eq!(status(&s, proposal), Status::Proposed);

    let open = [a, b, a1, b1, proposal];
    let before: Vec<usize> = [root, a, b, a1, a2, b1, proposal, outside]
        .iter()
        .map(|&id| event_count(&s, id))
        .collect();

    s.set_status(root, Status::Dropped, None, &Actor::Me)
        .unwrap();

    assert_eq!(status(&s, root), Status::Dropped);
    for id in open {
        assert_eq!(status(&s, id), Status::Dropped, "{id:?}");
        let history = s.history(id).unwrap();
        let last = history.last().unwrap();
        assert_eq!(last.kind, EventKind::Status);
        assert_eq!(last.to_status, Some(Status::Dropped));
        assert_eq!(last.actor, Actor::Me);
        assert_eq!(
            last.body.as_deref(),
            Some(&*format!("dropped with #{}", root.0))
        );
    }
    assert_eq!(s.task(b1).unwrap().blocked_reason, None, "rule 1");
    assert_eq!(status(&s, a2), Status::Done);
    assert_eq!(status(&s, outside), Status::Todo);

    let after: Vec<usize> = [root, a, b, a1, a2, b1, proposal, outside]
        .iter()
        .map(|&id| event_count(&s, id))
        .collect();
    let added: Vec<usize> = after.iter().zip(&before).map(|(x, y)| x - y).collect();
    // root, a, b, a1 dropped; a2 done; b1, proposal dropped; outside untouched.
    assert_eq!(added, vec![1, 1, 1, 1, 0, 1, 1, 0]);
}

#[test]
fn dropping_a_dropped_parent_again_drops_what_was_reopened_under_it() {
    let mut s = store();
    let p = top(&mut s, "P");
    let child = under(&mut s, p, "Child");
    s.set_status(p, Status::Dropped, None, &Actor::Me).unwrap();
    set(&mut s, child, Status::Todo);
    let parent_events = event_count(&s, p);
    let child_events = event_count(&s, child);

    s.set_status(p, Status::Dropped, None, &Actor::Me).unwrap();

    assert_eq!(status(&s, child), Status::Dropped);
    assert_eq!(event_count(&s, child), child_events + 1);
    assert_eq!(
        event_count(&s, p),
        parent_events,
        "the parent itself is unchanged"
    );
}

#[test]
fn dropping_a_leaf_drops_only_the_leaf() {
    let mut s = store();
    let p = top(&mut s, "P");
    let leaf = under(&mut s, p, "Leaf");
    let sibling = under(&mut s, p, "Sibling");

    s.set_status(leaf, Status::Dropped, None, &Actor::Me)
        .unwrap();

    assert_eq!(status(&s, leaf), Status::Dropped);
    assert_eq!(status(&s, p), Status::Todo);
    assert_eq!(status(&s, sibling), Status::Todo);
    assert_eq!(event_count(&s, p), 1);
    assert_eq!(event_count(&s, sibling), 1);
}

#[test]
fn only_me_can_drop_a_subtree() {
    let mut s = store();
    let p = top(&mut s, "P");
    let child = under(&mut s, p, "Child");
    for actor in [
        agent(),
        Actor::Plan,
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ] {
        let err = s
            .set_status(p, Status::Dropped, Some("no"), &actor)
            .unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor:?}: {err:?}");
    }
    assert_eq!(status(&s, p), Status::Todo);
    assert_eq!(status(&s, child), Status::Todo);
    assert_eq!(event_count(&s, child), 1);
}

// ---- delete_task: rules 5, 13 and 17 ----

#[test]
fn deleting_removes_the_subtree_and_renumbers_the_siblings() {
    let mut s = store();
    let p = top(&mut s, "P");
    let first = under(&mut s, p, "First");
    let doomed = under(&mut s, p, "Doomed");
    let last = under(&mut s, p, "Last");
    let child = under(&mut s, doomed, "Child");
    let grandchild = under(&mut s, child, "Grandchild");
    s.link(grandchild, NewLink::pr("acme/web#7"), &Actor::Me)
        .unwrap();
    s.link(doomed, NewLink::new(LinkKind::Doc, "notes.md"), &Actor::Me)
        .unwrap();
    s.add_dependency(grandchild, first, &Actor::Me).unwrap();
    s.add_note(child, "a note", &Actor::Me).unwrap();

    s.delete_task(doomed, &Actor::Me).unwrap();

    for id in [doomed, child, grandchild] {
        assert!(matches!(s.task(id), Err(Error::NotFound)), "{id:?}");
        assert!(matches!(s.history(id), Err(Error::NotFound)), "{id:?}");
    }
    assert!(
        s.tasks_for_link(LinkKind::Pr, "acme/web#7")
            .unwrap()
            .is_empty()
    );
    assert!(
        s.tasks_for_link(LinkKind::Doc, "notes.md")
            .unwrap()
            .is_empty()
    );
    assert_eq!(group(&s, Some(p)), vec![(first, 1), (last, 2)]);
    assert_eq!(s.tree().unwrap().len(), 3);
    // The surviving blocker has nobody waiting on it, and wrote no event.
    assert_eq!(event_count(&s, first), 1);
    assert_eq!(event_count(&s, last), 1, "renumbering writes no events");
}

#[test]
fn deleting_a_top_level_task_renumbers_the_top_level() {
    let mut s = store();
    let a = top(&mut s, "A");
    let b = top(&mut s, "B");
    let c = top(&mut s, "C");
    s.delete_task(a, &Actor::Me).unwrap();
    assert_eq!(group(&s, None), vec![(b, 1), (c, 2)]);
}

#[test]
fn deleting_a_blocker_tells_its_waiters_and_makes_them_ready() {
    let mut s = store();
    let blocker = top(&mut s, "Blocker");
    let inner = under(&mut s, blocker, "Inner");
    let w1 = top(&mut s, "W1");
    let w2 = top(&mut s, "W2");
    s.add_dependency(w1, blocker, &Actor::Me).unwrap();
    s.add_dependency(w2, inner, &Actor::Me).unwrap();
    assert!(!s.ready().unwrap().iter().any(|t| t.id == w1 || t.id == w2));

    s.delete_task(blocker, &Actor::Me).unwrap();

    for w in [w1, w2] {
        let last = s.history(w).unwrap().pop().unwrap();
        assert_eq!(last.kind, EventKind::WaitsOnRemoved);
        assert_eq!(last.actor, Actor::Me);
        assert!(s.waits_on(w).unwrap().is_empty());
    }
    let ready: Vec<_> = s.ready().unwrap().iter().map(|t| t.id).collect();
    assert_eq!(ready, vec![w1, w2]);
}

#[test]
fn only_me_can_delete_a_task() {
    let mut s = store();
    let t = top(&mut s, "T");
    for actor in [
        agent(),
        Actor::Plan,
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ] {
        let err = s.delete_task(t, &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor:?}: {err:?}");
    }
    assert_eq!(s.task(t).unwrap().title, "T");
}

#[test]
fn deleting_a_missing_task_is_not_found() {
    let mut s = store();
    top(&mut s, "T");
    assert!(matches!(
        s.delete_task(TaskId(999), &Actor::Me),
        Err(Error::NotFound)
    ));
    assert_eq!(s.tree().unwrap().len(), 1);
}
