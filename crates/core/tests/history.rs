//! Rule 28: activity read in bulk, for a subtree and for every tree node.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, Error, EventKind, NewTask, Store, TaskId};
use common::{agent, store};

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
