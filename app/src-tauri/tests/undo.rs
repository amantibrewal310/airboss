//! Undo: every inverse, a dropped subtree, refused undo, deferred rejects and the depth.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use airboss_app::Caller;
use airboss_app::commands::{needs, tasks};
use airboss_app::errors::ErrorCode;
use airboss_core::{Actor, NewTask, Status, TaskId};
use airboss_services::{AttentionService, NeedsKind};
use common::{TestApp, needs_item};

const M: Caller = Caller::Main;

fn status(app: &TestApp, id: TaskId) -> Status {
    app.core(|s| s.task(id).unwrap().status)
}

#[test]
fn undo_reverts_a_status_change_and_a_blocked_reason_comes_back() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    tasks::set_status(
        &app.state,
        M,
        t,
        Status::Blocked,
        Some("waiting on keys".into()),
    )
    .unwrap();
    tasks::set_status(&app.state, M, t, Status::Done, None).unwrap();
    let r = tasks::undo(&app.state, M, None).unwrap();
    assert_eq!(r.message, format!("Undid “Mark done {}”", t.0));
    assert!(r.patch.is_some());
    let task = app.core(|s| s.task(t).unwrap());
    assert_eq!(task.status, Status::Blocked);
    assert_eq!(task.blocked_reason.as_deref(), Some("waiting on keys"));
    tasks::undo(&app.state, M, None).unwrap();
    assert_eq!(status(&app, t), Status::Todo);
    let r = tasks::undo(&app.state, M, None).unwrap();
    assert_eq!(r.message, "Nothing to undo.");
    app.assert_lock_free();
}

#[test]
fn undo_puts_a_moved_task_back_where_it_was() {
    let app = TestApp::new();
    let a = app.add("A", None);
    let b = app.add("B", None);
    let p = app.add("P", None);
    app.sync_vm();
    tasks::move_task(&app.state, M, a, Some(p), None).unwrap();
    tasks::reorder(&app.state, M, b, tasks::Direction::Down).unwrap();
    tasks::undo(&app.state, M, None).unwrap();
    tasks::undo(&app.state, M, None).unwrap();
    let a_task = app.core(|s| s.task(a).unwrap());
    assert_eq!((a_task.parent, a_task.position), (None, 1));
    assert_eq!(app.core(|s| s.task(b).unwrap().position), 2);
}

#[test]
fn undoing_a_drop_restores_the_subtrees_previous_statuses() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let a = app.add("A", Some(p));
    let b = app.add("B", Some(p));
    let c = app.add("C", Some(p));
    app.core(|s| {
        s.set_status(p, Status::Doing, None, &Actor::Me).unwrap();
        s.set_status(a, Status::Doing, None, &Actor::Me).unwrap();
        s.set_status(b, Status::Blocked, Some("why"), &Actor::Me)
            .unwrap();
        s.set_status(c, Status::Done, None, &Actor::Me).unwrap();
    });
    app.sync_vm();
    let r = tasks::set_status(&app.state, M, p, Status::Dropped, None).unwrap();
    assert_eq!(r.undo.unwrap().label, format!("Drop {}", p.0));
    assert_eq!(status(&app, a), Status::Dropped);
    assert_eq!(status(&app, c), Status::Done, "done stays done");
    tasks::undo(&app.state, M, None).unwrap();
    assert_eq!(status(&app, p), Status::Doing);
    assert_eq!(status(&app, a), Status::Doing);
    let bt = app.core(|s| s.task(b).unwrap());
    assert_eq!(
        (bt.status, bt.blocked_reason.as_deref()),
        (Status::Blocked, Some("why"))
    );
    assert_eq!(status(&app, c), Status::Done);
}

#[test]
fn undo_is_refused_after_someone_else_changes_the_task() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    tasks::set_status(&app.state, M, t, Status::Doing, None).unwrap();
    app.core(|s| {
        s.set_status(t, Status::InReview, None, &Actor::Agent("t3/x".into()))
            .unwrap()
    });
    app.sync_vm();
    let e = tasks::undo(&app.state, M, None).unwrap_err();
    assert_eq!(e.code, ErrorCode::UndoRefused);
    assert_eq!(
        e.message,
        format!("{} changed since; nothing was undone.", t.0)
    );
    assert_eq!(status(&app, t), Status::InReview);
    // The entry was dropped.
    assert_eq!(
        tasks::undo(&app.state, M, None).unwrap().message,
        "Nothing to undo."
    );
}

#[test]
fn my_own_later_change_doesnt_refuse_an_undo() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    tasks::set_status(&app.state, M, t, Status::Doing, None).unwrap();
    tasks::rename_task(&app.state, M, t, "Renamed".into()).unwrap();
    tasks::undo(&app.state, M, None).unwrap();
    assert_eq!(status(&app, t), Status::Todo);
}

#[test]
fn dismiss_and_snooze_undo_through_restore() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.attention.set_items(vec![
        needs_item("a", NeedsKind::Question, t, common::NOW),
        needs_item("b", NeedsKind::Quiet, t, common::NOW),
    ]);
    app.state.catch_up_services();
    let first = needs::needs_dismiss(&app.state, M, "a".into())
        .unwrap()
        .undo
        .unwrap();
    needs::needs_snooze(&app.state, M, "b".into(), None).unwrap();
    assert_eq!(app.attention.needs_me().count, 0);
    // A toast's Undo undoes its own entry, not the newest.
    tasks::undo(&app.state, M, Some(first.id)).unwrap();
    assert_eq!(app.attention.needs_me().items[0].key.0, "a");
    tasks::undo(&app.state, M, None).unwrap();
    assert_eq!(app.attention.needs_me().count, 2);
}

fn proposal(app: &TestApp, parent: TaskId, title: &str) -> TaskId {
    app.core(|s| {
        s.add_task(
            NewTask::new(title).under(parent),
            &Actor::Agent("t3/x".into()),
        )
        .unwrap()
        .id
    })
}

#[test]
fn a_reject_is_written_after_the_delay_unless_undone() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let x = proposal(&app, p, "x");
    let y = proposal(&app, p, "y");
    app.sync_vm();
    app.state.start_background();

    let undo = tasks::reject_proposals(&app.state, M, vec![x], Some("not now".into()))
        .unwrap()
        .undo
        .unwrap();
    assert_eq!(undo.label, format!("Reject {}", x.0));
    assert_eq!(status(&app, x), Status::Proposed, "nothing written yet");
    tasks::undo(&app.state, M, None).unwrap();

    tasks::reject_proposals(&app.state, M, vec![y], None).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        status(&app, x),
        Status::Proposed,
        "the cancelled reject never lands"
    );
    assert_eq!(status(&app, y), Status::Dropped);
    let history = app.core(|s| s.history(y).unwrap());
    assert_eq!(
        history.last().unwrap().kind,
        airboss_core::EventKind::Rejected
    );
    assert!(
        app.state.row(y).is_none(),
        "a rejected proposal leaves the tree"
    );
    // Its undo entry went with it.
    assert_eq!(
        tasks::undo(&app.state, M, None).unwrap().message,
        "Nothing to undo."
    );
}

#[test]
fn pending_rejects_are_written_at_shutdown() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let x = proposal(&app, p, "x");
    app.sync_vm();
    tasks::reject_proposals(&app.state, M, vec![x], None).unwrap();
    app.state.shutdown(Duration::from_secs(1));
    assert_eq!(status(&app, x), Status::Dropped);
    for calls in [app.sync.calls(), app.launch.calls(), app.attention.calls()] {
        assert!(calls.contains(&"shutdown".to_string()));
    }
}

#[test]
fn the_stack_is_twenty_deep() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    for i in 0..21 {
        tasks::rename_task(&app.state, M, t, format!("T{i}")).unwrap();
        let to = if i % 2 == 0 {
            Status::Doing
        } else {
            Status::Todo
        };
        tasks::set_status(&app.state, M, t, to, None).unwrap();
    }
    let mut undone = 0;
    while tasks::undo(&app.state, M, None).unwrap().patch.is_some() {
        undone += 1;
    }
    assert_eq!(undone, 20);
    assert_eq!(
        status(&app, t),
        Status::Doing,
        "the oldest change fell off the stack"
    );
}

#[test]
fn the_task_menu_shows_the_newest_undo() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    tasks::set_status(&app.state, M, t, Status::Done, None).unwrap();
    let last = app
        .shell
        .calls()
        .into_iter()
        .rev()
        .find(|c| c.starts_with("update_menus"))
        .unwrap();
    assert!(last.contains(&format!("Mark done {}", t.0)), "{last}");
}

#[test]
fn undoing_a_reject_is_never_refused_by_an_agents_change() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let x = proposal(&app, p, "x");
    app.sync_vm();
    tasks::reject_proposals(&app.state, M, vec![x], None).unwrap();
    app.core(|s| {
        s.add_note(x, "more detail", &Actor::Agent("t3/x".into()))
            .unwrap()
    });
    app.sync_vm();
    let r = tasks::undo(&app.state, M, None).unwrap();
    assert!(r.message.starts_with("Undid"), "{}", r.message);
    app.state.shutdown(Duration::from_secs(1));
    assert_eq!(
        status(&app, x),
        Status::Proposed,
        "the cancelled reject is never written"
    );
}

#[test]
fn only_proposals_can_be_rejected_and_an_accepted_one_is_left_alone() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let x = proposal(&app, p, "x");
    app.sync_vm();
    assert_eq!(
        tasks::reject_proposals(&app.state, M, vec![p], None)
            .unwrap_err()
            .code,
        ErrorCode::NotAllowed
    );
    tasks::reject_proposals(&app.state, M, vec![x], None).unwrap();
    // Accepted in the meantime (from the CLI, say): the reject isn't written.
    app.core(|s| s.set_status(x, Status::Todo, None, &Actor::Me).unwrap());
    app.sync_vm();
    app.state.shutdown(Duration::from_secs(1));
    assert_eq!(status(&app, x), Status::Todo);
}

#[test]
fn dropping_a_dropped_task_makes_no_undo_entry() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    tasks::set_status(&app.state, M, t, Status::Dropped, None).unwrap();
    let again = tasks::set_status(&app.state, M, t, Status::Dropped, None).unwrap();
    assert!(again.undo.is_none());
}
