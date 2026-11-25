//! One test per SPEC-core rule, named after the behaviour.
#![allow(clippy::unwrap_used)]

use std::path::Path;

mod common;

use airboss_core::{
    Actor, Error, EventKind, LinkKind, NewLink, NewTask, Origin, PrState, RepoId, Status, Store,
    TaskEdit, TaskId,
};
use common::{agent, store};

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

// Rule 8, `edit_task`: only `me` edits title and notes; one `edited` event.

#[test]
fn me_edits_title_and_body_with_one_edited_event() {
    let mut s = store();
    let t = s.add_task(NewTask::new("Fix typo"), &Actor::Me).unwrap();
    let edit = TaskEdit::default()
        .title("  Fix the typo  ")
        .body(Some("in the footer"));
    let edited = s.edit_task(t.id, edit, &Actor::Me).unwrap();
    assert_eq!(edited.title, "Fix the typo");
    assert_eq!(edited.body.as_deref(), Some("in the footer"));
    assert_eq!(s.task(t.id).unwrap(), edited);

    let history = s.history(t.id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].kind, EventKind::Edited);
    assert_eq!(history[1].body.as_deref(), Some("title, body"));
    assert_eq!(history[1].actor, Actor::Me);
}

#[test]
fn editing_can_clear_the_body() {
    let mut s = store();
    let t = s
        .add_task(NewTask::new("x").with_body("notes"), &Actor::Me)
        .unwrap();
    let edited = s
        .edit_task(t.id, TaskEdit::default().body(None), &Actor::Me)
        .unwrap();
    assert_eq!(edited.body, None);
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
fn editing_a_missing_task_is_not_found() {
    let mut s = store();
    let err = s
        .edit_task(TaskId(9), TaskEdit::default().title("x"), &Actor::Me)
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

// Rule 8, `add_note`: every actor except `plan` adds notes. Rule 4: a note has text.

#[test]
fn plan_cannot_add_notes() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    let err = s.add_note(t.id, "from the plan", &Actor::Plan).unwrap_err();
    assert!(matches!(err, Error::NotAllowed), "{err:?}");
}

#[test]
fn every_other_actor_adds_a_note_event_with_its_text() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    let actors = [
        Actor::Me,
        agent(),
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
    // A rejection is its own kind of event, so nothing depends on the wording of a note.
    let last = s.history(p.id).unwrap().pop().unwrap();
    assert_eq!(
        (last.kind, last.to_status, last.body.as_deref(), last.actor),
        (EventKind::Rejected, None, Some("out of scope"), Actor::Me)
    );

    // A task of mine that I drop stays visible; only rejected proposals are hidden.
    s.set_status(kept.id, Status::Dropped, None, &Actor::Me)
        .unwrap();
    assert_eq!(titles(&s), vec!["Kept"]);
}

#[test]
fn an_accepted_proposal_that_i_later_drop_stays_in_the_tree() {
    let mut s = store();
    let p = s.add_task(NewTask::new("Good idea"), &agent()).unwrap();
    s.set_status(p.id, Status::Todo, None, &Actor::Me).unwrap();
    s.add_task(NewTask::new("Step").under(p.id), &Actor::Me)
        .unwrap();
    s.set_status(p.id, Status::Dropped, None, &Actor::Me)
        .unwrap();

    assert_eq!(titles(&s), vec!["Good idea", "Step"]);
    assert!(s.proposed().unwrap().is_empty());
}

#[test]
fn an_open_proposal_dropped_with_its_parent_stays_in_the_tree() {
    let mut s = store();
    let parent = s.add_task(NewTask::new("Parent"), &Actor::Me).unwrap();
    s.add_task(NewTask::new("Idea").under(parent.id), &agent())
        .unwrap();
    s.set_status(parent.id, Status::Dropped, None, &Actor::Me)
        .unwrap();

    assert_eq!(titles(&s), vec!["Parent", "Idea"]);
    assert!(s.proposed().unwrap().is_empty());
    assert!(s.ready().unwrap().is_empty());
}

#[test]
fn a_rejected_proposal_i_reopen_comes_back_into_the_tree() {
    let mut s = store();
    let p = s
        .add_task(NewTask::new("Second thoughts"), &agent())
        .unwrap();
    s.set_status(p.id, Status::Dropped, None, &Actor::Me)
        .unwrap();
    assert!(titles(&s).is_empty());

    s.set_status(p.id, Status::Todo, None, &Actor::Me).unwrap();
    assert_eq!(titles(&s), vec!["Second thoughts"]);
}

#[test]
fn rejecting_writes_exactly_one_event() {
    let mut s = store();
    let p = s.add_task(NewTask::new("Idea"), &agent()).unwrap();
    s.set_status(p.id, Status::Dropped, Some("no"), &Actor::Me)
        .unwrap();
    let kinds: Vec<_> = s
        .history(p.id)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, vec![EventKind::Created, EventKind::Rejected]);
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

// Rule 24: a task links to any number of repos; only `me` and `plan` change them.

fn three_repos(s: &mut Store) -> [RepoId; 3] {
    ["web", "api", "ios"].map(|name| {
        s.add_repo(name, Path::new(&format!("/w/{name}")), None, &Actor::Me)
            .unwrap()
            .id
    })
}

#[test]
fn a_task_links_to_several_repos_listed_by_name() {
    let mut s = store();
    let [web, api, ios] = three_repos(&mut s);
    let t = s
        .add_task(
            NewTask::new("Booking")
                .in_repo(web)
                .in_repo(ios)
                .in_repo(api),
            &Actor::Me,
        )
        .unwrap();
    assert_eq!(t.repos, vec![api, ios, web]);
    assert_eq!(s.task(t.id).unwrap().repos, vec![api, ios, web]);
    assert_eq!(s.tree().unwrap()[0].task.repos, vec![api, ios, web]);
    assert_eq!(s.history(t.id).unwrap().len(), 1, "one created event");
}

#[test]
fn the_same_repo_given_twice_is_linked_once() {
    let mut s = store();
    let [web, ..] = three_repos(&mut s);
    let t = s
        .add_task(NewTask::new("x").in_repo(web).in_repo(web), &Actor::Me)
        .unwrap();
    assert_eq!(t.repos, vec![web]);
}

#[test]
fn link_repo_adds_a_repo_with_one_repo_linked_event() {
    let mut s = store();
    let [web, api, _] = three_repos(&mut s);
    let t = s
        .add_task(NewTask::new("x").in_repo(web), &Actor::Me)
        .unwrap();
    s.link_repo(t.id, api, &Actor::Me).unwrap();
    assert_eq!(s.task(t.id).unwrap().repos, vec![api, web]);
    let last = s.history(t.id).unwrap().pop().unwrap();
    assert_eq!(last.kind, EventKind::RepoLinked);
    assert_eq!(last.body.as_deref(), Some("api"));
    assert_eq!(last.actor, Actor::Me);
}

#[test]
fn linking_a_repo_twice_changes_nothing() {
    let mut s = store();
    let [web, ..] = three_repos(&mut s);
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    s.link_repo(t.id, web, &Actor::Me).unwrap();
    let rx = s.subscribe();
    s.link_repo(t.id, web, &Actor::Me).unwrap();
    assert_eq!(s.history(t.id).unwrap().len(), 2);
    assert!(rx.try_recv().is_err());
}

#[test]
fn unlink_repo_removes_it_with_one_repo_unlinked_event() {
    let mut s = store();
    let [web, api, _] = three_repos(&mut s);
    let t = s
        .add_task(NewTask::new("x").in_repo(web).in_repo(api), &Actor::Me)
        .unwrap();
    s.unlink_repo(t.id, web, &Actor::Plan).unwrap();
    assert_eq!(s.task(t.id).unwrap().repos, vec![api]);
    let last = s.history(t.id).unwrap().pop().unwrap();
    assert_eq!(last.kind, EventKind::RepoUnlinked);
    assert_eq!(last.body.as_deref(), Some("web"));
    assert_eq!(last.actor, Actor::Plan);
}

#[test]
fn unlinking_a_repo_that_is_not_linked_is_not_found() {
    let mut s = store();
    let [web, ..] = three_repos(&mut s);
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    let err = s.unlink_repo(t.id, web, &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
    assert_eq!(s.history(t.id).unwrap().len(), 1);
}

#[test]
fn linking_a_missing_task_or_repo_is_not_found() {
    let mut s = store();
    let [web, ..] = three_repos(&mut s);
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    for (task, repo) in [(TaskId(99), web), (t.id, RepoId(99))] {
        let err = s.link_repo(task, repo, &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::NotFound), "{err:?}");
        let err = s.unlink_repo(task, repo, &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::NotFound), "{err:?}");
    }
}

#[test]
fn agents_sync_and_launch_cannot_change_repos() {
    let mut s = store();
    let [web, api, _] = three_repos(&mut s);
    let t = s
        .add_task(NewTask::new("x").in_repo(web), &Actor::Me)
        .unwrap();
    for actor in [agent(), Actor::SyncGh, Actor::SyncT3, Actor::Launch] {
        let err = s.link_repo(t.id, api, &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
        let err = s.unlink_repo(t.id, web, &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }
    assert_eq!(s.task(t.id).unwrap().repos, vec![web]);
    assert_eq!(s.history(t.id).unwrap().len(), 1);
}

#[test]
fn a_parent_in_the_tree_shows_only_its_own_repos() {
    let mut s = store();
    let [web, api, _] = three_repos(&mut s);
    let p = s
        .add_task(NewTask::new("Parent").in_repo(web), &Actor::Me)
        .unwrap();
    s.add_task(NewTask::new("Child").under(p.id).in_repo(api), &Actor::Me)
        .unwrap();
    let repos: Vec<_> = s
        .tree()
        .unwrap()
        .into_iter()
        .map(|n| n.task.repos)
        .collect();
    assert_eq!(repos, vec![vec![web], vec![api]]);
}

// Rule 33: `me` edits and removes repos; removing one unlinks it from its tasks first.

fn repo(s: &Store, id: RepoId) -> Option<airboss_core::Repo> {
    s.repos().unwrap().into_iter().find(|r| r.id == id)
}

#[test]
fn edit_repo_renames_moves_and_changes_the_remote() {
    let mut s = store();
    let [web, ..] = three_repos(&mut s);
    let t = s
        .add_task(NewTask::new("x").in_repo(web), &Actor::Me)
        .unwrap();
    let rx = s.subscribe();
    let edited = s
        .edit_repo(
            web,
            Some(" site "),
            Some(Path::new("/new/site")),
            Some(Some("git@x:site.git")),
            &Actor::Me,
        )
        .unwrap();
    assert_eq!(edited.name, "site", "trimmed");
    assert_eq!(edited.path, "/new/site");
    assert_eq!(edited.remote_url.as_deref(), Some("git@x:site.git"));
    assert_eq!(repo(&s, web), Some(edited));
    assert_eq!(s.task(t.id).unwrap().repos, vec![web], "tasks keep it");
    assert_eq!(s.history(t.id).unwrap().len(), 1, "repos have no history");
    assert!(rx.try_recv().is_err(), "no events, so no notification");

    let cleared = s
        .edit_repo(web, None, None, Some(None), &Actor::Me)
        .unwrap();
    assert_eq!(
        (cleared.name.as_str(), cleared.remote_url),
        ("site", None),
        "left out: kept; Some(None): cleared"
    );
}

#[test]
fn edit_repo_refuses_a_taken_name_or_path_a_blank_one_or_a_missing_repo() {
    let mut s = store();
    let [web, api, _] = three_repos(&mut s);
    let before = s.repos().unwrap();
    let err = s
        .edit_repo(web, Some("api"), None, None, &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::AlreadyExists), "{err:?}");
    let err = s
        .edit_repo(web, None, Some(Path::new("/w/api")), None, &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::AlreadyExists), "{err:?}");
    for (name, path) in [(Some("  "), None), (None, Some(Path::new("")))] {
        let err = s.edit_repo(web, name, path, None, &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{err:?}");
    }
    let err = s
        .edit_repo(RepoId(99), Some("x"), None, None, &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
    // Its own name and path again is fine, and changes nothing.
    s.edit_repo(
        api,
        Some("api"),
        Some(Path::new("/w/api")),
        None,
        &Actor::Me,
    )
    .unwrap();
    assert_eq!(s.repos().unwrap(), before);
}

#[test]
fn remove_repo_unlinks_it_from_every_task_with_an_event_each_then_deletes_it() {
    let mut s = store();
    let [web, api, ios] = three_repos(&mut s);
    let a = s
        .add_task(NewTask::new("a").in_repo(web).in_repo(api), &Actor::Me)
        .unwrap();
    let b = s
        .add_task(NewTask::new("b").in_repo(web), &Actor::Me)
        .unwrap();
    s.set_status(b.id, Status::Done, None, &Actor::Me).unwrap();
    let other = s
        .add_task(NewTask::new("c").in_repo(api), &Actor::Me)
        .unwrap();
    let rx = s.subscribe();

    let unlinked = s.remove_repo(web, &Actor::Me).unwrap();
    assert_eq!(unlinked, vec![a.id, b.id], "done tasks too, by id");
    assert_eq!(repo(&s, web), None);
    assert_eq!(s.task(a.id).unwrap().repos, vec![api]);
    assert!(s.task(b.id).unwrap().repos.is_empty());
    for t in [a.id, b.id] {
        let last = s.history(t).unwrap().pop().unwrap();
        assert_eq!(last.kind, EventKind::RepoUnlinked);
        assert_eq!(last.body.as_deref(), Some("web"));
        assert_eq!(last.actor, Actor::Me);
    }
    assert_eq!(s.history(other.id).unwrap().len(), 1, "untouched");
    assert_eq!(
        rx.try_recv().unwrap().len(),
        2,
        "one notification, both events"
    );

    // A repo no task uses goes with no event at all.
    assert_eq!(s.remove_repo(ios, &Actor::Me).unwrap(), vec![]);
    let err = s.remove_repo(ios, &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
    // Its name and path are free again.
    s.add_repo("web", Path::new("/w/web"), None, &Actor::Me)
        .unwrap();
}

#[test]
fn only_me_edits_or_removes_repos() {
    let mut s = store();
    let [web, ..] = three_repos(&mut s);
    let t = s
        .add_task(NewTask::new("x").in_repo(web), &Actor::Me)
        .unwrap();
    for actor in [
        agent(),
        Actor::Plan,
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ] {
        let err = s.edit_repo(web, Some("y"), None, None, &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
        let err = s.remove_repo(web, &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }
    assert_eq!(repo(&s, web).unwrap().name, "web");
    assert_eq!(s.task(t.id).unwrap().repos, vec![web]);
}

// Rule 25: one branch per task, set by `me` or `launch`, with an `edited` event naming it.

fn last_event(s: &Store, id: TaskId) -> airboss_core::Event {
    s.history(id).unwrap().pop().unwrap()
}

#[test]
fn a_new_task_has_no_branch() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    assert_eq!(t.branch, None);
}

#[test]
fn a_branch_is_set_changed_and_cleared_with_one_edited_event_each() {
    let mut s = store();
    let t = s.add_task(NewTask::new("Booking"), &Actor::Me).unwrap();

    let set = s
        .set_branch(t.id, Some("booking-flow"), &Actor::Launch)
        .unwrap();
    assert_eq!(set.branch.as_deref(), Some("booking-flow"));
    assert_eq!(s.task(t.id).unwrap(), set);
    let e = last_event(&s, t.id);
    assert_eq!(e.kind, EventKind::Edited);
    assert_eq!(e.body.as_deref(), Some("branch: booking-flow"));
    assert_eq!(e.actor, Actor::Launch);

    s.set_branch(t.id, Some("  booking-v2 "), &Actor::Me)
        .unwrap();
    assert_eq!(s.task(t.id).unwrap().branch.as_deref(), Some("booking-v2"));
    assert_eq!(
        last_event(&s, t.id).body.as_deref(),
        Some("branch: booking-v2")
    );

    let cleared = s.set_branch(t.id, None, &Actor::Me).unwrap();
    assert_eq!(cleared.branch, None);
    assert_eq!(last_event(&s, t.id).body.as_deref(), Some("branch cleared"));
    assert_eq!(s.history(t.id).unwrap().len(), 4);
}

#[test]
fn setting_the_branch_a_task_already_has_writes_nothing() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    s.set_branch(t.id, None, &Actor::Me).unwrap();
    s.set_branch(t.id, Some("b"), &Actor::Me).unwrap();
    let rx = s.subscribe();
    s.set_branch(t.id, Some("b"), &Actor::Launch).unwrap();
    assert_eq!(s.history(t.id).unwrap().len(), 2);
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_blank_branch_clears_it() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    s.set_branch(t.id, Some("b"), &Actor::Me).unwrap();
    let cleared = s.set_branch(t.id, Some("   "), &Actor::Me).unwrap();
    assert_eq!(cleared.branch, None);
}

#[test]
fn agents_plan_and_sync_cannot_set_a_branch() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    for actor in [agent(), Actor::Plan, Actor::SyncGh, Actor::SyncT3] {
        let err = s.set_branch(t.id, Some("b"), &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }
    assert_eq!(s.task(t.id).unwrap().branch, None);
    assert_eq!(s.history(t.id).unwrap().len(), 1);
}

#[test]
fn setting_the_branch_of_a_missing_task_is_not_found() {
    let mut s = store();
    let err = s.set_branch(TaskId(4), Some("b"), &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

// Rule 25: a branch name `git` and `t3` can't misread.

#[test]
fn branch_names_git_accepts_are_kept() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    let longest = "b".repeat(200);
    for name in [
        "booking-flow",
        "feature/booking_v2.1",
        "Release/2026.11",
        "a",
        "x.locked",
        longest.as_str(),
    ] {
        let got = s.set_branch(t.id, Some(name), &Actor::Launch).unwrap();
        assert_eq!(got.branch.as_deref(), Some(name));
    }
}

#[test]
fn branch_names_git_could_misread_are_refused_and_nothing_is_written() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    s.set_branch(t.id, Some("keep"), &Actor::Me).unwrap();
    let too_long = "b".repeat(201);
    for bad in [
        "has space",
        "tab\there",
        "a..b",
        "..",
        "-x",
        "--force",
        "/x",
        "x/",
        "x.lock",
        "feature/x.lock",
        "ünïcode",
        "a~b",
        "a^b",
        "a:b",
        "a@{1}",
        "a*b",
        "a?b",
        "a[b",
        "a\\b",
        "new\nline",
        too_long.as_str(),
    ] {
        let err = s.set_branch(t.id, Some(bad), &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{bad:?}: {err:?}");
    }
    assert_eq!(s.task(t.id).unwrap().branch.as_deref(), Some("keep"));
    assert_eq!(s.history(t.id).unwrap().len(), 2);
}

#[test]
fn a_refused_actor_is_told_so_before_its_branch_name_is_checked() {
    let mut s = store();
    let t = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
    let err = s.set_branch(t.id, Some("--force"), &agent()).unwrap_err();
    assert!(matches!(err, Error::NotAllowed), "{err:?}");
}
