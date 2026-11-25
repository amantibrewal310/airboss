//! Rule 28: activity read in bulk, for a subtree and for every tree node.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, Error, EventKind, NewTask, Store, TaskId};
use common::{agent, store, tick};

fn top(s: &mut Store, title: &str) -> TaskId {
    s.add_task(NewTask::new(title), &Actor::Me).unwrap().id
}

fn under(s: &mut Store, parent: TaskId, title: &str) -> TaskId {
    s.add_task(NewTask::new(title).under(parent), &Actor::Me)
        .unwrap()
        .id
}

/// P has A, and A has A1; Q is a separate task. Each gets a note after all are created.
fn three_levels(s: &mut Store) -> [TaskId; 4] {
    let p = top(s, "P");
    let a = under(s, p, "A");
    let a1 = under(s, a, "A1");
    let q = top(s, "Q");
    for (id, text) in [(a1, "deep"), (q, "elsewhere"), (p, "top"), (a, "middle")] {
        s.add_note(id, text, &Actor::Me).unwrap();
    }
    [p, a, a1, q]
}

#[test]
fn subtree_history_covers_the_task_and_everything_under_it_newest_first() {
    let mut s = store();
    let [p, a, a1, _] = three_levels(&mut s);
    let events = s.history_with_subtasks(p, 100).unwrap();
    let got: Vec<(TaskId, EventKind)> = events.iter().map(|e| (e.task, e.kind)).collect();
    assert_eq!(
        got,
        vec![
            (a, EventKind::Note),
            (p, EventKind::Note),
            (a1, EventKind::Note),
            (a1, EventKind::Created),
            (a, EventKind::Created),
            (p, EventKind::Created),
        ]
    );
    assert!(events.windows(2).all(|w| w[0].id > w[1].id));
}

#[test]
fn subtree_history_stops_at_the_limit() {
    let mut s = store();
    let [p, a, ..] = three_levels(&mut s);
    let events = s.history_with_subtasks(p, 2).unwrap();
    let got: Vec<TaskId> = events.iter().map(|e| e.task).collect();
    assert_eq!(got, vec![a, p]);
    assert!(s.history_with_subtasks(p, 0).unwrap().is_empty());
}

#[test]
fn subtree_history_of_a_leaf_is_its_own_history_reversed() {
    let mut s = store();
    let [_, _, a1, _] = three_levels(&mut s);
    let mut own = s.history(a1).unwrap();
    own.reverse();
    assert_eq!(s.history_with_subtasks(a1, 10).unwrap(), own);
}

#[test]
fn subtree_history_of_a_missing_task_is_not_found() {
    let s = store();
    let err = s.history_with_subtasks(TaskId(3), 10).unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

#[test]
fn subtree_history_names_each_events_actor() {
    let mut s = store();
    let p = top(&mut s, "P");
    let a = under(&mut s, p, "A");
    s.add_note(a, "on it", &agent()).unwrap();
    let newest = &s.history_with_subtasks(p, 1).unwrap()[0];
    assert_eq!(
        (newest.task, newest.actor.clone(), newest.body.as_deref()),
        (a, agent(), Some("on it"))
    );
}

fn node(s: &Store, id: TaskId) -> airboss_core::TaskNode {
    s.tree()
        .unwrap()
        .into_iter()
        .find(|n| n.task.id == id)
        .unwrap()
}

fn newest_at(s: &Store, id: TaskId) -> String {
    s.history(id).unwrap().pop().unwrap().at
}

#[test]
fn a_tree_node_carries_its_own_newest_event_and_actor() {
    let mut s = store();
    let t = top(&mut s, "T");
    let fresh = node(&s, t);
    assert_eq!(fresh.last_actor, Actor::Me);
    assert_eq!(fresh.last_event_at, newest_at(&s, t));

    tick();
    s.add_note(t, "picked up", &agent()).unwrap();
    let n = node(&s, t);
    assert_eq!(n.last_actor, agent());
    assert_eq!(n.last_event_at, newest_at(&s, t));
    assert_eq!(n.subtree_last_event_at, n.last_event_at);
}

#[test]
fn a_subtrees_newest_event_comes_up_from_a_grandchild() {
    let mut s = store();
    let [p, a, a1, q] = three_levels(&mut s);
    tick();
    s.add_note(a1, "newest of all", &agent()).unwrap();
    let deepest = newest_at(&s, a1);

    for id in [p, a, a1] {
        assert_eq!(node(&s, id).subtree_last_event_at, deepest, "{id:?}");
    }
    // Each keeps its own newest event and actor.
    let parent = node(&s, p);
    assert_eq!(parent.last_event_at, newest_at(&s, p));
    assert_eq!(parent.last_actor, Actor::Me);
    // Another tree is untouched.
    assert_eq!(node(&s, q).subtree_last_event_at, newest_at(&s, q));
}

#[test]
fn the_newest_event_goes_by_event_id_not_time() {
    let mut s = store();
    let p = top(&mut s, "P");
    let a = under(&mut s, p, "A");
    tick();
    s.add_note(p, "parent note", &Actor::Me).unwrap();
    let newest = s.history_with_subtasks(p, 1).unwrap().remove(0);
    assert_eq!(newest.task, p);
    assert_eq!(node(&s, p).subtree_last_event_at, newest.at);
    assert_eq!(node(&s, a).subtree_last_event_at, newest_at(&s, a));
}
