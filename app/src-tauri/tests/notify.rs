//! Notifications, end to end through the app: what attention lists, the window's focus, the
//! setting and the launch rule decide; a fake records what would be posted.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use airboss_app::Caller;
use airboss_app::commands::app as appcmd;
use airboss_app::notify::Permission;
use airboss_services::NeedsKind;
use common::{NOW, TestApp, needs_item};

const M: Caller = Caller::Main;

/// Launch is over: sync has read once, and nothing was waiting.
fn launched() -> TestApp {
    let app = TestApp::new();
    app.attention.set_checked_at(Some(NOW.into()));
    app.attention.set_items(vec![]);
    app.state.catch_up_services();
    app
}

fn posted(app: &TestApp) -> Vec<String> {
    app.shell
        .notifications_fake()
        .posted()
        .into_iter()
        .map(|n| format!("{}|{}|{}", n.title, n.body, n.group))
        .collect()
}

#[test]
fn a_new_question_notifies_once_titled_by_its_task() {
    let app = launched();
    let t = app.add("Web: slot picker", None);
    app.sync_vm();
    let mut q = needs_item("question:r1", NeedsKind::Question, t, NOW);
    q.task_title = "Web: slot picker".into();
    q.headline = "Agent in slot-picker asks: Which calendar?".into();
    app.attention.set_items(vec![q.clone()]);
    app.state.catch_up_services();
    let expect = format!(
        "{} Web: slot picker|Agent in slot-picker asks: Which calendar?|task-{}",
        t.0, t.0
    );
    assert_eq!(posted(&app), vec![expect.clone()]);
    // The same key again, and a proposal: nothing more.
    let p = needs_item("proposed:1", NeedsKind::Proposed, t, NOW);
    app.attention.set_items(vec![q, p]);
    app.state.catch_up_services();
    assert_eq!(posted(&app), vec![expect]);
}

#[test]
fn nothing_while_the_window_is_focused_or_the_setting_is_off() {
    let app = launched();
    app.state.set_main_focused(true);
    app.attention.set_items(vec![needs_item(
        "question:a",
        NeedsKind::Question,
        airboss_core::TaskId(1),
        NOW,
    )]);
    app.state.catch_up_services();
    app.state.set_main_focused(false);
    let mut s = appcmd::get_settings(&app.state, M).unwrap();
    s.notifications = false;
    appcmd::set_settings(&app.state, M, s).unwrap();
    app.attention.set_items(vec![
        needs_item(
            "question:a",
            NeedsKind::Question,
            airboss_core::TaskId(1),
            NOW,
        ),
        needs_item(
            "failed:b",
            NeedsKind::FailedRun,
            airboss_core::TaskId(2),
            NOW,
        ),
    ]);
    app.state.catch_up_services();
    assert!(posted(&app).is_empty());
}

#[test]
fn what_was_waiting_at_launch_never_notifies() {
    let app = TestApp::new();
    // Sync's first read finds an open question, as at a real launch.
    app.attention.set_checked_at(Some(NOW.into()));
    app.attention.set_items(vec![needs_item(
        "question:old",
        NeedsKind::Question,
        airboss_core::TaskId(1),
        NOW,
    )]);
    app.state.catch_up_services();
    assert!(posted(&app).is_empty());
}

#[test]
fn turning_notifications_on_asks_macos_when_it_hasnt_been_asked() {
    let app = TestApp::new();
    let fake = app.shell.notifications_fake();
    fake.set_permission(Permission::NotAsked);
    assert_eq!(
        appcmd::notification_permission(&app.state, M).unwrap(),
        Permission::NotAsked
    );
    let mut s = appcmd::get_settings(&app.state, M).unwrap();
    assert!(s.notifications, "on by default");
    s.notifications = false;
    appcmd::set_settings(&app.state, M, s.clone()).unwrap();
    assert_eq!(fake.requests(), 0);
    s.notifications = true;
    appcmd::set_settings(&app.state, M, s.clone()).unwrap();
    assert_eq!(fake.requests(), 1);
    // Denied: turning it on again doesn't ask (macOS wouldn't show the prompt again).
    fake.set_permission(Permission::Denied);
    s.notifications = false;
    appcmd::set_settings(&app.state, M, s.clone()).unwrap();
    s.notifications = true;
    appcmd::set_settings(&app.state, M, s).unwrap();
    assert_eq!(fake.requests(), 1);
    appcmd::open_notification_settings(&app.state, M).unwrap();
}
