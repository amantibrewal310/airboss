//! One test per SPEC-core rule, named after the behaviour.
#![allow(clippy::unwrap_used)]

use airboss_core::{Actor, Error, EventKind, NewTask, Store, TaskId};

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

// Rule 2: a title can't be blank, and a parent must exist.

#[test]
fn blank_title_is_rejected() {
    let mut s = store();
    let err = s.add_task(NewTask::new("   "), &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::EmptyTitle), "{err:?}");
    assert!(s.tree().unwrap().is_empty());
}

#[test]
fn title_is_trimmed() {
    let mut s = store();
    let t = s
        .add_task(NewTask::new("  Fix typo  "), &Actor::Me)
        .unwrap();
    assert_eq!(t.title, "Fix typo");
}

#[test]
fn missing_parent_is_rejected() {
    let mut s = store();
    let err = s
        .add_task(NewTask::new("Orphan").under(Some(TaskId(99))), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

#[test]
fn reading_a_missing_task_is_not_found() {
    let s = store();
    assert!(matches!(s.task(TaskId(1)).unwrap_err(), Error::NotFound));
}

// Rule 21: one event per task per call, written with the change.

#[test]
fn adding_a_task_writes_one_created_event_by_its_actor() {
    let mut s = store();
    let t = s.add_task(NewTask::new("Fix typo"), &Actor::Me).unwrap();
    let history = s.history(t.id).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, EventKind::Created);
    assert_eq!(history[0].actor, Actor::Me);
    assert_eq!(history[0].task, t.id);
}

#[test]
fn history_of_a_missing_task_is_not_found() {
    let s = store();
    assert!(matches!(s.history(TaskId(7)).unwrap_err(), Error::NotFound));
}

// Rule 8: only `me` adds repos (other actors never need to).

#[test]
fn only_me_adds_repos() {
    let mut s = store();
    let err = s
        .add_repo(
            "web",
            std::path::Path::new("/w"),
            None,
            &Actor::Agent("t3/abc".into()),
        )
        .unwrap_err();
    assert!(matches!(err, Error::NotAllowed));
}

// Actors round-trip through storage.

#[test]
fn every_actor_kind_reads_back_as_written() {
    for actor in [
        Actor::Me,
        Actor::Agent("t3/97b5c8b5".into()),
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Plan,
        Actor::Launch,
    ] {
        assert_eq!(
            Actor::parse(&actor.to_string()),
            Some(actor.clone()),
            "{actor}"
        );
    }
    assert_eq!(Actor::parse("agent:"), None);
    assert_eq!(Actor::parse("someone"), None);
}
