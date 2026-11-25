//! Every kind through `compute` alone: one test per kind showing, and one per rule hiding it.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use airboss_attention::AttentionConfig;
use common::*;

#[test]
fn the_defaults_are_the_em_thresholds() {
    let cfg = AttentionConfig::default();
    assert_eq!(cfg.quiet_after, DAY * 5);
    assert_eq!(cfg.snooze_for, DAY * 3);
    assert_eq!(cfg.ready_window, DAY * 7);
    assert_eq!(cfg.menu_items, 6);
    assert_eq!(cfg.settle, Duration::from_millis(150));
}

#[test]
fn nothing_in_gives_nothing_out() {
    let out = run(empty_core(), Links::new().build(), at(10, 0));
    assert!(out.items.is_empty());
    assert!(out.live_keys.is_empty());
    assert_eq!(out.next_wake, None);
}

// ---------- Questions and approvals ----------

use airboss_services::{NeedsGroup, NeedsKind, RequestKind, Target};

fn asking(r: &str, kind: RequestKind, text: &str) -> airboss_services::ThreadView {
    let mut t = thread(r, NeedsMe);
    t.request = Some(request(&format!("req-{r}"), kind, text, &ts(2, 3)));
    t
}

#[test]
fn a_question_in_a_linked_thread_shows() {
    let core = Core::new()
        .titled(5, None, Doing, &ts(1, 0), "Web: slot picker")
        .build();
    let sync = Links::new()
        .thread(
            asking(
                "a1",
                RequestKind::Question,
                "Should slots be 15 or 30 minutes?",
            ),
            &[5],
        )
        .build();
    let out = run(core, sync, at(3, 0));
    assert_eq!(out.items.len(), 1);
    let item = &out.items[0];
    assert_eq!(item.key, key("question:req-a1"));
    assert_eq!(item.kind, NeedsKind::Question);
    assert_eq!(item.group, NeedsGroup::Questions);
    assert_eq!(item.task, id(5));
    assert_eq!(item.task_title, "Web: slot picker");
    assert_eq!(item.since, ts(2, 3));
    assert_eq!(item.headline, "Should slots be 15 or 30 minutes?");
    assert_eq!(item.context, "Agent in a1-thread asks");
    assert_eq!(item.target, Target::Thread(t3("a1")));
    assert!(item.proposed.is_empty());
    assert!(!item.stale);
    assert_eq!(out.live_keys, vec![key("question:req-a1")]);
}

#[test]
fn an_approval_shows_its_first_line() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let sync = Links::new()
        .thread(
            asking(
                "a1",
                RequestKind::Approval,
                "  cargo add tray-icon\nin crates/app",
            ),
            &[5],
        )
        .build();
    let out = run(core, sync, at(3, 0));
    let item = &out.items[0];
    assert_eq!(item.key, key("approval:req-a1"));
    assert_eq!(item.kind, NeedsKind::Approval);
    assert_eq!(item.group, NeedsGroup::Questions);
    assert_eq!(item.headline, "Allow: cargo add tray-icon");
    assert_eq!(item.context, "Agent in a1-thread wants approval");
}

#[test]
fn a_question_on_a_closed_task_is_hidden() {
    for status in [Done, Dropped, Proposed] {
        let core = Core::new().task(5, None, status, &ts(1, 0)).build();
        let sync = Links::new()
            .thread(asking("a1", RequestKind::Question, "Q?"), &[5])
            .build();
        assert!(run(core, sync, at(3, 0)).items.is_empty(), "{status:?}");
    }
}

#[test]
fn every_open_status_shows_a_question() {
    for status in [Todo, Doing, InReview, Blocked] {
        let core = Core::new().task(5, None, status, &ts(1, 0)).build();
        let sync = Links::new()
            .thread(asking("a1", RequestKind::Question, "Q?"), &[5])
            .build();
        assert_eq!(run(core, sync, at(3, 0)).items.len(), 1, "{status:?}");
    }
}

#[test]
fn a_waiting_thread_without_a_request_shows_nothing() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let sync = Links::new().thread(thread("a1", NeedsMe), &[5]).build();
    assert!(run(core, sync, at(3, 0)).items.is_empty());
}

#[test]
fn an_untracked_thread_never_shows() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let mut sync = Links::new().build();
    let mut t = asking("u1", RequestKind::Question, "Q?");
    t.linked = false;
    sync.threads.insert(t.key.clone(), t);
    let mut e = thread("u2", Errored);
    e.linked = false;
    sync.threads.insert(e.key.clone(), e);
    assert!(run(core, sync, at(3, 0)).items.is_empty());
}

#[test]
fn a_thread_on_several_tasks_shows_once_at_the_lowest_open_task() {
    let core = Core::new()
        .task(4, None, Done, &ts(1, 0))
        .task(7, None, Doing, &ts(1, 0))
        .task(9, None, Doing, &ts(1, 0))
        .build();
    let sync = Links::new()
        .thread(asking("a1", RequestKind::Question, "Q?"), &[9, 4, 7])
        .build();
    let out = run(core, sync, at(3, 0));
    assert_eq!(keys(&out), vec!["question:req-a1"]);
    assert_eq!(out.items[0].task, id(7));
}

#[test]
fn a_thread_without_a_title_names_its_task() {
    let core = Core::new()
        .titled(5, None, Doing, &ts(1, 0), "Tray count")
        .build();
    let mut t = asking("a1", RequestKind::Question, "Q?");
    t.title = None;
    let sync = Links::new().thread(t, &[5]).build();
    assert_eq!(
        run(core, sync, at(3, 0)).items[0].context,
        "Agent in Tray count asks"
    );
}

#[test]
fn an_item_from_a_stale_thread_is_stale() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let mut t = asking("a1", RequestKind::Question, "Q?");
    t.stale = true;
    let sync = Links::new().thread(t, &[5]).build();
    assert!(run(core, sync, at(3, 0)).items[0].stale);
}

// ---------- Failed runs ----------

fn failed(r: &str, run: Option<&str>, error: Option<&str>) -> airboss_services::ThreadView {
    let mut t = thread(r, Errored);
    t.run_id = run.map(str::to_string);
    t.error = error.map(str::to_string);
    t.last_activity_at = Some(ts(2, 5));
    t
}

#[test]
fn a_failed_run_shows() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let sync = Links::new()
        .thread(
            failed("fix-flaky-login", Some("run-9"), Some("3 tests failed")),
            &[5],
        )
        .build();
    let out = run(core, sync, at(3, 0));
    let item = &out.items[0];
    assert_eq!(item.key, key("failed:run-9"));
    assert_eq!(item.kind, NeedsKind::FailedRun);
    assert_eq!(item.group, NeedsGroup::FailedRuns);
    assert_eq!(item.since, ts(2, 5));
    assert_eq!(
        item.headline,
        "fix-flaky-login-thread stopped: 3 tests failed"
    );
    assert_eq!(item.context, "Thread stopped · 3 tests failed");
    assert_eq!(item.target, Target::Thread(t3("fix-flaky-login")));
}

#[test]
fn a_failed_run_without_an_error_or_run_id_still_shows() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let sync = Links::new().thread(failed("a1", None, None), &[5]).build();
    let item = run(core, sync, at(3, 0)).items.remove(0);
    assert_eq!(item.key, key(&format!("failed:a1@{}", ts(2, 5))));
    assert_eq!(item.headline, "a1-thread stopped");
    assert_eq!(item.context, "Thread stopped");
}

#[test]
fn a_failed_run_on_a_closed_task_is_hidden() {
    let core = Core::new().task(5, None, Done, &ts(1, 0)).build();
    let sync = Links::new()
        .thread(failed("a1", Some("r"), Some("boom")), &[5])
        .build();
    assert!(run(core, sync, at(3, 0)).items.is_empty());
}

#[test]
fn threads_that_are_running_idle_settled_or_gone_ask_nothing() {
    for state in [Running, Idle, Settled, Gone] {
        let core = Core::new().task(5, None, Todo, &ts(1, 0)).build();
        let sync = Links::new().thread(thread("a1", state), &[5]).build();
        assert!(run(core, sync, at(1, 1)).items.is_empty(), "{state:?}");
    }
}
