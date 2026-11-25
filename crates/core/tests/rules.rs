//! One test per SPEC-core rule, named after the behaviour.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use airboss_core::{Actor, Error, EventKind, NewTask, Origin, RepoId, Status, Store, TaskId};

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

fn agent() -> Actor {
    Actor::Agent("t3/abc".into())
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
        .add_task(NewTask::new("Orphan").under(TaskId(99)), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

#[test]
fn missing_repo_is_rejected() {
    let mut s = store();
    let err = s
        .add_task(NewTask::new("Somewhere").in_repo(RepoId(5)), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

#[test]
fn reading_a_missing_task_is_not_found() {
    let s = store();
    assert!(matches!(s.task(TaskId(1)).unwrap_err(), Error::NotFound));
}

// Rule 8: an agent's new task is only a proposal.

#[test]
fn a_task_added_by_an_agent_starts_as_proposed() {
    let mut s = store();
    let t = s
        .add_task(NewTask::new("Add retry to sync"), &agent())
        .unwrap();
    assert_eq!((t.status, t.origin), (Status::Proposed, Origin::Agent));
}

#[test]
fn sync_plan_and_launch_cannot_add_single_tasks() {
    let mut s = store();
    for actor in [Actor::SyncGh, Actor::SyncT3, Actor::Plan, Actor::Launch] {
        let err = s.add_task(NewTask::new("x"), &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }
}

#[test]
fn an_agent_with_an_empty_thread_ref_is_refused() {
    let mut s = store();
    let err = s
        .add_task(NewTask::new("x"), &Actor::Agent(String::new()))
        .unwrap_err();
    assert!(matches!(err, Error::NotAllowed));
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

// Repos: only `me` adds them, and names and paths are unique.

#[test]
fn only_me_adds_repos() {
    let mut s = store();
    let err = s
        .add_repo("web", Path::new("/w"), None, &agent())
        .unwrap_err();
    assert!(matches!(err, Error::NotAllowed));
}

#[test]
fn a_repo_name_or_path_used_twice_already_exists() {
    let mut s = store();
    s.add_repo("web", Path::new("/w/web"), None, &Actor::Me)
        .unwrap();
    let same_name = s
        .add_repo("web", Path::new("/w/other"), None, &Actor::Me)
        .unwrap_err();
    let same_path = s
        .add_repo("other", Path::new("/w/web"), None, &Actor::Me)
        .unwrap_err();
    assert!(matches!(same_name, Error::AlreadyExists), "{same_name:?}");
    assert!(matches!(same_path, Error::AlreadyExists), "{same_path:?}");
}

// Actors and statuses round-trip through their stored text.

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
            actor.to_string().parse::<Actor>(),
            Ok(actor.clone()),
            "{actor}"
        );
    }
    assert!("agent:".parse::<Actor>().is_err());
    assert!("someone".parse::<Actor>().is_err());
}

#[test]
fn actors_serialize_as_their_stored_text() {
    let json = serde_json_like(&Actor::Agent("t3/a".into()));
    assert_eq!(json, "agent:t3/a");
    let back: Actor = String::from("sync:gh").try_into().unwrap();
    assert_eq!(back, Actor::SyncGh);
}

/// `Actor` converts into the same `String` its serde form uses.
fn serde_json_like(actor: &Actor) -> String {
    String::from(actor.clone())
}

#[test]
fn every_status_reads_back_as_written() {
    for status in [
        Status::Proposed,
        Status::Todo,
        Status::Doing,
        Status::InReview,
        Status::Blocked,
        Status::Done,
        Status::Dropped,
    ] {
        assert_eq!(status.to_string().parse::<Status>(), Ok(status));
    }
}
