//! Rule 30: one consistent read, and the small reads it is made of.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, CoreSnapshot, EventId, NewTask, Status, Store, TaskId};
use common::store;

fn top(s: &mut Store, title: &str) -> TaskId {
    s.add_task(NewTask::new(title), &Actor::Me).unwrap().id
}

fn under(s: &mut Store, parent: TaskId, title: &str) -> TaskId {
    s.add_task(NewTask::new(title).under(parent), &Actor::Me)
        .unwrap()
        .id
}

/// P has A and B; C waits on A (done) and B (open); D waits on C; E is free.
fn fixture(s: &mut Store) -> [TaskId; 6] {
    let p = top(s, "P");
    let a = under(s, p, "A");
    let b = under(s, p, "B");
    let c = top(s, "C");
    let d = top(s, "D");
    let e = top(s, "E");
    for (task, on) in [(c, b), (c, a), (d, c)] {
        s.add_dependency(task, on, &Actor::Me).unwrap();
    }
    s.set_status(a, Status::Done, None, &Actor::Me).unwrap();
    s.add_note(e, "latest", &Actor::Me).unwrap();
    [p, a, b, c, d, e]
}

#[test]
fn an_empty_store_has_an_empty_snapshot_and_event_id_zero() {
    let s = store();
    assert_eq!(s.latest_event_id().unwrap(), EventId(0));
    assert!(s.dependency_edges().unwrap().is_empty());
    let snap = s.snapshot().unwrap();
    assert_eq!(
        snap,
        CoreSnapshot {
            tree: vec![],
            edges: vec![],
            ready: vec![],
            latest_event_id: EventId(0),
        }
    );
}

#[test]
fn dependency_edges_lists_every_pair_with_finished_ones_sorted() {
    let mut s = store();
    let [_, a, b, c, d, _] = fixture(&mut s);
    assert_eq!(s.dependency_edges().unwrap(), vec![(c, a), (c, b), (d, c)]);
}

#[test]
fn latest_event_id_is_the_newest_event() {
    let mut s = store();
    fixture(&mut s);
    let newest = s.events_after(EventId(0)).unwrap().pop().unwrap().id;
    assert_eq!(s.latest_event_id().unwrap(), newest);
}

#[test]
fn a_snapshot_matches_each_read_made_on_its_own() {
    let mut s = store();
    let [_, _, b, ..] = fixture(&mut s);
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.tree, s.tree().unwrap());
    assert_eq!(snap.edges, s.dependency_edges().unwrap());
    assert_eq!(snap.ready, s.ready().unwrap());
    assert_eq!(snap.latest_event_id, s.latest_event_id().unwrap());

    // B and E are ready; C and D wait; P has steps, so it isn't ready itself (rule 10).
    let ready: Vec<TaskId> = snap.ready.iter().map(|r| r.task.id).collect();
    assert!(ready.contains(&b));
    assert_eq!(ready.len(), 2);
}

#[test]
fn a_snapshot_follows_each_change() {
    let mut s = store();
    let [_, _, b, c, ..] = fixture(&mut s);
    let before = s.snapshot().unwrap();
    s.set_status(b, Status::Done, None, &Actor::Me).unwrap();
    let after = s.snapshot().unwrap();
    assert!(after.latest_event_id > before.latest_event_id);
    assert!(after.ready.iter().any(|r| r.task.id == c));
    assert!(!before.ready.iter().any(|r| r.task.id == c));
}
