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
    assert_eq!(link.title, None, "no title: the app shows the ref");
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

#[test]
fn a_link_keeps_its_title() {
    let mut s = store();
    let t = task(&mut s, "Booking");
    let link = s
        .link(
            t,
            NewLink::new(LinkKind::Slack, "C01/p123").with_title("#booking thread"),
            &Actor::Me,
        )
        .unwrap();
    assert_eq!(link.title.as_deref(), Some("#booking thread"));
    assert_eq!(s.links(t).unwrap(), vec![link]);
}

#[test]
fn a_blank_link_title_is_no_title() {
    let mut s = store();
    let t = task(&mut s, "Booking");
    let link = s
        .link(
            t,
            NewLink::new(LinkKind::Doc, "booking-flow.md").with_title("  "),
            &Actor::Me,
        )
        .unwrap();
    assert_eq!(link.title, None);
}

#[test]
fn linking_again_with_another_title_returns_the_link_as_it_was() {
    let mut s = store();
    let t = task(&mut s, "Booking");
    let first = s
        .link(
            t,
            NewLink::new(LinkKind::Doc, "flow.md").with_title("Flow"),
            &Actor::Me,
        )
        .unwrap();
    let again = s
        .link(
            t,
            NewLink::new(LinkKind::Doc, "flow.md").with_title("Other"),
            &Actor::Me,
        )
        .unwrap();
    assert_eq!(again, first);
}

#[test]
fn links_of_kind_lists_every_tasks_links_of_that_kind_by_id() {
    let mut s = store();
    let a = task(&mut s, "A");
    let b = task(&mut s, "B");
    let pr1 = s.link(b, NewLink::pr("acme/web#1"), &Actor::Me).unwrap();
    s.link(a, NewLink::new(LinkKind::T3Thread, "t3/1"), &Actor::Me)
        .unwrap();
    let pr2 = s.link(a, NewLink::pr("acme/web#2"), &Actor::Me).unwrap();
    let pr3 = s.link(b, NewLink::pr("acme/api#3"), &Actor::Me).unwrap();

    assert_eq!(s.links_of_kind(LinkKind::Pr).unwrap(), vec![pr1, pr2, pr3]);
    let threads = s.links_of_kind(LinkKind::T3Thread).unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].task, a);
    assert!(s.links_of_kind(LinkKind::Slack).unwrap().is_empty());
}

// Rule 29: a PR has one ref, lowercase `owner/repo#n`, whichever form it came in.

#[test]
fn a_pr_ref_in_mixed_case_is_stored_in_lowercase() {
    let mut s = store();
    let t = task(&mut s, "Booking");
    let link = s.link(t, NewLink::pr("Acme/Web#42"), &Actor::Me).unwrap();
    assert_eq!(link.r#ref, "acme/web#42");
    let last = s.history(t).unwrap().pop().unwrap();
    assert_eq!(last.body.as_deref(), Some("pr acme/web#42"));
}

#[test]
fn a_pr_url_is_stored_as_its_ref() {
    for url in [
        "https://github.com/Acme/Web/pull/12",
        "https://github.com/acme/web/pull/12/",
        "https://github.com/acme/web/pull/12/files",
        "https://github.com/acme/web/pull/12/commits/4f2a9c1?w=1",
        "https://github.com/acme/web/pull/12#discussion_r123",
        "HTTPS://GitHub.com/ACME/web/pull/12?tab=checks#top",
        "  https://github.com/acme/web/pull/12  ",
        "acme/web#012",
    ] {
        let mut s = store();
        let t = task(&mut s, "Booking");
        let link = s.link(t, NewLink::pr(url), &Actor::Me).unwrap();
        assert_eq!(link.r#ref, "acme/web#12", "{url}");
    }
}

#[test]
fn the_same_pr_in_two_forms_is_one_link() {
    let mut s = store();
    let t = task(&mut s, "Booking");
    let first = s
        .link(
            t,
            NewLink::pr("https://github.com/Acme/Web/pull/12/files"),
            &Actor::Me,
        )
        .unwrap();
    let again = s.link(t, NewLink::pr("ACME/web#12"), &Actor::Me).unwrap();
    assert_eq!(again, first);
    assert_eq!(s.links(t).unwrap().len(), 1);
    assert_eq!(kinds(&s, t), vec![EventKind::Created, EventKind::Linked]);
}

#[test]
fn set_pr_state_and_tasks_for_link_accept_either_form() {
    let mut s = store();
    let t = task(&mut s, "Booking");
    s.link(t, NewLink::pr("acme/web#12"), &Actor::Me).unwrap();
    assert_eq!(
        s.tasks_for_link(LinkKind::Pr, "https://github.com/ACME/Web/pull/12")
            .unwrap(),
        vec![t]
    );
    assert_eq!(
        s.tasks_for_link(LinkKind::Pr, "Acme/Web#12").unwrap(),
        vec![t]
    );
    let changed = s
        .set_pr_state(
            "https://github.com/Acme/web/pull/12?x=1",
            PrState::Merged,
            &Actor::SyncGh,
        )
        .unwrap();
    assert_eq!(changed, vec![t]);
    assert_eq!(s.links(t).unwrap()[0].pr_state, Some(PrState::Merged));
    let last = s.history(t).unwrap().pop().unwrap();
    assert_eq!(last.body.as_deref(), Some("acme/web#12 merged"));
}

const MALFORMED_PR_REFS: &[&str] = &[
    "",
    "   ",
    "acme/web",
    "acme#12",
    "acme/web#",
    "acme/web#0",
    "acme/web#-3",
    "acme/web#12a",
    "acme/web#1 2",
    "acme/web#99999999999999999999999",
    "acme web/site#1",
    "acme/we@b#1",
    "acme/web/extra#1",
    "/web#1",
    "acme/#1",
    "../web#1",
    "acme/..#1",
    "acme/.#1",
    "https://github.com/acme/web/issues/12",
    "https://github.com/acme/web/pull/",
    "https://github.com/acme/web/pull/x",
    "https://github.com/acme/web",
    "https://github.com/acme/web/pull/12abc",
    "https://gitlab.com/acme/web/pull/12",
    "http://github.com/acme/web/pull/12",
    "https://github.com.evil.example/acme/web/pull/12",
    "github.com/acme/web/pull/12",
];

#[test]
fn a_malformed_pr_ref_is_refused_and_nothing_is_written() {
    let mut s = store();
    let t = task(&mut s, "Booking");
    for bad in MALFORMED_PR_REFS {
        let err = s.link(t, NewLink::pr(*bad), &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{bad:?}: {err:?}");
        let err = s
            .set_pr_state(bad, PrState::Merged, &Actor::SyncGh)
            .unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{bad:?}: {err:?}");
        let err = s.tasks_for_link(LinkKind::Pr, bad).unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{bad:?}: {err:?}");
    }
    assert!(s.links(t).unwrap().is_empty());
    assert_eq!(kinds(&s, t), vec![EventKind::Created]);
}

#[test]
fn a_refused_actor_is_told_so_before_its_ref_is_checked() {
    let mut s = store();
    let err = s
        .set_pr_state("not a ref", PrState::Merged, &agent("t3/a"))
        .unwrap_err();
    assert!(matches!(err, Error::NotAllowed), "{err:?}");
}

#[test]
fn other_link_kinds_keep_their_ref_as_given() {
    let mut s = store();
    let t = task(&mut s, "Booking");
    let issue = s
        .link(
            t,
            NewLink::new(LinkKind::GithubIssue, "Acme/Web#7"),
            &Actor::Me,
        )
        .unwrap();
    let doc = s
        .link(
            t,
            NewLink::new(LinkKind::Doc, "Docs/Booking-Flow.md"),
            &Actor::Me,
        )
        .unwrap();
    assert_eq!(issue.r#ref, "Acme/Web#7");
    assert_eq!(doc.r#ref, "Docs/Booking-Flow.md");
    assert_eq!(
        s.tasks_for_link(LinkKind::Doc, "Docs/Booking-Flow.md")
            .unwrap(),
        vec![t]
    );
}

#[test]
fn a_herdr_terminal_is_linked_by_its_address() {
    let mut s = store();
    let t = task(&mut s, "Fix typo");
    let by_launch = s
        .link(
            t,
            NewLink::new(LinkKind::HerdrTerminal, "t4p"),
            &Actor::Launch,
        )
        .unwrap();
    assert_eq!(by_launch.kind, LinkKind::HerdrTerminal);
    assert_eq!(by_launch.r#ref, "t4p");
    // The agent in that terminal linking "this thread" finds the same link, not a second one.
    let by_agent = s
        .link(
            t,
            NewLink::new(LinkKind::HerdrTerminal, "t4p"),
            &agent("herdr/t4p"),
        )
        .unwrap();
    assert_eq!(by_agent, by_launch);
    assert_eq!(s.links(t).unwrap().len(), 1);
    // A terminal on another machine.
    s.link(
        t,
        NewLink::new(LinkKind::HerdrTerminal, "vps/t7"),
        &Actor::Me,
    )
    .unwrap();
    for bad in ["", "t 4p", "t4p\n", "tä", &"t".repeat(65)] {
        assert!(
            matches!(
                s.link(t, NewLink::new(LinkKind::HerdrTerminal, bad), &Actor::Me),
                Err(Error::InvalidTarget)
            ),
            "{bad:?}"
        );
    }
}
