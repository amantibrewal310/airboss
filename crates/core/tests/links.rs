//! Links to outside things, and PR state.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, Error, EventKind, LinkKind, NewLink, NewTask, PrState, Store, TaskId};
use common::store;

fn agent(r: &str) -> Actor {
    Actor::Agent(r.into())
}

fn task(s: &mut Store, title: &str) -> TaskId {
    s.add_task(NewTask::new(title), &Actor::Me).unwrap().id
}

fn kinds(s: &Store, id: TaskId) -> Vec<EventKind> {
    s.history(id).unwrap().into_iter().map(|e| e.kind).collect()
}

#[test]
fn linking_writes_the_link_and_one_linked_event() {
    let mut s = store();
    let t = task(&mut s, "Fix typo");
    let link = s
        .link(
            t,
            NewLink::new(LinkKind::T3Thread, "t3/97b5").with_url("https://t3/97b5"),
            &agent("t3/97b5"),
        )
        .unwrap();
    assert_eq!(link.task, t);
    assert_eq!(link.kind, LinkKind::T3Thread);
    assert_eq!(link.r#ref, "t3/97b5");
    assert_eq!(link.url.as_deref(), Some("https://t3/97b5"));
    assert_eq!(link.pr_state, None);
    assert_eq!(link.linked_by, agent("t3/97b5"));
    assert_eq!(s.links(t).unwrap(), vec![link]);
    assert_eq!(kinds(&s, t), vec![EventKind::Created, EventKind::Linked]);
}

#[test]
fn every_actor_may_link() {
    let mut s = store();
    let t = task(&mut s, "x");
    for (i, actor) in [
        Actor::Me,
        agent("t3/a"),
        Actor::Plan,
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ]
    .iter()
    .enumerate()
    {
        s.link(t, NewLink::new(LinkKind::Doc, format!("doc{i}.md")), actor)
            .unwrap();
    }
    assert_eq!(s.links(t).unwrap().len(), 6);
}

#[test]
fn linking_the_same_thing_twice_returns_the_existing_link_without_an_event() {
    let mut s = store();
    let t = task(&mut s, "x");
    let first = s.link(t, NewLink::pr("acme/web#12"), &Actor::Me).unwrap();
    let again = s
        .link(t, NewLink::pr("acme/web#12"), &agent("t3/a"))
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(s.links(t).unwrap().len(), 1);
    assert_eq!(kinds(&s, t), vec![EventKind::Created, EventKind::Linked]);
}

#[test]
fn the_same_ref_may_link_to_different_tasks_and_kinds() {
    let mut s = store();
    let a = task(&mut s, "a");
    let b = task(&mut s, "b");
    s.link(a, NewLink::pr("acme/web#12"), &Actor::Me).unwrap();
    s.link(b, NewLink::pr("acme/web#12"), &Actor::Me).unwrap();
    s.link(
        a,
        NewLink::new(LinkKind::GithubIssue, "acme/web#12"),
        &Actor::Me,
    )
    .unwrap();
    assert_eq!(s.links(a).unwrap().len(), 2);
    assert_eq!(s.links(b).unwrap().len(), 1);
}

// Rule 3: a PR link always has a state, and no other link has one.

#[test]
fn a_pr_link_defaults_to_open() {
    let mut s = store();
    let t = task(&mut s, "x");
    let link = s.link(t, NewLink::pr("acme/web#1"), &Actor::Me).unwrap();
    assert_eq!(link.pr_state, Some(PrState::Open));
    let merged = s
        .link(
            t,
            NewLink::pr("acme/web#2").with_pr_state(PrState::Merged),
            &Actor::Me,
        )
        .unwrap();
    assert_eq!(merged.pr_state, Some(PrState::Merged));
}

#[test]
fn a_non_pr_link_with_a_pr_state_is_rejected_and_writes_nothing() {
    let mut s = store();
    let t = task(&mut s, "x");
    let err = s
        .link(
            t,
            NewLink::new(LinkKind::Slack, "C1/p2").with_pr_state(PrState::Open),
            &Actor::Me,
        )
        .unwrap_err();
    assert!(matches!(err, Error::Db(_)), "{err:?}");
    assert!(s.links(t).unwrap().is_empty());
    assert_eq!(kinds(&s, t), vec![EventKind::Created]);
}

#[test]
fn an_empty_ref_is_rejected() {
    let mut s = store();
    let t = task(&mut s, "x");
    let err = s
        .link(t, NewLink::new(LinkKind::Doc, ""), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::Db(_)), "{err:?}");
    assert!(s.links(t).unwrap().is_empty());
}

#[test]
fn linking_to_a_missing_task_is_not_found() {
    let mut s = store();
    let err = s
        .link(TaskId(8), NewLink::pr("acme/web#1"), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
    assert!(matches!(s.links(TaskId(8)).unwrap_err(), Error::NotFound));
}

// Unlinking.

#[test]
fn me_unlinks_any_link_with_an_unlinked_event() {
    let mut s = store();
    let t = task(&mut s, "x");
    let link = s
        .link(t, NewLink::pr("acme/web#1"), &agent("t3/a"))
        .unwrap();
    s.unlink(link.id, &Actor::Me).unwrap();
    assert!(s.links(t).unwrap().is_empty());
    assert_eq!(
        kinds(&s, t),
        vec![EventKind::Created, EventKind::Linked, EventKind::Unlinked]
    );
}

#[test]
fn an_agent_unlinks_only_links_it_added() {
    let mut s = store();
    let t = task(&mut s, "x");
    let own = s
        .link(t, NewLink::new(LinkKind::Doc, "a.md"), &agent("t3/a"))
        .unwrap();
    let other_agents = s
        .link(t, NewLink::new(LinkKind::Doc, "b.md"), &agent("t3/b"))
        .unwrap();
    let mine = s
        .link(t, NewLink::new(LinkKind::Doc, "c.md"), &Actor::Me)
        .unwrap();
    for link in [&other_agents, &mine] {
        let err = s.unlink(link.id, &agent("t3/a")).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{err:?}");
    }
    s.unlink(own.id, &agent("t3/a")).unwrap();
    assert_eq!(s.links(t).unwrap(), vec![other_agents, mine]);
}

#[test]
fn unlinking_a_missing_link_is_not_found() {
    let mut s = store();
    let err = s.unlink(airboss_core::LinkId(5), &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

// Finding tasks by link, and PR state.

#[test]
fn tasks_for_link_finds_every_task_with_that_pr() {
    let mut s = store();
    let a = task(&mut s, "a");
    let b = task(&mut s, "b");
    let c = task(&mut s, "c");
    s.link(b, NewLink::pr("acme/web#12"), &Actor::Me).unwrap();
    s.link(a, NewLink::pr("acme/web#12"), &Actor::Me).unwrap();
    s.link(c, NewLink::pr("acme/web#13"), &Actor::Me).unwrap();
    s.link(
        c,
        NewLink::new(LinkKind::GithubIssue, "acme/web#12"),
        &Actor::Me,
    )
    .unwrap();
    assert_eq!(
        s.tasks_for_link(LinkKind::Pr, "acme/web#12").unwrap(),
        vec![a, b]
    );
    assert!(
        s.tasks_for_link(LinkKind::Pr, "acme/web#99")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn set_pr_state_updates_every_task_with_that_pr_and_returns_them() {
    let mut s = store();
    let a = task(&mut s, "a");
    let b = task(&mut s, "b");
    let c = task(&mut s, "c");
    s.link(a, NewLink::pr("acme/web#12"), &Actor::Me).unwrap();
    s.link(b, NewLink::pr("acme/web#12"), &Actor::Me).unwrap();
    s.link(c, NewLink::pr("acme/web#13"), &Actor::Me).unwrap();

    let affected = s
        .set_pr_state("acme/web#12", PrState::Merged, &Actor::SyncGh)
        .unwrap();
    assert_eq!(affected, vec![a, b]);
    for t in [a, b] {
        assert_eq!(s.links(t).unwrap()[0].pr_state, Some(PrState::Merged));
        let last = s.history(t).unwrap().pop().unwrap();
        assert_eq!(last.kind, EventKind::LinkState);
        assert_eq!(last.actor, Actor::SyncGh);
        assert_eq!(last.body.as_deref(), Some("acme/web#12 merged"));
    }
    assert_eq!(s.links(c).unwrap()[0].pr_state, Some(PrState::Open));
    assert_eq!(kinds(&s, c), vec![EventKind::Created, EventKind::Linked]);
}

#[test]
fn setting_the_state_a_pr_already_has_changes_nothing() {
    let mut s = store();
    let a = task(&mut s, "a");
    s.link(a, NewLink::pr("acme/web#12"), &Actor::Me).unwrap();
    let affected = s
        .set_pr_state("acme/web#12", PrState::Open, &Actor::SyncGh)
        .unwrap();
    assert!(affected.is_empty());
    assert_eq!(kinds(&s, a), vec![EventKind::Created, EventKind::Linked]);
    assert!(
        s.set_pr_state("acme/web#404", PrState::Closed, &Actor::Me)
            .unwrap()
            .is_empty()
    );
}
