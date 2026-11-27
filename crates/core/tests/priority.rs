//! Rule 35: a task's priority, P0 to P3 or none.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, Error, EventKind, NewTask, Priority, Status, TaskEdit};
use common::{agent, store};

#[test]
fn a_new_task_has_no_priority_unless_given_one() {
    let mut s = store();
    let plain = s.add_task(NewTask::new("Plain"), &Actor::Me).unwrap();
    let urgent = s
        .add_task(
            NewTask::new("Urgent").with_priority(Priority::P0),
            &Actor::Me,
        )
        .unwrap();
    assert_eq!(plain.priority, None);
    assert_eq!(urgent.priority, Some(Priority::P0));
    assert_eq!(s.task(urgent.id).unwrap().priority, Some(Priority::P0));
    let tree = s.tree().unwrap();
    assert_eq!(
        tree.iter().map(|n| n.task.priority).collect::<Vec<_>>(),
        vec![None, Some(Priority::P0)]
    );
}

#[test]
fn an_agents_proposal_keeps_the_priority_it_suggests() {
    let mut s = store();
    let t = s
        .add_task(NewTask::new("Idea").with_priority(Priority::P2), &agent())
        .unwrap();
    assert_eq!(
        (t.status, t.priority),
        (Status::Proposed, Some(Priority::P2))
    );
}

#[test]
fn me_sets_changes_and_clears_a_priority_with_one_edited_event_each() {
    let mut s = store();
    let t = s.add_task(NewTask::new("Ship it"), &Actor::Me).unwrap();
    let set = |p| TaskEdit::default().priority(p);

    let edited = s
        .edit_task(t.id, set(Some(Priority::P1)), &Actor::Me)
        .unwrap();
    assert_eq!(edited.priority, Some(Priority::P1));
    s.edit_task(t.id, set(Some(Priority::P3)), &Actor::Me)
        .unwrap();
    // The same priority again changes nothing and writes nothing.
    s.edit_task(t.id, set(Some(Priority::P3)), &Actor::Me)
        .unwrap();
    let cleared = s.edit_task(t.id, set(None), &Actor::Me).unwrap();
    assert_eq!(cleared.priority, None);
    // Leaving the priority out of an edit keeps it.
    s.edit_task(t.id, set(Some(Priority::P2)), &Actor::Me)
        .unwrap();
    let renamed = s
        .edit_task(t.id, TaskEdit::default().title("Ship it now"), &Actor::Me)
        .unwrap();
    assert_eq!(renamed.priority, Some(Priority::P2));

    let edits: Vec<String> = s
        .history(t.id)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EventKind::Edited)
        .map(|e| e.body.unwrap())
        .collect();
    assert_eq!(
        edits,
        [
            "priority: P1",
            "priority: P3",
            "priority cleared",
            "priority: P2",
            "title"
        ]
    );
}

#[test]
fn one_edit_names_every_field_it_changed() {
    let mut s = store();
    let t = s.add_task(NewTask::new("Old"), &Actor::Me).unwrap();
    let edit = TaskEdit::default()
        .title("New")
        .priority(Some(Priority::P0));
    s.edit_task(t.id, edit, &Actor::Me).unwrap();
    let last = s.history(t.id).unwrap().pop().unwrap();
    assert_eq!(last.body.as_deref(), Some("title, priority: P0"));
}

#[test]
fn only_me_sets_a_priority() {
    let mut s = store();
    let t = s.add_task(NewTask::new("Mine"), &Actor::Me).unwrap();
    for actor in [
        agent(),
        Actor::Plan,
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ] {
        let edit = TaskEdit::default().priority(Some(Priority::P0));
        let err = s.edit_task(t.id, edit, &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }
    assert_eq!(s.task(t.id).unwrap().priority, None);
}

#[test]
fn a_parent_keeps_its_own_priority_whatever_its_steps_have() {
    let mut s = store();
    let parent = s
        .add_task(NewTask::new("Plan").with_priority(Priority::P3), &Actor::Me)
        .unwrap();
    s.add_task(
        NewTask::new("Step")
            .under(parent.id)
            .with_priority(Priority::P0),
        &Actor::Me,
    )
    .unwrap();
    assert_eq!(s.task(parent.id).unwrap().priority, Some(Priority::P3));
}

#[test]
fn priorities_sort_p0_first_and_read_in_either_case() {
    let mut all = vec![Priority::P3, Priority::P0, Priority::P2, Priority::P1];
    all.sort();
    assert_eq!(all, Priority::ALL);
    assert_eq!("p1".parse::<Priority>().unwrap(), Priority::P1);
    assert_eq!("P3".parse::<Priority>().unwrap(), Priority::P3);
    assert!("P4".parse::<Priority>().is_err());
    assert!("high".parse::<Priority>().is_err());
    assert_eq!(Priority::P2.to_string(), "P2");
}
