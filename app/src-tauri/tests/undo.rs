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
    assert_eq!(r.message, format!("Undid “Mark done #{}”", t.0));
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
        s.set_status(a, Status::Doing, None, &Actor::Me).unwrap();
        s.set_status(b, Status::Blocked, Some("why"), &Actor::Me)
            .unwrap();
        s.set_status(c, Status::Done, None, &Actor::Me).unwrap();
    });
    app.sync_vm();
    let r = tasks::set_status(&app.state, M, p, Status::Dropped, None).unwrap();
    assert_eq!(r.undo.unwrap().label, format!("Drop #{}", p.0));
    assert_eq!(status(&app, a), Status::Dropped);
    assert_eq!(status(&app, c), Status::Done, "done stays done");
    assert_eq!(
        status(&app, p),
        Status::Done,
        "its open steps dropped, one done"
    );
    tasks::undo(&app.state, M, None).unwrap();
    // It comes back through its steps (SPEC-core rule 10): B is blocked again.
    assert_eq!(status(&app, p), Status::Blocked);
    assert_eq!(status(&app, a), Status::Doing);
    let bt = app.core(|s| s.task(b).unwrap());
    assert_eq!(
        (bt.status, bt.blocked_reason.as_deref()),
        (Status::Blocked, Some("why"))
    );
    assert_eq!(status(&app, c), Status::Done);
}

#[test]
fn a_parents_status_follows_its_steps_so_only_drop_changes_it() {
    let app = TestApp::new();
    let p = app.add("P", None);
    app.add("A", Some(p));
    app.sync_vm();
    for to in [Status::Done, Status::Doing] {
        let e = tasks::set_status(&app.state, M, p, to, None).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidTarget);
        assert_eq!(e.message, airboss_app::errors::MSG_HAS_STEPS);
    }
    let e = tasks::toggle_done(&app.state, M, p).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidTarget);
    assert_eq!(status(&app, p), Status::Todo);
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
        format!("#{} changed since; nothing was undone.", t.0)
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
    assert_eq!(undo.label, format!("Reject #{}", x.0));
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
    assert!(last.contains(&format!("Mark done #{}", t.0)), "{last}");
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
    // Accepted in the meantime (from the CLI, say): the reject isn't written, and says so.
    app.core(|s| s.set_status(x, Status::Todo, None, &Actor::Me).unwrap());
    app.sync_vm();
    app.state.shutdown(Duration::from_secs(1));
    assert_eq!(status(&app, x), Status::Todo);
    assert_eq!(
        toasts(&app),
        vec![format!("#{} was already accepted; left as is", x.0)]
    );
}

fn toasts(app: &TestApp) -> Vec<String> {
    app.shell
        .events()
        .into_iter()
        .filter_map(|e| match e {
            airboss_app::UiEvent::Toast(t) => Some(t.text),
            _ => None,
        })
        .collect()
}

/// One reject of several, written through core's `reject_proposals` in one transaction:
/// what's still proposed is rejected, the rest is left as is and named in one toast.
#[test]
fn a_reject_all_writes_what_is_still_proposed_and_names_the_rest() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|t| proposal(&app, p, t));
    app.sync_vm();
    tasks::reject_proposals(&app.state, M, vec![a, b, c, d], None).unwrap();
    app.core(|s| {
        s.set_status(b, Status::Todo, None, &Actor::Me).unwrap();
        s.set_status(c, Status::Doing, None, &Actor::Me).unwrap();
        // Rejected elsewhere already (`airboss reject`).
        s.set_status(d, Status::Dropped, None, &Actor::Me).unwrap();
    });
    app.sync_vm();
    app.state.shutdown(Duration::from_secs(1));
    assert_eq!(status(&app, a), Status::Dropped);
    assert_eq!(
        (status(&app, b), status(&app, c)),
        (Status::Todo, Status::Doing)
    );
    assert_eq!(
        toasts(&app),
        vec![format!(
            "#{} and #{} were already accepted and #{} already dropped; left as is",
            b.0, c.0, d.0
        )]
    );
}

#[test]
fn a_proposal_dropped_meanwhile_says_already_dropped() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let x = proposal(&app, p, "x");
    app.sync_vm();
    tasks::reject_proposals(&app.state, M, vec![x], None).unwrap();
    app.core(|s| s.set_status(x, Status::Dropped, None, &Actor::Me).unwrap());
    app.sync_vm();
    app.state.shutdown(Duration::from_secs(1));
    assert_eq!(
        toasts(&app),
        vec![format!("#{} was already dropped; left as is", x.0)]
    );
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

/// Audit should-fix 1: rule 9 drops an agent's proposals with their parent, and `me` can't
/// make a proposal again (rule 6). Undo used to bring them back as todo, accepting them
/// without me; now they stay dropped, and the message says so.
#[test]
fn undoing_a_drop_leaves_the_agents_proposals_dropped() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let a = app.add("A", Some(p));
    app.core(|s| {
        s.set_status(a, Status::Doing, None, &Actor::Me).unwrap();
    });
    let x = proposal(&app, p, "x");
    let y = proposal(&app, p, "y");
    app.sync_vm();
    tasks::set_status(&app.state, M, p, Status::Dropped, None).unwrap();
    assert_eq!(
        status(&app, x),
        Status::Dropped,
        "rule 9 drops proposals too"
    );
    let r = tasks::undo(&app.state, M, None).unwrap();
    assert_eq!(status(&app, p), Status::Doing);
    assert_eq!(status(&app, a), Status::Doing);
    for proposal in [x, y] {
        assert_eq!(
            status(&app, proposal),
            Status::Dropped,
            "a proposal never comes back accepted"
        );
    }
    assert_eq!(
        r.message,
        format!("Undid “Drop #{}”. Its 2 proposals stay dropped.", p.0)
    );
}

/// Dropping a proposal by itself is a reject, which can't be undone once written.
#[test]
fn dropping_a_proposal_makes_no_undo_entry_that_would_accept_it() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let x = proposal(&app, p, "x");
    app.sync_vm();
    let w = tasks::set_status(&app.state, M, x, Status::Dropped, None).unwrap();
    assert!(w.undo.is_none());
    assert_eq!(
        tasks::undo(&app.state, M, None).unwrap().message,
        "Nothing to undo."
    );
    assert_eq!(status(&app, x), Status::Dropped);
}

/// Audit should-fix 2: an agent sets in_review, and I press Space before the app catches up.
/// The view model still says doing, so ⌘Z must be refused, not put doing back over the
/// agent's change.
#[test]
fn undo_is_refused_when_an_agent_wrote_before_the_app_caught_up() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.core(|s| s.set_status(t, Status::Doing, None, &Actor::Me).unwrap());
    app.sync_vm();
    // The agent's write; no catch-up runs before my key press.
    app.core(|s| {
        s.set_status(t, Status::InReview, None, &Actor::Agent("t3/x".into()))
            .unwrap()
    });
    tasks::toggle_done(&app.state, M, t).unwrap();
    assert_eq!(status(&app, t), Status::Done);
    let e = tasks::undo(&app.state, M, None).unwrap_err();
    assert_eq!(e.code, ErrorCode::UndoRefused);
    assert_eq!(status(&app, t), Status::Done, "nothing was undone");
}

/// The same for a move: the position came from the view model, so an agent's change to the
/// task before the catch-up refuses the undo.
#[test]
fn a_move_undo_is_refused_when_an_agent_wrote_before_the_app_caught_up() {
    let app = TestApp::new();
    let a = app.add("A", None);
    app.add("B", None);
    app.sync_vm();
    app.core(|s| {
        s.add_note(a, "progress", &Actor::Agent("t3/x".into()))
            .unwrap()
    });
    tasks::reorder(&app.state, M, a, tasks::Direction::Down).unwrap();
    assert_eq!(
        tasks::undo(&app.state, M, None).unwrap_err().code,
        ErrorCode::UndoRefused
    );
}

/// The siblings the tree shows, in order: rejected proposals are gone from the tree, and
/// dropped tasks are hidden in tree layouts.
fn shown_children(app: &TestApp, parent: TaskId) -> Vec<TaskId> {
    app.core(|s| s.tree().unwrap())
        .into_iter()
        .filter(|n| n.task.parent == Some(parent) && n.task.status != Status::Dropped)
        .map(|n| n.task.id)
        .collect()
}

/// Audit should-fix 3: ⌥⌘↑/↓ next to a hidden sibling (a rejected proposal, a dropped task)
/// moves past it to the visible neighbour, and an end move changes nothing and pushes no
/// undo entry.
#[test]
fn reorder_moves_past_hidden_siblings_and_only_a_real_move_is_undoable() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let a = app.add("A", Some(p));
    let x = proposal(&app, p, "x");
    let d = app.add("D", Some(p));
    let b = app.add("B", Some(p));
    app.core(|s| {
        // Rejected, so gone from the tree; and a plain drop, hidden in tree layouts.
        s.set_status(x, Status::Dropped, None, &Actor::Me).unwrap();
        s.set_status(d, Status::Dropped, None, &Actor::Me).unwrap();
    });
    app.sync_vm();
    assert_eq!(shown_children(&app, p), vec![a, b]);

    let w = tasks::reorder(&app.state, M, a, tasks::Direction::Down).unwrap();
    assert_eq!(
        shown_children(&app, p),
        vec![b, a],
        "A moved past the hidden ones"
    );
    assert_eq!(w.undo.unwrap().label, format!("Move #{}", a.0));

    // At either end nothing happens, and nothing joins the undo stack.
    for (id, dir) in [(a, tasks::Direction::Down), (b, tasks::Direction::Up)] {
        let w = tasks::reorder(&app.state, M, id, dir).unwrap();
        assert!(w.undo.is_none() && w.patch.is_none(), "{id:?} {dir:?}");
    }
    assert_eq!(shown_children(&app, p), vec![b, a]);

    tasks::reorder(&app.state, M, a, tasks::Direction::Up).unwrap();
    assert_eq!(shown_children(&app, p), vec![a, b]);
    tasks::undo(&app.state, M, None).unwrap();
    assert_eq!(shown_children(&app, p), vec![b, a]);
    tasks::undo(&app.state, M, None).unwrap();
    assert_eq!(shown_children(&app, p), vec![a, b]);
    assert_eq!(
        tasks::undo(&app.state, M, None).unwrap().message,
        "Nothing to undo."
    );
}

/// Holds SQLite's write lock on `db` from another connection for `hold`, as a long write by
/// `airboss mcp` would. Returns once the lock is taken.
fn hold_write_lock(db: &std::path::Path, hold: Duration) -> std::thread::JoinHandle<()> {
    let (taken, ready) = std::sync::mpsc::channel();
    let db = db.to_path_buf();
    let h = std::thread::spawn(move || {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        taken.send(()).unwrap();
        std::thread::sleep(hold);
        conn.execute_batch("COMMIT").unwrap();
    });
    ready.recv().unwrap();
    h
}

fn on_file(name: &str) -> (TestApp, std::path::PathBuf) {
    let dir = common::temp_dir(name);
    let db = dir.join("airboss.db");
    (
        TestApp::with_store(airboss_core::Store::open(&db).unwrap()),
        db,
    )
}

/// Audit should-fix 4: a reject the database was too busy to take is retried.
#[test]
fn a_reject_is_retried_while_the_database_is_busy() {
    let (app, db) = on_file("reject-busy");
    let p = app.add("P", None);
    let x = proposal(&app, p, "x");
    app.sync_vm();
    tasks::reject_proposals(&app.state, M, vec![x], None).unwrap();
    // Longer than one attempt's 2 s busy timeout, shorter than two.
    let holder = hold_write_lock(&db, Duration::from_millis(2300));
    app.state.shutdown(Duration::from_secs(1));
    holder.join().unwrap();
    assert_eq!(
        status(&app, x),
        Status::Dropped,
        "the second attempt landed"
    );
    assert!(
        !app.shell
            .events()
            .iter()
            .any(|e| matches!(e, airboss_app::UiEvent::Toast(_))),
        "no toast when it worked"
    );
}

/// …and one that still fails says so, with Retry, instead of the proposal quietly coming back.
#[test]
fn a_reject_that_still_fails_shows_a_toast_with_retry() {
    let (app, db) = on_file("reject-fails");
    let p = app.add("P", None);
    let x = proposal(&app, p, "x");
    app.sync_vm();
    tasks::reject_proposals(&app.state, M, vec![x], Some("not now".into())).unwrap();
    let holder = hold_write_lock(&db, Duration::from_millis(4600));
    app.state.write_rejects_now();
    holder.join().unwrap();
    assert_eq!(status(&app, x), Status::Proposed, "nothing was written");
    let toast = app
        .shell
        .events()
        .into_iter()
        .find_map(|e| match e {
            airboss_app::UiEvent::Toast(t) => Some(t),
            _ => None,
        })
        .expect("a toast");
    assert_eq!(
        toast.text,
        format!("Couldn't reject #{}: the database stayed busy.", x.0)
    );
    assert_eq!(toast.tone, Some(airboss_app::dto::ToastTone::Warning));
    let retry = &toast.actions[0];
    assert_eq!(
        (retry.label.as_str(), retry.command.as_str()),
        ("Retry", "reject_proposals")
    );
    assert_eq!(
        serde_json::to_value(&retry.args).unwrap(),
        serde_json::json!({ "ids": [x.0], "reason": "not now" })
    );
    // Retry is the same reject again; this time the database is free.
    tasks::reject_proposals(&app.state, M, vec![x], Some("not now".into())).unwrap();
    app.state.shutdown(Duration::from_secs(1));
    assert_eq!(status(&app, x), Status::Dropped);
}
