//! One test per SPEC-core rule, named after the behaviour.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use airboss_core::{
    Actor, Error, EventKind, LinkKind, NewLink, NewTask, Origin, PrState, RepoId, Status, Store,
    TaskEdit, TaskId,
};

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

// Rule 8, `edit_task`: only `me` edits title, notes and repo; one `edited` event.

#[test]
fn me_edits_title_body_and_repo_with_one_edited_event() {
    let mut s = store();
    let repo = s
        .add_repo("web", Path::new("/w/web"), None, &Actor::Me)
        .unwrap();
    let t = s.add_task(NewTask::new("Fix typo"), &Actor::Me).unwrap();
    let edit = TaskEdit::default()
        .title("  Fix the typo  ")
        .body(Some("in the footer"))
        .repo(Some(repo.id));
    let edited = s.edit_task(t.id, edit, &Actor::Me).unwrap();
    assert_eq!(edited.title, "Fix the typo");
    assert_eq!(edited.body.as_deref(), Some("in the footer"));
    assert_eq!(edited.repo, Some(repo.id));
    assert_eq!(s.task(t.id).unwrap(), edited);

    let history = s.history(t.id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].kind, EventKind::Edited);
    assert_eq!(history[1].actor, Actor::Me);
}

#[test]
fn editing_can_clear_body_and_repo() {
    let mut s = store();
    let repo = s
        .add_repo("web", Path::new("/w/web"), None, &Actor::Me)
        .unwrap();
    let t = s
        .add_task(
            NewTask::new("x").with_body("notes").in_repo(repo.id),
            &Actor::Me,
        )
        .unwrap();
    let edited = s
        .edit_task(t.id, TaskEdit::default().body(None).repo(None), &Actor::Me)
        .unwrap();
    assert_eq!((edited.body, edited.repo), (None, None));
}

#[test]
fn an_edit_that_changes_nothing_writes_no_event() {
    let mut s = store();
    let t = s.add_task(NewTask::new("Same"), &Actor::Me).unwrap();
    s.edit_task(t.id, TaskEdit::default().title("Same"), &Actor::Me)
        .unwrap();
    s.edit_task(t.id, TaskEdit::default(), &Actor::Me).unwrap();
    assert_eq!(s.history(t.id).unwrap().len(), 1);
}

#[test]
fn editing_to_a_blank_title_is_rejected() {
    let mut s = store();
    let t = s.add_task(NewTask::new("Keep"), &Actor::Me).unwrap();
    let err = s
        .edit_task(t.id, TaskEdit::default().title("  "), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::EmptyTitle), "{err:?}");
    assert_eq!(s.task(t.id).unwrap().title, "Keep");
}

#[test]
fn editing_a_missing_task_or_to_a_missing_repo_is_not_found() {
    let mut s = store();
    let err = s
        .edit_task(TaskId(9), TaskEdit::default().title("x"), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    let err = s
        .edit_task(t.id, TaskEdit::default().repo(Some(RepoId(4))), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

#[test]
fn only_me_edits_tasks() {
    let mut s = store();
    let t = s.add_task(NewTask::new("Mine"), &Actor::Me).unwrap();
    for actor in [
        agent(),
        Actor::Plan,
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ] {
        let err = s
            .edit_task(t.id, TaskEdit::default().title("Theirs"), &actor)
            .unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }
    assert_eq!(s.task(t.id).unwrap().title, "Mine");
    assert_eq!(s.history(t.id).unwrap().len(), 1);
}

// Rule 8, `add_note`: every actor adds notes (as `permissions.rs` has it). Rule 4: a note has text.

#[test]
fn every_actor_adds_a_note_event_with_its_text() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    let actors = [
        Actor::Me,
        agent(),
        Actor::Plan,
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ];
    for actor in &actors {
        s.add_note(t.id, &format!("from {actor}"), actor).unwrap();
    }
    let notes: Vec<_> = s
        .history(t.id)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == EventKind::Note)
        .map(|e| (e.actor, e.body))
        .collect();
    let expected: Vec<_> = actors
        .iter()
        .map(|a| (a.clone(), Some(format!("from {a}"))))
        .collect();
    assert_eq!(notes, expected);
}

#[test]
fn a_blank_note_is_rejected() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    let err = s.add_note(t.id, "  ", &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::NoteRequired), "{err:?}");
    assert_eq!(s.history(t.id).unwrap().len(), 1);
}

#[test]
fn a_note_on_a_missing_task_is_not_found() {
    let mut s = store();
    let err = s.add_note(TaskId(3), "hi", &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

// Rule 8, `unlink` and `set_pr_state`: one test per "no".

#[test]
fn plan_sync_and_launch_cannot_unlink() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    for actor in [Actor::Plan, Actor::SyncGh, Actor::SyncT3, Actor::Launch] {
        let link = s
            .link(
                t.id,
                NewLink::new(LinkKind::Doc, format!("{actor}.md")),
                &actor,
            )
            .unwrap();
        let err = s.unlink(link.id, &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }
    assert_eq!(s.links(t.id).unwrap().len(), 4);
}

#[test]
fn an_agent_cannot_unlink_a_link_it_did_not_add() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    let link = s.link(t.id, NewLink::pr("acme/web#1"), &Actor::Me).unwrap();
    let err = s.unlink(link.id, &agent()).unwrap_err();
    assert!(matches!(err, Error::NotAllowed), "{err:?}");
}

#[test]
fn only_me_and_sync_gh_set_pr_state() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    s.link(t.id, NewLink::pr("acme/web#1"), &Actor::Me).unwrap();
    for actor in [agent(), Actor::Plan, Actor::SyncT3, Actor::Launch] {
        let err = s
            .set_pr_state("acme/web#1", PrState::Merged, &actor)
            .unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }
    assert_eq!(s.links(t.id).unwrap()[0].pr_state, Some(PrState::Open));
    s.set_pr_state("acme/web#1", PrState::Merged, &Actor::Me)
        .unwrap();
    assert_eq!(s.links(t.id).unwrap()[0].pr_state, Some(PrState::Merged));
}

// Rule 6, proposals: only `me` accepts (-> todo) or rejects (-> dropped) them. Rejected
// proposals are hidden from the tree but stay in history.

fn titles(s: &Store) -> Vec<String> {
    s.tree()
        .unwrap()
        .into_iter()
        .map(|n| n.task.title)
        .collect()
}

#[test]
fn proposed_lists_every_proposal_in_tree_order() {
    let mut s = store();
    let parent = s.add_task(NewTask::new("Parent"), &Actor::Me).unwrap();
    let top = s.add_task(NewTask::new("Top idea"), &agent()).unwrap();
    let nested = s
        .add_task(NewTask::new("Nested idea").under(parent.id), &agent())
        .unwrap();
    s.add_task(NewTask::new("Mine"), &Actor::Me).unwrap();
    let ids: Vec<_> = s.proposed().unwrap().into_iter().map(|t| t.id).collect();
    assert_eq!(ids, vec![nested.id, top.id]);
}

#[test]
fn me_accepts_a_proposal_by_moving_it_to_todo() {
    let mut s = store();
    let p = s.add_task(NewTask::new("Idea"), &agent()).unwrap();
    let t = s.set_status(p.id, Status::Todo, None, &Actor::Me).unwrap();
    assert_eq!(t.status, Status::Todo);
    assert!(s.proposed().unwrap().is_empty());
    assert_eq!(titles(&s), vec!["Idea"]);
}

#[test]
fn a_rejected_proposal_is_gone_from_the_tree_but_still_in_history() {
    let mut s = store();
    let kept = s.add_task(NewTask::new("Kept"), &Actor::Me).unwrap();
    let p = s.add_task(NewTask::new("Bad idea"), &agent()).unwrap();
    s.set_status(p.id, Status::Dropped, Some("out of scope"), &Actor::Me)
        .unwrap();

    assert_eq!(titles(&s), vec!["Kept"]);
    assert!(s.proposed().unwrap().is_empty());
    assert_eq!(s.task(p.id).unwrap().status, Status::Dropped);
    let last = s.history(p.id).unwrap().pop().unwrap();
    assert_eq!(
        (last.kind, last.to_status, last.body.as_deref(), last.actor),
        (
            EventKind::Status,
            Some(Status::Dropped),
            Some("out of scope"),
            Actor::Me
        )
    );

    // A task of mine that I drop stays visible; only rejected proposals are hidden.
    s.set_status(kept.id, Status::Dropped, None, &Actor::Me)
        .unwrap();
    assert_eq!(titles(&s), vec!["Kept"]);
}

#[test]
fn rejecting_needs_no_reason() {
    let mut s = store();
    let p = s.add_task(NewTask::new("Idea"), &agent()).unwrap();
    s.set_status(p.id, Status::Dropped, None, &Actor::Me)
        .unwrap();
    assert!(s.history(p.id).unwrap().pop().unwrap().body.is_none());
    assert!(titles(&s).is_empty());
}

#[test]
fn an_agent_cannot_move_its_own_proposal_anywhere() {
    let mut s = store();
    let p = s.add_task(NewTask::new("Idea"), &agent()).unwrap();
    for to in [
        Status::Proposed,
        Status::Todo,
        Status::Doing,
        Status::InReview,
        Status::Blocked,
        Status::Done,
        Status::Dropped,
    ] {
        let err = s
            .set_status(p.id, to, Some("please"), &agent())
            .unwrap_err();
        assert!(
            matches!(err, Error::NotAllowed | Error::InvalidTransition),
            "{to}: {err:?}"
        );
    }
    assert_eq!(s.task(p.id).unwrap().status, Status::Proposed);
    assert_eq!(s.history(p.id).unwrap().len(), 1);
}
